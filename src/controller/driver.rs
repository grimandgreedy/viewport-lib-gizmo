//! Per-frame driver over [`ManipulationController`].
//!
//! [`ManipulationController::update`] takes a fully-assembled
//! [`ManipulationContext`]. `GizmoDriver` builds that context each frame so a
//! consumer does not have to: it owns the [`Gizmo`] state, sizes the gizmo for the
//! screen, hit-tests the cursor to set the hovered axis, and calls the controller.
//!
//! It does not touch a scene or a renderer. It needs only a camera and a cursor,
//! plus the world-space centre of the current selection (which the consumer
//! computes from its own scene). It returns the [`ManipResult`]; the consumer
//! applies the resulting [`TransformDelta`](super::types::TransformDelta) to its
//! own objects.
//!
//! Resolving raw events into the [`ActionFrame`] is a separate job: use
//! `viewport-lib-input` or your own resolver, then pass the frame in here.

use glam::{Vec2, Vec3};
use viewport_lib_types::camera::Camera;
use viewport_lib_types::input::ActionFrame;

use super::ManipulationController;
use super::types::{
    GizmoInfo, ManipResult, ManipulationContext, ManipulationKind, ManipulationState,
};
use crate::maths::gizmo::{self, Gizmo, GizmoAxis, GizmoMode, GizmoSpace};
use crate::maths::snap::SnapConfig;

/// The per-frame inputs the driver needs.
///
/// `selection_center` is the world-space gizmo pivot, which the consumer computes
/// from its selection (for example with
/// [`gizmo_center_for_pivot`](crate::maths::gizmo::gizmo_center_for_pivot)). The
/// drag flags come from the consumer's pointer handling: `drag_started` on the
/// frame a drag begins, `dragging` while it is held, `clicked` on a completed
/// click.
pub struct FrameInput<'a> {
    /// The resolved input for this frame.
    pub action_frame: &'a ActionFrame,
    /// The viewport camera.
    pub camera: &'a Camera,
    /// Viewport size in logical pixels.
    pub viewport_size: Vec2,
    /// Cursor position in viewport pixels (top-left origin), or `None` if outside.
    pub cursor_viewport: Option<Vec2>,
    /// Pointer movement since the last frame, in pixels.
    pub pointer_delta: Vec2,
    /// World-space centre of the selection. `None` hides the gizmo.
    pub selection_center: Option<Vec3>,
    /// True on the frame a drag begins.
    pub drag_started: bool,
    /// True while a drag is held.
    pub dragging: bool,
    /// True on a completed click.
    pub clicked: bool,
}

/// Drives a [`ManipulationController`] from a camera and cursor each frame.
pub struct GizmoDriver {
    controller: ManipulationController,
    gizmo: Gizmo,
    gizmo_center: Option<Vec3>,
    gizmo_scale: f32,
}

impl Default for GizmoDriver {
    fn default() -> Self {
        Self::new()
    }
}

impl GizmoDriver {
    /// Create a driver with no active session and no snapping.
    pub fn new() -> Self {
        Self {
            controller: ManipulationController::new(),
            gizmo: Gizmo::new(),
            gizmo_center: None,
            gizmo_scale: 1.0,
        }
    }

    /// Set the snap increments used while dragging. See
    /// [`ManipulationController::set_snap`].
    pub fn set_snap(&mut self, snap: SnapConfig) {
        self.controller.set_snap(snap);
    }

    /// Builder form of [`set_snap`](Self::set_snap).
    pub fn with_snap(mut self, snap: SnapConfig) -> Self {
        self.controller.set_snap(snap);
        self
    }

    /// The current gizmo mode (translate / rotate / scale).
    pub fn mode(&self) -> GizmoMode {
        self.gizmo.mode
    }

    /// Set the gizmo mode. Ignored while a session is active.
    pub fn set_mode(&mut self, mode: GizmoMode) {
        if !self.controller.is_active() {
            self.gizmo.mode = mode;
        }
    }

    /// Cycle the gizmo mode: translate -> rotate -> scale -> translate. Ignored
    /// while a session is active.
    pub fn cycle_mode(&mut self) {
        let next = match self.gizmo.mode {
            GizmoMode::Translate => GizmoMode::Rotate,
            GizmoMode::Rotate => GizmoMode::Scale,
            _ => GizmoMode::Translate,
        };
        self.set_mode(next);
    }

    /// The current gizmo space (world / local).
    pub fn space(&self) -> GizmoSpace {
        self.gizmo.space
    }

    /// Set the gizmo space.
    pub fn set_space(&mut self, space: GizmoSpace) {
        self.gizmo.space = space;
    }

    /// True while a G/R/S session or a gizmo drag is in progress.
    pub fn is_active(&self) -> bool {
        self.controller.is_active()
    }

    /// The axis currently under the cursor.
    pub fn hovered_axis(&self) -> GizmoAxis {
        self.gizmo.hovered_axis
    }

    /// The gizmo pivot resolved on the last [`update`](Self::update).
    pub fn gizmo_center(&self) -> Option<Vec3> {
        self.gizmo_center
    }

    /// The screen-constant world scale resolved on the last [`update`](Self::update).
    pub fn gizmo_scale(&self) -> f32 {
        self.gizmo_scale
    }

    /// The gizmo state, for feeding
    /// [`build_gizmo_overlays`](crate::overlay::builder::build_gizmo_overlays).
    pub fn gizmo(&self) -> &Gizmo {
        &self.gizmo
    }

    /// The controller's manipulation state, or `None` when idle. Carries the
    /// active kind, axis constraint, and numeric-input display string.
    pub fn state(&self) -> Option<ManipulationState> {
        self.controller.state()
    }

    /// The numeric-input string being typed this session (for example
    /// `"X: 22  Y: 2"`), or `None` when there is no numeric input. This is the
    /// raw values only; see [`readout`](Self::readout) for the mode-prefixed form.
    pub fn numeric_display(&self) -> Option<String> {
        self.controller.state().and_then(|s| s.numeric_display)
    }

    /// The readout to show while a session is active: the mode name, plus the
    /// typed values once the user starts entering them. For example `"Scale:"`
    /// on entering scale mode, then `"Scale:  X: 22  Y: 2"` while typing. `None`
    /// when no session is active. Feed it to
    /// [`build_numeric_label`](crate::overlay::builder::build_numeric_label) or
    /// render it yourself.
    pub fn readout(&self) -> Option<String> {
        let state = self.controller.state()?;
        let mode = match state.kind {
            ManipulationKind::Move => "Translate",
            ManipulationKind::Rotate => "Rotate",
            ManipulationKind::Scale => "Scale",
        };
        Some(match state.numeric_display {
            Some(values) => format!("{mode}:  {values}"),
            None => format!("{mode}:"),
        })
    }

    /// A [`GizmoInfo`] snapshot of the current centre, scale, and mode.
    pub fn gizmo_info(&self) -> Option<GizmoInfo> {
        self.gizmo_center.map(|center| GizmoInfo {
            center,
            scale: self.gizmo_scale,
            orientation: glam::Quat::IDENTITY,
            mode: self.gizmo.mode,
        })
    }

    /// Drive the gizmo for one frame: size it, update the hovered axis, assemble
    /// the [`ManipulationContext`], and run the controller. Returns the
    /// [`ManipResult`]; apply any [`Update`](ManipResult::Update) delta to your
    /// own objects.
    pub fn update(&mut self, input: FrameInput) -> ManipResult {
        self.gizmo_center = input.selection_center;

        if let Some(center) = self.gizmo_center {
            self.gizmo_scale = gizmo::compute_gizmo_scale(
                center,
                input.camera.eye_position(),
                input.camera.fov_y,
                input.viewport_size.y,
            );
        }

        // Update the hovered axis only when no session is active; during a drag
        // the active axis is fixed.
        if !self.controller.is_active() {
            if let (Some(center), Some(cursor)) = (self.gizmo_center, input.cursor_viewport) {
                let (ray_origin, ray_dir) =
                    gizmo::cursor_ray(input.camera, cursor, input.viewport_size);
                self.gizmo.hovered_axis = self.gizmo.hit_test_oriented(
                    ray_origin,
                    ray_dir,
                    center,
                    self.gizmo_scale,
                    glam::Quat::IDENTITY,
                );
            } else {
                self.gizmo.hovered_axis = GizmoAxis::None;
            }
        }

        let ctx = ManipulationContext {
            camera: input.camera.clone(),
            viewport_size: input.viewport_size,
            cursor_viewport: input.cursor_viewport,
            pointer_delta: input.pointer_delta,
            selection_center: self.gizmo_center,
            gizmo: self.gizmo_info(),
            drag_started: input.drag_started,
            dragging: input.dragging,
            clicked: input.clicked,
        };

        let result = self.controller.update(input.action_frame, ctx);

        // While a session is active, draw the handles for what is being done: a
        // keyboard G/R/S session (or a gizmo drag) sets the mode the overlay uses.
        if let Some(state) = self.controller.state() {
            self.gizmo.mode = match state.kind {
                ManipulationKind::Move => GizmoMode::Translate,
                ManipulationKind::Rotate => GizmoMode::Rotate,
                ManipulationKind::Scale => GizmoMode::Scale,
            };
        }

        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn camera() -> Camera {
        let mut c = Camera::default();
        c.center = Vec3::ZERO;
        c.distance = 6.0;
        c
    }

    /// Project a world point to a viewport pixel, matching the overlay builder.
    fn to_px(camera: &Camera, world: Vec3, vp: Vec2) -> Vec2 {
        let clip = (camera.proj_matrix() * camera.view_matrix()) * world.extend(1.0);
        let ndc = clip.truncate() / clip.w;
        Vec2::new((ndc.x * 0.5 + 0.5) * vp.x, (1.0 - (ndc.y * 0.5 + 0.5)) * vp.y)
    }

    fn frame<'a>(
        af: &'a ActionFrame,
        camera: &'a Camera,
        vp: Vec2,
        cursor: Option<Vec2>,
        center: Option<Vec3>,
    ) -> FrameInput<'a> {
        FrameInput {
            action_frame: af,
            camera,
            viewport_size: vp,
            cursor_viewport: cursor,
            pointer_delta: Vec2::ZERO,
            selection_center: center,
            drag_started: false,
            dragging: false,
            clicked: false,
        }
    }

    #[test]
    fn no_selection_is_idle() {
        let mut d = GizmoDriver::new();
        let af = ActionFrame::default();
        let cam = camera();
        let r = d.update(frame(&af, &cam, Vec2::new(1280.0, 720.0), None, None));
        assert!(matches!(r, ManipResult::None));
        assert_eq!(d.hovered_axis(), GizmoAxis::None);
        assert!(!d.is_active());
    }

    #[test]
    fn resolves_scale_and_centre_from_selection() {
        let mut d = GizmoDriver::new();
        let af = ActionFrame::default();
        let cam = camera();
        let vp = Vec2::new(1280.0, 720.0);
        d.update(frame(&af, &cam, vp, None, Some(Vec3::ZERO)));
        assert_eq!(d.gizmo_center(), Some(Vec3::ZERO));
        assert!(d.gizmo_scale() > 0.0, "scale should be sized for the screen");
    }

    #[test]
    fn cursor_over_the_x_arm_hovers_x() {
        let mut d = GizmoDriver::new();
        let af = ActionFrame::default();
        let cam = camera();
        let vp = Vec2::new(1280.0, 720.0);
        // Size the gizmo first so we know where the X arm projects to.
        d.update(frame(&af, &cam, vp, None, Some(Vec3::ZERO)));
        let x_handle = Vec3::X * (d.gizmo_scale() * 0.5);
        let cursor = to_px(&cam, x_handle, vp);
        d.update(frame(&af, &cam, vp, Some(cursor), Some(Vec3::ZERO)));
        assert_eq!(d.hovered_axis(), GizmoAxis::X);
    }

    #[test]
    fn cursor_off_the_gizmo_hovers_nothing() {
        let mut d = GizmoDriver::new();
        let af = ActionFrame::default();
        let cam = camera();
        let vp = Vec2::new(1280.0, 720.0);
        d.update(frame(&af, &cam, vp, Some(Vec2::new(5.0, 5.0)), Some(Vec3::ZERO)));
        assert_eq!(d.hovered_axis(), GizmoAxis::None);
    }

    #[test]
    fn set_mode_switches_the_drawn_handles() {
        let mut d = GizmoDriver::new();
        assert_eq!(d.mode(), GizmoMode::Translate);
        d.set_mode(GizmoMode::Rotate);
        assert_eq!(d.mode(), GizmoMode::Rotate);
    }

    #[test]
    fn cycle_mode_advances_through_the_three_modes() {
        let mut d = GizmoDriver::new();
        assert_eq!(d.mode(), GizmoMode::Translate);
        d.cycle_mode();
        assert_eq!(d.mode(), GizmoMode::Rotate);
        d.cycle_mode();
        assert_eq!(d.mode(), GizmoMode::Scale);
        d.cycle_mode();
        assert_eq!(d.mode(), GizmoMode::Translate);
    }
}
