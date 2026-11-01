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

use glam::{Quat, Vec2, Vec3};
use viewport_lib_types::camera::Camera;
use viewport_lib_types::input::ActionFrame;

use super::ManipulationController;
use super::types::{
    GizmoInfo, ManipResult, ManipulationContext, ManipulationKind, ManipulationState, ValueMode,
};
use crate::maths::gizmo::{self, Gizmo, GizmoAxis, GizmoMode, GizmoSpace};
use crate::maths::snap::SnapConfig;
use crate::overlay::builder::build_gizmo_overlays;
use crate::overlay::style::GizmoStyle;
use viewport_lib_types::overlay::{OverlayPolylineItem, OverlayShapeItem};

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
    /// World-space centre of the selection. `None` hides the gizmo, and ends a
    /// session in progress with [`ManipResult::Cancel`].
    pub selection_center: Option<Vec3>,
    /// The selection's orientation (for one object, its rotation). In local
    /// space the handles are drawn, picked and dragged along its axes; world
    /// space ignores it. Pass `Quat::IDENTITY` if you only use world space.
    pub selection_orientation: Quat,
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
    selection_orientation: Quat,
    value_mode: ValueMode,
    /// The mode set before a keyboard session switched the handles to its own
    /// kind, put back when the session ends.
    mode_before_session: Option<GizmoMode>,
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
            selection_orientation: Quat::IDENTITY,
            value_mode: ValueMode::Relative,
            mode_before_session: None,
        }
    }

    /// Whether values are read relative to the start pose or as absolute
    /// (objective) world values. The gizmo tracks and displays this; the app
    /// applies it.
    pub fn value_mode(&self) -> ValueMode {
        self.value_mode
    }

    /// Set the [`ValueMode`].
    pub fn set_value_mode(&mut self, mode: ValueMode) {
        self.value_mode = mode;
    }

    /// Flip between relative and objective.
    pub fn toggle_value_mode(&mut self) {
        self.value_mode = self.value_mode.toggled();
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

    /// The current gizmo mode (translate / rotate / scale). While a G / R / S
    /// session runs this is the session's kind; the mode set before it comes
    /// back when the session ends.
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

    /// Set the gizmo space. A session in progress keeps the axes it began
    /// with.
    pub fn set_space(&mut self, space: GizmoSpace) {
        self.gizmo.space = space;
    }

    /// The orientation the handles are drawn and picked in: identity in world
    /// space, the last [`FrameInput::selection_orientation`] in local space.
    pub fn space_orientation(&self) -> Quat {
        match self.gizmo.space {
            GizmoSpace::Local => self.selection_orientation,
            _ => Quat::IDENTITY,
        }
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

    /// The readout to show while a session is active: the mode, the constraint
    /// axis, and the value mode, plus the typed values once the user starts
    /// entering them. For example `"Translate X [rel]"` while dragging the X arm,
    /// or `"Scale XY [rel]:  *X: 22  Y: 2"` while typing (the `*` marks the field
    /// taking input). `None` when no session is active. Feed it to
    /// [`build_numeric_label`](crate::overlay::builder::build_numeric_label) or
    /// render it yourself.
    pub fn readout(&self) -> Option<String> {
        let state = self.controller.state()?;
        let mode = match state.kind {
            ManipulationKind::Move => "Translate",
            ManipulationKind::Rotate => "Rotate",
            ManipulationKind::Scale => "Scale",
        };
        let mut header = mode.to_string();
        let axis = constraint_label(state.axis, state.exclude_axis);
        if !axis.is_empty() {
            header.push(' ');
            header.push_str(axis);
        }
        header.push_str(&format!(" [{}]", self.value_mode.tag()));
        Some(match state.numeric_display {
            Some(values) => format!("{header}:  {values}"),
            None => header,
        })
    }

    /// A [`GizmoInfo`] snapshot of the current centre, scale, orientation (see
    /// [`space_orientation`](Self::space_orientation)), and mode.
    pub fn gizmo_info(&self) -> Option<GizmoInfo> {
        self.gizmo_center.map(|center| GizmoInfo {
            center,
            scale: self.gizmo_scale,
            orientation: self.space_orientation(),
            mode: self.gizmo.mode,
        })
    }

    /// The axis to brighten in one view. While a session runs, every view shows
    /// the axis or plane being changed; otherwise only the view that drives the
    /// driver (`driving_view`) shows the hovered axis, since the cursor is over
    /// that view alone.
    pub fn highlight(&self, driving_view: bool) -> GizmoAxis {
        if let Some(state) = self.controller.state() {
            return match (state.axis, state.exclude_axis) {
                (Some(GizmoAxis::X), true) => GizmoAxis::YZ,
                (Some(GizmoAxis::Y), true) => GizmoAxis::XZ,
                (Some(GizmoAxis::Z), true) => GizmoAxis::XY,
                (Some(axis), _) => axis,
                (None, _) => GizmoAxis::None,
            };
        }
        if driving_view {
            self.gizmo.hovered_axis
        } else {
            GizmoAxis::None
        }
    }

    /// Build the gizmo's overlay items for one view, sized for that view's own
    /// camera and height. With several views of one scene, drive the driver from
    /// one of them and call this for each, passing `driving_view = true` for the
    /// one that drove it. Builds nothing when there is no selection.
    pub fn build_view_overlays(
        &self,
        style: &GizmoStyle,
        camera: &Camera,
        viewport_size: Vec2,
        driving_view: bool,
        shapes: &mut Vec<OverlayShapeItem>,
        polylines: &mut Vec<OverlayPolylineItem>,
    ) {
        let Some(centre) = self.gizmo_center else {
            return;
        };
        let scale = gizmo::gizmo_scale_for_camera(centre, camera, viewport_size.y);
        let forward = camera.forward();
        build_gizmo_overlays(
            style,
            self.gizmo.mode,
            camera.proj_matrix() * camera.view_matrix(),
            forward,
            [viewport_size.x, viewport_size.y],
            centre,
            scale,
            self.highlight(driving_view),
            self.space_orientation(),
            shapes,
            polylines,
        );
    }

    /// Drive the gizmo for one frame: size it, update the hovered axis, assemble
    /// the [`ManipulationContext`], and run the controller. Returns the
    /// [`ManipResult`]; apply any [`Update`](ManipResult::Update) delta to your
    /// own objects.
    pub fn update(&mut self, input: FrameInput) -> ManipResult {
        self.gizmo_center = input.selection_center;
        self.selection_orientation = input.selection_orientation;

        if let Some(center) = self.gizmo_center {
            self.gizmo_scale =
                gizmo::gizmo_scale_for_camera(center, input.camera, input.viewport_size.y);
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
                    self.selection_orientation,
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
        // keyboard G/R/S session sets the mode the overlay uses, and the mode set
        // before it comes back once the session ends.
        if let Some(state) = self.controller.state() {
            let kind_mode = match state.kind {
                ManipulationKind::Move => GizmoMode::Translate,
                ManipulationKind::Rotate => GizmoMode::Rotate,
                ManipulationKind::Scale => GizmoMode::Scale,
            };
            self.mode_before_session.get_or_insert(self.gizmo.mode);
            self.gizmo.mode = kind_mode;
        } else if let Some(mode) = self.mode_before_session.take() {
            self.gizmo.mode = mode;
        }

        result
    }
}

/// A short label for the active constraint: a single axis (`"X"`), the plane of
/// the two non-excluded axes (`"XY"`), or `""` for a free (unconstrained) drag.
fn constraint_label(axis: Option<GizmoAxis>, exclude: bool) -> &'static str {
    match (axis, exclude) {
        (Some(GizmoAxis::X), false) => "X",
        (Some(GizmoAxis::Y), false) => "Y",
        (Some(GizmoAxis::Z), false) => "Z",
        (Some(GizmoAxis::X), true) => "YZ",
        (Some(GizmoAxis::Y), true) => "XZ",
        (Some(GizmoAxis::Z), true) => "XY",
        _ => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn camera() -> Camera {
        let mut c = Camera::default();
        c.set_orbit(glam::DVec3::ZERO, 6.0, c.orientation());
        c
    }

    /// Project a world point to a viewport pixel, matching the overlay builder.
    fn to_px(camera: &Camera, world: Vec3, vp: Vec2) -> Vec2 {
        let clip = (camera.proj_matrix() * camera.view_matrix()) * world.extend(1.0);
        let ndc = clip.truncate() / clip.w;
        Vec2::new(
            (ndc.x * 0.5 + 0.5) * vp.x,
            (1.0 - (ndc.y * 0.5 + 0.5)) * vp.y,
        )
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
            selection_orientation: Quat::IDENTITY,
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
        assert!(
            d.gizmo_scale() > 0.0,
            "scale should be sized for the screen"
        );
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
        d.update(frame(
            &af,
            &cam,
            vp,
            Some(Vec2::new(5.0, 5.0)),
            Some(Vec3::ZERO),
        ));
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
