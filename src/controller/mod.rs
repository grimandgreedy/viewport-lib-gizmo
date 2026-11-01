//! Object manipulation controller: move, rotate, and scale with axis constraints.
//!
//! # Quick start
//!
//! ```rust,ignore
//! let mut manip = ManipulationController::new();
//!
//! // Each frame:
//! let result = manip.update(&frame, ManipulationContext { ... });
//! match result {
//!     ManipResult::Update(delta) => { /* apply delta to selected objects */ }
//!     ManipResult::Commit        => { /* finalize / push undo */ }
//!     ManipResult::Cancel        => { /* restore snapshot */ }
//!     ManipResult::None          => {}
//! }
//!
//! // Suppress orbit while manipulating:
//! if manip.is_active() {
//!     orbit_controller.resolve();
//! } else {
//!     orbit_controller.apply_to_camera(&mut camera);
//! }
//! ```

pub mod driver;
pub mod types;
mod views;

mod session;

pub use driver::{FrameInput, GizmoDriver};
pub use types::*;
pub use views::ActiveView;

use crate::maths::gizmo::{Gizmo, GizmoAxis, GizmoMode, GizmoSpace};
use crate::maths::snap::{SnapConfig, snap_value, snap_vec3};
use crate::maths::solvers;
use session::{ManipulationSession, update_constraint, update_numeric_state};
use viewport_lib_types::input::{Action, ActionFrame};

/// Manages a single object-manipulation session (G/R/S + axis constraints + gizmo drag).
///
/// Owns all session state; the app only supplies per-frame context and applies the
/// resulting [`TransformDelta`].
pub struct ManipulationController {
    session: Option<ManipulationSession>,
    /// Snap increments applied while dragging. Default is all-`None` (no snapping),
    /// so a controller that never calls [`set_snap`](Self::set_snap) behaves exactly
    /// as before. Snapping rounds the cumulative transform (not the per-frame delta),
    /// so the object steps cleanly between grid stops without accumulating drift.
    snap: SnapConfig,
}

impl ManipulationController {
    /// Create a controller with no active session and no snapping.
    pub fn new() -> Self {
        Self {
            session: None,
            snap: SnapConfig::default(),
        }
    }

    /// Set the snap increments used while dragging.
    ///
    /// Each field is an optional increment: translation in world units, rotation in
    /// radians, scale as a fraction. `None` (or a non-positive increment) disables
    /// snapping on that channel. Rotation snapping applies to single-axis rotations
    /// only, where a snapped angle is well defined. Safe to change mid-drag, e.g. to
    /// bind snapping to a held modifier key.
    pub fn set_snap(&mut self, snap: SnapConfig) {
        self.snap = snap;
    }

    /// Builder form of [`set_snap`](Self::set_snap).
    pub fn with_snap(mut self, snap: SnapConfig) -> Self {
        self.snap = snap;
        self
    }

    /// Drive the controller for one frame.
    ///
    /// Priority order:
    /// 1. Confirm (Enter, or left-click while not a gizmo drag) -> [`ManipResult::Commit`]
    /// 2. Cancel (Escape, or the selection going away: `selection_center` is
    ///    `None`) -> [`ManipResult::Cancel`]
    /// 3. Gizmo drag release -> [`ManipResult::Commit`]
    /// 4. Update constraints and numeric input
    /// 5. Compute and return [`ManipResult::Update`]
    /// 6. Gizmo drag start -> begins session, returns [`ManipResult::None`] this frame
    /// 7. G/R/S keys (when `selection_center` is `Some`) -> begins session
    /// 8. Otherwise -> [`ManipResult::None`]
    pub fn update(&mut self, frame: &ActionFrame, ctx: ManipulationContext) -> ManipResult {
        let snap = self.snap.clone();
        if let Some(ref mut session) = self.session {
            // 1. Confirm: Enter key, or left-click when not a gizmo drag.
            let click_confirm = ctx.clicked && !session.is_gizmo_drag;
            if frame.is_active(Action::Confirm) || click_confirm {
                self.session = None;
                return ManipResult::Commit;
            }

            // 2. Cancel: Escape key, or nothing left to manipulate.
            if frame.is_active(Action::Cancel) || ctx.selection_center.is_none() {
                self.session = None;
                return ManipResult::Cancel;
            }

            // 3. Gizmo drag released.
            if session.is_gizmo_drag && !ctx.dragging {
                self.session = None;
                return ManipResult::Commit;
            }

            // 4. Constraint and numeric updates.
            let axis_before = session.axis;
            let exclude_before = session.exclude_axis;
            update_constraint(
                session,
                frame.is_active(Action::ConstrainX),
                frame.is_active(Action::ConstrainY),
                frame.is_active(Action::ConstrainZ),
                frame.is_active(Action::ExcludeX),
                frame.is_active(Action::ExcludeY),
                frame.is_active(Action::ExcludeZ),
            );
            update_numeric_state(session, frame);

            // If the constraint changed, reset the cursor anchor so the next
            // frame's delta is computed relative to the current cursor position
            // with the new constraint : and tell the app to restore its snapshot.
            if session.axis != axis_before || session.exclude_axis != exclude_before {
                session.cursor_anchor = ctx.cursor_viewport;
                session.cursor_last_total = glam::Vec2::ZERO;
                session.cumulative_scale = 1.0;
                // The app restores its snapshot on this result, so the cumulative
                // accumulators must start fresh for the new constraint too.
                session.cumulative_translation = glam::Vec3::ZERO;
                session.cumulative_angle = 0.0;
                session.cumulative_rotation = glam::Quat::IDENTITY;
                session.cumulative_scale = 1.0;
                return ManipResult::ConstraintChanged;
            }

            // 5. Compute delta.
            //
            // Prefer absolute-cursor arithmetic over raw pointer_delta so that
            // the per-frame increment is stable even if the OS coalesces events.
            // Falls back to ctx.pointer_delta when cursor_viewport is unavailable.
            let pointer_delta = if session.numeric.is_some() {
                glam::Vec2::ZERO
            } else {
                match (ctx.cursor_viewport, session.cursor_anchor) {
                    (Some(current), Some(anchor)) => {
                        let total = current - anchor;
                        let increment = total - session.cursor_last_total;
                        session.cursor_last_total = total;
                        increment
                    }
                    (Some(current), None) => {
                        // The session began with the cursor outside the viewport:
                        // anchor where it first appears.
                        session.cursor_anchor = Some(current);
                        session.cursor_last_total = glam::Vec2::ZERO;
                        glam::Vec2::ZERO
                    }
                    (None, _) => {
                        // Outside the viewport. Count the reported delta against
                        // the anchor too, so the move is not counted again when
                        // the cursor comes back.
                        session.cursor_last_total += ctx.pointer_delta;
                        ctx.pointer_delta
                    }
                }
            };

            let mut delta = TransformDelta::default();

            let camera_view = ctx.camera.view_matrix();
            let view_proj = ctx.camera.proj_matrix() * camera_view;

            match session.kind {
                ManipulationKind::Move => {
                    // With the cursor in view, intersect the anchor's and the
                    // cursor's rays with the constraint: the total is then exact,
                    // keeps the grabbed point under the cursor, and puts it back
                    // there after the cursor has been outside the viewport.
                    // Without a cursor, while typing, or where the rays give no
                    // stable answer, add the scaled pointer delta instead.
                    let exact = match (ctx.cursor_viewport, session.cursor_anchor) {
                        (Some(cursor), Some(anchor)) if session.numeric.is_none() => {
                            solvers::cursor_translation(
                                anchor,
                                cursor,
                                session.axis,
                                session.exclude_axis,
                                session.orientation,
                                session.gizmo_center,
                                &ctx.camera,
                                ctx.viewport_size,
                            )
                        }
                        _ => None,
                    };
                    match exact {
                        Some(total) => session.cumulative_translation = total,
                        None => {
                            session.cumulative_translation += solvers::oriented_translation(
                                pointer_delta,
                                session.axis,
                                session.exclude_axis,
                                session.orientation,
                                session.gizmo_center,
                                &ctx.camera,
                                ctx.viewport_size,
                            )
                        }
                    }
                    // Emit the total translation since the drag began. Snapping
                    // rounds the cumulative (not a per-frame step) so the object
                    // sits on grid stops without drift.
                    delta.translation = match snap.translation {
                        Some(inc) if inc > 0.0 => snap_vec3(session.cumulative_translation, inc),
                        _ => session.cumulative_translation,
                    };
                    // Numeric position override.
                    if let Some(ref numeric) = session.numeric {
                        delta.position_override = numeric.parsed_values();
                    }
                }

                ManipulationKind::Rotate => {
                    let twist = frame.navigation.twist;
                    let rot = if let Some(ax) = session.axis {
                        if session.exclude_axis {
                            // Excluded axis: rotate around the dominant of the two
                            // remaining axes. The chosen axis can change between
                            // frames, so accumulate the per-frame rotation into a
                            // running total quaternion.
                            let (ax1, ax2) = solvers::excluded_axes(ax);
                            let (ax1, ax2) = (session.orientation * ax1, session.orientation * ax2);
                            let a1 = solvers::drag_onto_rotation(pointer_delta, ax1, camera_view);
                            let a2 = solvers::drag_onto_rotation(pointer_delta, ax2, camera_view);
                            let (chosen_axis, drag_angle) = if a1.abs() >= a2.abs() {
                                (ax1, a1)
                            } else {
                                (ax2, a2)
                            };
                            let frame_rot =
                                glam::Quat::from_axis_angle(chosen_axis, drag_angle + twist);
                            session.cumulative_rotation = frame_rot * session.cumulative_rotation;
                            session.cumulative_rotation
                        } else {
                            // Constrained to a single axis: angular sweep around screen center.
                            let axis_world = session.orientation * solvers::gizmo_axis_to_vec3(ax);
                            let angle = solvers::angular_rotation_from_cursor(
                                ctx.cursor_viewport,
                                pointer_delta,
                                session.gizmo_center,
                                axis_world,
                                view_proj,
                                ctx.viewport_size,
                                camera_view,
                            ) + twist;
                            session.cumulative_angle += angle;
                            // Emit the total rotation since the drag began. Snapping
                            // rounds the cumulative angle so rotation clicks between
                            // fixed steps (e.g. 15 deg).
                            let total = match snap.rotation {
                                Some(inc) if inc > 0.0 => snap_value(session.cumulative_angle, inc),
                                _ => session.cumulative_angle,
                            };
                            glam::Quat::from_axis_angle(axis_world, total)
                        }
                    } else {
                        // Unconstrained: rotate about the view axis, accumulating the
                        // per-frame rotation into a running total (the camera, and so
                        // the axis, can change between frames). With the cursor in
                        // view the object turns with the cursor's sweep around the
                        // pivot, the same as a single-axis rotation; without one,
                        // horizontal pointer motion turns it.
                        let toward_camera = -ctx.camera.forward();
                        let angle = match ctx.cursor_viewport {
                            Some(_) => solvers::angular_rotation_from_cursor(
                                ctx.cursor_viewport,
                                pointer_delta,
                                session.gizmo_center,
                                toward_camera,
                                view_proj,
                                ctx.viewport_size,
                                camera_view,
                            ),
                            None => -pointer_delta.x * 0.01,
                        };
                        let frame_rot = glam::Quat::from_axis_angle(toward_camera, angle + twist);
                        session.cumulative_rotation = frame_rot * session.cumulative_rotation;
                        session.cumulative_rotation
                    };
                    delta.rotation = rot;
                    // Numeric rotation override (per-axis angle). The app interprets
                    // the value; the drag-driven rotation above is ignored when set.
                    if let Some(ref numeric) = session.numeric {
                        delta.rotation_override = numeric.parsed_values();
                    }
                }

                ManipulationKind::Scale => {
                    // Project the pivot into viewport-pixel space.
                    let ndc = view_proj.project_point3(session.gizmo_center);
                    let center_screen = glam::Vec2::new(
                        (ndc.x + 1.0) * 0.5 * ctx.viewport_size.x,
                        (1.0 - ndc.y) * 0.5 * ctx.viewport_size.y,
                    );

                    // Cumulative scale factor = current distance / anchor distance.
                    // Moving toward the centre shrinks; moving away (or passing through
                    // and out the other side) grows. An anchor close to the centre
                    // (a grab on the centre handle) would make that ratio jump, or
                    // divide by zero, so the change is measured against at least
                    // SCALE_MIN_REFERENCE_PX instead.
                    let cumulative = match (ctx.cursor_viewport, session.cursor_anchor) {
                        (Some(cursor), Some(anchor)) => {
                            let dist_anchor = (anchor - center_screen).length();
                            let dist_now = (cursor - center_screen).length();
                            let reference = dist_anchor.max(SCALE_MIN_REFERENCE_PX);
                            (1.0 + (dist_now - dist_anchor) / reference).max(0.001)
                        }
                        _ => {
                            // Fallback when cursor is unavailable: integrate pointer_delta.
                            (session.cumulative_scale
                                * (1.0 + pointer_delta.x * 4.0 / ctx.viewport_size.x.max(1.0)))
                            .max(0.001)
                        }
                    };

                    // Snap the cumulative factor (not a per-frame step) so scaling
                    // clicks between stops, then emit the total factor since the
                    // drag began.
                    let total = match snap.scale {
                        Some(inc) if inc > 0.0 => snap_value(cumulative, inc).max(0.001),
                        _ => cumulative,
                    };
                    session.cumulative_scale = total;

                    delta.scale = match (session.axis, session.exclude_axis) {
                        (None, _) => glam::Vec3::splat(total),
                        (Some(GizmoAxis::X), false) => glam::Vec3::new(total, 1.0, 1.0),
                        (Some(GizmoAxis::Y), false) => glam::Vec3::new(1.0, total, 1.0),
                        (Some(_), false) => glam::Vec3::new(1.0, 1.0, total),
                        (Some(GizmoAxis::X), true) => glam::Vec3::new(1.0, total, total),
                        (Some(GizmoAxis::Y), true) => glam::Vec3::new(total, 1.0, total),
                        (Some(_), true) => glam::Vec3::new(total, total, 1.0),
                    };

                    // Numeric scale override.
                    if let Some(ref numeric) = session.numeric {
                        delta.scale_override = numeric.parsed_values();
                    }
                }
            }

            return ManipResult::Update(delta);
        }

        // No active session : check for session starts.

        // 6. Gizmo drag start.
        if ctx.drag_started {
            if let (Some(gizmo_info), Some(center), Some(cursor)) =
                (&ctx.gizmo, ctx.selection_center, ctx.cursor_viewport)
            {
                // Build a ray from the cursor position.
                let (ray_origin, ray_dir) =
                    crate::maths::gizmo::cursor_ray(&ctx.camera, cursor, ctx.viewport_size);

                // `GizmoInfo::orientation` is already resolved for the space
                // (identity in world space), so pick in it as given.
                let temp_gizmo = Gizmo {
                    mode: gizmo_info.mode,
                    space: GizmoSpace::Local,
                    hovered_axis: GizmoAxis::None,
                    active_axis: GizmoAxis::None,
                    drag_start_mouse: None,
                    pivot_mode: crate::maths::gizmo::PivotMode::SelectionCentroid,
                };
                let hit = temp_gizmo.hit_test_oriented(
                    ray_origin,
                    ray_dir,
                    gizmo_info.center,
                    gizmo_info.scale,
                    gizmo_info.orientation,
                );

                if hit != GizmoAxis::None {
                    let kind = match gizmo_info.mode {
                        GizmoMode::Translate => ManipulationKind::Move,
                        GizmoMode::Rotate => ManipulationKind::Rotate,
                        GizmoMode::Scale => ManipulationKind::Scale,
                    };
                    let (axis, exclude_axis) = hit_to_constraint(hit);
                    self.session = Some(ManipulationSession {
                        kind,
                        axis,
                        exclude_axis,
                        numeric: None,
                        is_gizmo_drag: true,
                        gizmo_center: center,
                        orientation: gizmo_info.orientation,
                        cursor_anchor: ctx.cursor_viewport,
                        cursor_last_total: glam::Vec2::ZERO,
                        cumulative_scale: 1.0,
                        ..Default::default()
                    });
                    return ManipResult::None;
                }
            }
        }

        // 7. G/R/S keyboard shortcuts.
        if let Some(center) = ctx.selection_center {
            let kind = if frame.is_active(Action::BeginMove) {
                Some(ManipulationKind::Move)
            } else if frame.is_active(Action::BeginRotate) {
                Some(ManipulationKind::Rotate)
            } else if frame.is_active(Action::BeginScale) {
                Some(ManipulationKind::Scale)
            } else {
                None
            };

            if let Some(kind) = kind {
                self.session = Some(ManipulationSession {
                    kind,
                    axis: None,
                    exclude_axis: false,
                    numeric: None,
                    is_gizmo_drag: false,
                    gizmo_center: center,
                    // Constraint keys work in the gizmo's space.
                    orientation: ctx.gizmo.map_or(glam::Quat::IDENTITY, |g| g.orientation),
                    cursor_anchor: ctx.cursor_viewport,
                    cursor_last_total: glam::Vec2::ZERO,
                    cumulative_scale: 1.0,
                    ..Default::default()
                });
                return ManipResult::None;
            }
        }

        ManipResult::None
    }

    /// Returns `true` when a manipulation session is in progress.
    ///
    /// Use this to suppress camera orbit:
    /// ```rust,ignore
    /// if manip.is_active() { orbit.resolve() } else { orbit.apply_to_camera(&mut cam) }
    /// ```
    pub fn is_active(&self) -> bool {
        self.session.is_some()
    }

    /// Returns an inspectable snapshot of the current session, or `None` when idle.
    pub fn state(&self) -> Option<ManipulationState> {
        self.session.as_ref().map(|s| s.to_state())
    }

    /// Force-begin a manipulation (e.g. from a UI button).
    ///
    /// No-op if a session is already active.
    pub fn begin(&mut self, kind: ManipulationKind, center: glam::Vec3) {
        if self.session.is_some() {
            return;
        }
        self.session = Some(ManipulationSession {
            kind,
            axis: None,
            exclude_axis: false,
            numeric: None,
            is_gizmo_drag: false,
            gizmo_center: center,
            cursor_anchor: None,
            cursor_last_total: glam::Vec2::ZERO,
            cumulative_scale: 1.0,
            ..Default::default()
        });
    }

    /// Force-cancel any active session without emitting [`ManipResult::Cancel`].
    pub fn reset(&mut self) {
        self.session = None;
    }
}

impl Default for ManipulationController {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// The smallest cursor-to-pivot distance, in pixels, a scale drag measures its
/// change against. About half the gizmo's on-screen length, so a drag that
/// starts on the centre handle doubles the size over that distance.
const SCALE_MIN_REFERENCE_PX: f32 = 50.0;

/// Normalise a hit gizmo axis into the `(axis, exclude)` constraint the solvers
/// understand.
///
/// The single-axis handles map straight through. A plane handle becomes an
/// *exclude* of the third axis, so translation and scale happen in that plane
/// (XY plane = exclude Z, and so on). The screen handle becomes a free
/// (unconstrained) manipulation: camera-plane translation or uniform scale.
/// Without this, plane and screen handles fell through to the Z axis.
fn hit_to_constraint(hit: GizmoAxis) -> (Option<GizmoAxis>, bool) {
    match hit {
        GizmoAxis::X => (Some(GizmoAxis::X), false),
        GizmoAxis::Y => (Some(GizmoAxis::Y), false),
        GizmoAxis::Z => (Some(GizmoAxis::Z), false),
        GizmoAxis::XY => (Some(GizmoAxis::Z), true),
        GizmoAxis::XZ => (Some(GizmoAxis::Y), true),
        GizmoAxis::YZ => (Some(GizmoAxis::X), true),
        GizmoAxis::Screen | GizmoAxis::None => (None, false),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::maths::snap::SnapConfig;
    use session::{NumericInputState, update_constraint};
    use viewport_lib_types::input::ActionFrame;

    fn make_camera() -> viewport_lib_types::camera::Camera {
        viewport_lib_types::camera::Camera::default()
    }

    /// A frame with no pointer input and a selection at the origin. A session
    /// cancels without a selection, so tests that drive one need it.
    fn idle_ctx() -> ManipulationContext {
        ManipulationContext {
            camera: make_camera(),
            viewport_size: glam::Vec2::new(800.0, 600.0),
            cursor_viewport: None,
            pointer_delta: glam::Vec2::ZERO,
            selection_center: Some(glam::Vec3::ZERO),
            gizmo: None,
            drag_started: false,
            dragging: false,
            clicked: false,
        }
    }

    // -----------------------------------------------------------------------
    // Constraint transition tests
    // -----------------------------------------------------------------------

    #[test]
    fn constraint_transitions_x_y_shift_z() {
        let mut session = ManipulationSession {
            kind: ManipulationKind::Move,
            axis: None,
            exclude_axis: false,
            numeric: None,
            is_gizmo_drag: false,
            gizmo_center: glam::Vec3::ZERO,
            cursor_anchor: None,
            cursor_last_total: glam::Vec2::ZERO,
            cumulative_scale: 1.0,
            ..Default::default()
        };

        // X: constrained, not excluded.
        update_constraint(&mut session, true, false, false, false, false, false);
        assert_eq!(session.axis, Some(GizmoAxis::X));
        assert!(!session.exclude_axis);

        // Y: constrained, not excluded.
        update_constraint(&mut session, false, true, false, false, false, false);
        assert_eq!(session.axis, Some(GizmoAxis::Y));
        assert!(!session.exclude_axis);

        // Shift+Z: excluded.
        update_constraint(&mut session, false, false, false, false, false, true);
        assert_eq!(session.axis, Some(GizmoAxis::Z));
        assert!(session.exclude_axis);
    }

    // -----------------------------------------------------------------------
    // Numeric parse test (deferred : Action enum lacks NumericDigit/Backspace/Tab)
    // -----------------------------------------------------------------------

    #[test]
    fn numeric_parse_x_axis() {
        let mut state = NumericInputState::new(Some(GizmoAxis::X), false);
        state.axis_inputs[0] = "2.50".to_string();
        let parsed = state.parsed_values();
        assert_eq!(parsed[0], Some(2.5));
        assert_eq!(parsed[1], None);
        assert_eq!(parsed[2], None);
    }

    #[test]
    fn numeric_input_bootstraps_on_first_digit() {
        let mut ctrl = ManipulationController::new();
        let center = glam::Vec3::new(1.0, 0.0, 0.0);
        ctrl.begin(ManipulationKind::Move, center);
        assert!(ctrl.is_active());

        // First digit: bootstrap numeric state.
        let mut frame = ActionFrame::default();
        frame.typed_chars.push('2');
        let mut ctx = idle_ctx();
        ctx.dragging = false; // not a mouse drag
        let result = ctrl.update(&frame, ctx);
        // Should get an Update with a zero translation (numeric override pending parse).
        assert!(matches!(result, ManipResult::Update(_)));
        let state = ctrl.state().unwrap();
        assert!(
            state.numeric_display.is_some(),
            "numeric display should be set after first digit"
        );
    }

    #[test]
    fn numeric_backspace_removes_last_digit() {
        let mut ctrl = ManipulationController::new();
        ctrl.begin(ManipulationKind::Move, glam::Vec3::ZERO);

        // Type "25".
        let mut frame = ActionFrame::default();
        frame.typed_chars.extend(['2', '5']);
        ctrl.update(&frame, idle_ctx());

        // Backspace once.
        let mut frame2 = ActionFrame::default();
        frame2.actions.insert(
            viewport_lib_types::input::Action::NumericBackspace,
            viewport_lib_types::input::ResolvedActionState::Pressed,
        );
        ctrl.update(&frame2, idle_ctx());

        let state = ctrl.state().unwrap();
        // Should now show "2" only.
        let display = state.numeric_display.unwrap();
        assert!(
            display.contains('2'),
            "display should contain '2': {display}"
        );
        assert!(
            !display.contains('5'),
            "display should not contain '5' after backspace: {display}"
        );
    }

    // -----------------------------------------------------------------------
    // angular_rotation_from_cursor sign tests
    // -----------------------------------------------------------------------

    fn make_view_proj_looking_neg_z() -> (glam::Mat4, glam::Mat4) {
        // Camera at (0, 0, 5) looking at origin.
        let view = glam::Mat4::look_at_rh(
            glam::Vec3::new(0.0, 0.0, 5.0),
            glam::Vec3::ZERO,
            glam::Vec3::Y,
        );
        let proj =
            glam::Mat4::perspective_rh(std::f32::consts::FRAC_PI_4, 800.0 / 600.0, 0.1, 100.0);
        (view, proj * view)
    }

    #[test]
    fn angular_rotation_z_toward_camera_cw_is_positive() {
        // Axis = +Z, camera at +Z => axis points toward camera (axis_z_cam > 0).
        // CW screen motion (cursor sweeps CW) should produce positive world angle.
        let (camera_view, view_proj) = make_view_proj_looking_neg_z();
        let gizmo_center = glam::Vec3::ZERO;
        let viewport_size = glam::Vec2::new(800.0, 600.0);

        // Place cursor to the right of center, move it upward (CW sweep).
        let cursor = glam::Vec2::new(500.0, 300.0); // right of screen center
        let pointer_delta = glam::Vec2::new(0.0, -20.0); // upward = CW for right-side cursor

        let angle = solvers::angular_rotation_from_cursor(
            Some(cursor),
            pointer_delta,
            gizmo_center,
            glam::Vec3::Z,
            view_proj,
            viewport_size,
            camera_view,
        );
        assert!(
            angle > 0.0,
            "CW motion with +Z axis (toward camera) should give positive angle, got {angle}"
        );
    }

    #[test]
    fn angular_rotation_neg_z_away_from_camera_cw_is_negative() {
        // Axis = -Z points away from camera.  Same CW cursor motion should give negative angle.
        let (camera_view, view_proj) = make_view_proj_looking_neg_z();
        let gizmo_center = glam::Vec3::ZERO;
        let viewport_size = glam::Vec2::new(800.0, 600.0);

        let cursor = glam::Vec2::new(500.0, 300.0);
        let pointer_delta = glam::Vec2::new(0.0, -20.0);

        let angle = solvers::angular_rotation_from_cursor(
            Some(cursor),
            pointer_delta,
            gizmo_center,
            glam::Vec3::NEG_Z,
            view_proj,
            viewport_size,
            camera_view,
        );
        assert!(
            angle < 0.0,
            "CW motion with -Z axis (away from camera) should give negative angle, got {angle}"
        );
    }

    // -----------------------------------------------------------------------
    // Controller lifecycle tests
    // -----------------------------------------------------------------------

    #[test]
    fn controller_lifecycle_begin_reset() {
        let mut ctrl = ManipulationController::new();
        assert!(!ctrl.is_active());

        ctrl.begin(ManipulationKind::Move, glam::Vec3::ZERO);
        assert!(ctrl.is_active());

        ctrl.reset();
        assert!(!ctrl.is_active());
    }

    #[test]
    fn controller_begin_no_op_when_active() {
        let mut ctrl = ManipulationController::new();
        ctrl.begin(ManipulationKind::Move, glam::Vec3::ONE);
        ctrl.begin(ManipulationKind::Rotate, glam::Vec3::ZERO);
        // Should still be Move (second begin was no-op).
        let state = ctrl.state().unwrap();
        assert_eq!(state.kind, ManipulationKind::Move);
    }

    #[test]
    fn controller_idle_returns_none() {
        let mut ctrl = ManipulationController::new();
        let frame = ActionFrame::default();
        let result = ctrl.update(&frame, idle_ctx());
        assert_eq!(result, ManipResult::None);
        assert!(!ctrl.is_active());
    }

    #[test]
    fn controller_no_session_without_selection_center() {
        let mut ctrl = ManipulationController::new();
        // No selection_center -> G/R/S should not start a session.
        let mut frame = ActionFrame::default();
        frame.actions.insert(
            viewport_lib_types::input::Action::BeginMove,
            viewport_lib_types::input::ResolvedActionState::Pressed,
        );
        let mut ctx = idle_ctx();
        ctx.selection_center = None;
        let result = ctrl.update(&frame, ctx);
        assert_eq!(result, ManipResult::None);
        assert!(!ctrl.is_active());
    }

    #[test]
    fn controller_g_key_starts_move_session() {
        let mut ctrl = ManipulationController::new();
        let mut frame = ActionFrame::default();
        frame.actions.insert(
            viewport_lib_types::input::Action::BeginMove,
            viewport_lib_types::input::ResolvedActionState::Pressed,
        );
        let mut ctx = idle_ctx();
        ctx.selection_center = Some(glam::Vec3::new(1.0, 2.0, 3.0));

        let result = ctrl.update(&frame, ctx);
        assert_eq!(result, ManipResult::None); // None on first frame
        assert!(ctrl.is_active());
        assert_eq!(ctrl.state().unwrap().kind, ManipulationKind::Move);
    }

    // -----------------------------------------------------------------------
    // Snapping
    // -----------------------------------------------------------------------

    /// Drive a move via `pointer_delta` (no cursor anchor), returning the emitted
    /// translation from the single resulting `Update`.
    fn drive_move(snap: SnapConfig, pointer_delta: glam::Vec2) -> glam::Vec3 {
        let mut ctrl = ManipulationController::new().with_snap(snap);
        ctrl.begin(ManipulationKind::Move, glam::Vec3::ZERO);
        let mut ctx = idle_ctx();
        ctx.pointer_delta = pointer_delta; // cursor_viewport stays None -> delta path
        match ctrl.update(&ActionFrame::default(), ctx) {
            ManipResult::Update(delta) => delta.translation,
            other => panic!("expected Update, got {other:?}"),
        }
    }

    #[test]
    fn move_snap_rounds_translation_to_increment() {
        let inc = 0.5;
        let snap = SnapConfig {
            translation: Some(inc),
            ..Default::default()
        };
        // A large drag so the snapped result lands on a non-zero grid stop.
        let t = drive_move(snap, glam::Vec2::new(4000.0, -2500.0));
        for c in [t.x, t.y, t.z] {
            let steps = c / inc;
            assert!(
                (steps - steps.round()).abs() < 1e-4,
                "component {c} is not a multiple of {inc}"
            );
        }
    }

    #[test]
    fn move_without_snap_matches_raw_solver() {
        // With snapping off, the emitted delta is exactly the solver output : the
        // existing behaviour is preserved.
        let pd = glam::Vec2::new(50.0, 0.0);
        let emitted = drive_move(SnapConfig::default(), pd);
        let raw = solvers::constrained_translation(
            pd,
            None,
            false,
            glam::Vec3::ZERO,
            &make_camera(),
            glam::Vec2::new(800.0, 600.0),
        );
        assert!(
            (emitted - raw).length() < 1e-4,
            "unsnapped emit {emitted:?} should equal raw solver {raw:?}"
        );
    }

    #[test]
    fn move_emits_cumulative_total_across_frames() {
        // The emitted translation is the total since the drag began, not a
        // per-frame increment. Two identical frames therefore emit about twice
        // the first frame.
        let mut ctrl = ManipulationController::new();
        ctrl.begin(ManipulationKind::Move, glam::Vec3::ZERO);
        let pd = glam::Vec2::new(50.0, 0.0);
        let mut emitted = [glam::Vec3::ZERO; 2];
        for out in emitted.iter_mut() {
            let mut ctx = idle_ctx();
            ctx.pointer_delta = pd;
            match ctrl.update(&ActionFrame::default(), ctx) {
                ManipResult::Update(delta) => *out = delta.translation,
                other => panic!("expected Update, got {other:?}"),
            }
        }
        assert!(
            (emitted[1] - emitted[0] * 2.0).length() < 1e-4,
            "frame 2 {:?} should be twice frame 1 {:?}",
            emitted[1],
            emitted[0]
        );
        assert!(
            emitted[0].length() > 1e-4,
            "the drag should produce movement"
        );
    }

    #[test]
    fn sub_increment_move_snaps_to_no_movement() {
        // A drag smaller than half an increment rounds to the nearest stop (no
        // movement) when snapping, but produces a real delta without it.
        let pd = glam::Vec2::new(3.0, 0.0);
        let snapped = drive_move(
            SnapConfig {
                translation: Some(1.0),
                ..Default::default()
            },
            pd,
        );
        let free = drive_move(SnapConfig::default(), pd);
        assert!(
            snapped.length() < 1e-6,
            "sub-increment drag should snap to no movement, got {snapped:?}"
        );
        assert!(
            free.length() > 1e-6,
            "same drag without snap should move, got {free:?}"
        );
    }

    #[test]
    fn sub_increment_scale_snaps_to_identity() {
        // The scale path snaps the cumulative factor the same way: a tiny drag
        // rounds back to 1.0 (no scale) when snapping, but scales without it.
        fn drive_scale(snap: SnapConfig) -> glam::Vec3 {
            let mut ctrl = ManipulationController::new().with_snap(snap);
            ctrl.begin(ManipulationKind::Scale, glam::Vec3::ZERO);
            let mut ctx = idle_ctx();
            ctx.pointer_delta = glam::Vec2::new(2.0, 0.0);
            match ctrl.update(&ActionFrame::default(), ctx) {
                ManipResult::Update(delta) => delta.scale,
                other => panic!("expected Update, got {other:?}"),
            }
        }
        let snapped = drive_scale(SnapConfig {
            scale: Some(0.5),
            ..Default::default()
        });
        let free = drive_scale(SnapConfig::default());
        assert!(
            (snapped - glam::Vec3::ONE).length() < 1e-4,
            "sub-increment scale should snap to identity, got {snapped:?}"
        );
        assert!(
            (free - glam::Vec3::ONE).length() > 1e-6,
            "same scale drag without snap should change scale, got {free:?}"
        );
    }

    // -----------------------------------------------------------------------
    // Handle constraint mapping
    // -----------------------------------------------------------------------

    #[test]
    fn hit_to_constraint_single_axes_pass_through() {
        assert_eq!(hit_to_constraint(GizmoAxis::X), (Some(GizmoAxis::X), false));
        assert_eq!(hit_to_constraint(GizmoAxis::Y), (Some(GizmoAxis::Y), false));
        assert_eq!(hit_to_constraint(GizmoAxis::Z), (Some(GizmoAxis::Z), false));
    }

    #[test]
    fn hit_to_constraint_plane_handles_exclude_third_axis() {
        // A plane handle drags in that plane, i.e. excludes the perpendicular axis.
        assert_eq!(hit_to_constraint(GizmoAxis::XY), (Some(GizmoAxis::Z), true));
        assert_eq!(hit_to_constraint(GizmoAxis::XZ), (Some(GizmoAxis::Y), true));
        assert_eq!(hit_to_constraint(GizmoAxis::YZ), (Some(GizmoAxis::X), true));
    }

    #[test]
    fn hit_to_constraint_screen_is_unconstrained() {
        assert_eq!(hit_to_constraint(GizmoAxis::Screen), (None, false));
    }

    // -----------------------------------------------------------------------
    // Numeric rotation
    // -----------------------------------------------------------------------

    #[test]
    fn numeric_rotate_emits_rotation_override() {
        // Typing a number during a rotate session fills the per-axis override the
        // app applies as an angle : the same path Move/Scale already had.
        let mut ctrl = ManipulationController::new();
        ctrl.begin(ManipulationKind::Rotate, glam::Vec3::ZERO);
        let mut frame = ActionFrame::default();
        frame.typed_chars.extend(['4', '5']);
        match ctrl.update(&frame, idle_ctx()) {
            ManipResult::Update(delta) => {
                assert_eq!(delta.rotation_override[0], Some(45.0));
                assert_eq!(delta.rotation_override[1], None);
                assert_eq!(delta.rotation_override[2], None);
            }
            other => panic!("expected Update, got {other:?}"),
        }
    }
}
