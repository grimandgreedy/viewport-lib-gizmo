//! A viewport-lib-ui `Custom` widget that draws and drives the gizmo.
//!
//! [`GizmoTool`] runs the existing [`GizmoDriver`] behind the toolkit's custom
//! widget path. Each frame the host sets the camera and the selection pivot with
//! [`set_frame`](GizmoTool::set_frame); the widget then paints the projected
//! handles through the draw path, hit-tests and drags itself from the toolkit's
//! pointer input, captures the pointer only while a handle is hovered or a drag is
//! live, and reports the transform as a typed [`GizmoResult`] the host reads back.
//!
//! The 3D maths, the camera, and the controller stay in the gizmo core; this type
//! only bridges them to the toolkit. It holds its state behind interior
//! mutability, since the registry holds the handler shared. Register one `Arc`,
//! keep a clone, and call [`set_frame`](GizmoTool::set_frame) on it each frame.
//!
//! # Placing the pane
//!
//! Give the custom node a rect that covers the viewport region the gizmo lives
//! over, so the pointer reaches the widget wherever a handle can project. Place it
//! before the panels in the tree (on the default `Content` layer) so a panel over
//! the viewport still wins the pointer. The widget captures only over a handle or
//! mid-drag, so empty space falls through to the camera.
//!
//! Because the pane covers the viewport, chrome can end up drawn over a handle. The
//! widget acts on the pointer only while the toolkit reports the pane as the widget
//! under it, so a control laid over a handle takes the press rather than the gizmo.
//! A live drag or keyboard session keeps the pointer wherever it travels.

use std::cell::{Cell, RefCell};

use glam::{Quat, Vec2, Vec3};
use serde::{Deserialize, Serialize};
use viewport_lib_types::camera::Camera;
use viewport_lib_types::input::ActionFrame;

use viewport_lib_ui_core::serde_json;
use viewport_lib_ui_core::{
    Cursor, CustomCtx, CustomInput, CustomWidget, DrawList, Input, Rect, Style, TextMeasure,
};

use crate::controller::driver::FrameInput;
use crate::controller::{GizmoDriver, ManipResult, TransformDelta, ValueMode};
use crate::maths::gizmo::{self, GizmoAxis, GizmoMode, GizmoSpace};
use crate::overlay::builder::build_gizmo_overlays;
use crate::overlay::style::GizmoStyle;


/// The default `kind` string to register [`GizmoTool`] under.
pub const GIZMO_KIND: &str = "gizmo";

/// A transform delta as plain arrays, so it serialises without a glam serde
/// dependency. `rotation` is a quaternion in `[x, y, z, w]` order.
///
/// A typed number replaces the dragged value on the axis it was typed for. Those
/// arrive in the three `*_override` arrays, one slot per axis, `None` where nothing
/// was typed. Apply an override in place of the corresponding dragged component:
/// [`position`](Self::position) and [`scale_values`](Self::scale_values) do that
/// for the two that are plain vectors.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct GizmoDelta {
    /// Total translation since the drag began.
    pub translation: [f32; 3],
    /// Total rotation since the drag began, as a quaternion `[x, y, z, w]`.
    pub rotation: [f32; 4],
    /// Total scale since the drag began.
    pub scale: [f32; 3],
    /// A typed position per axis, replacing that axis of `translation`.
    #[serde(default)]
    pub position_override: [Option<f32>; 3],
    /// A typed angle per axis, in radians, replacing that axis of the rotation.
    #[serde(default)]
    pub rotation_override: [Option<f32>; 3],
    /// A typed scale per axis, replacing that axis of `scale`.
    #[serde(default)]
    pub scale_override: [Option<f32>; 3],
}

impl Default for GizmoDelta {
    fn default() -> Self {
        Self {
            translation: [0.0; 3],
            rotation: [0.0, 0.0, 0.0, 1.0],
            scale: [1.0; 3],
            position_override: [None; 3],
            rotation_override: [None; 3],
            scale_override: [None; 3],
        }
    }
}

impl GizmoDelta {
    /// The translation to apply, with any typed value standing in for the dragged
    /// one on its axis. In [`ValueMode::Objective`](crate::ValueMode::Objective) a
    /// typed value is an absolute world coordinate, so add the start position back
    /// yourself rather than using this.
    pub fn position(&self) -> [f32; 3] {
        let mut out = self.translation;
        for (i, typed) in self.position_override.iter().enumerate() {
            if let Some(value) = typed {
                out[i] = *value;
            }
        }
        out
    }

    /// The scale to apply, with any typed value standing in for the dragged one on
    /// its axis.
    pub fn scale_values(&self) -> [f32; 3] {
        let mut out = self.scale;
        for (i, typed) in self.scale_override.iter().enumerate() {
            if let Some(value) = typed {
                out[i] = *value;
            }
        }
        out
    }
}

impl From<TransformDelta> for GizmoDelta {
    fn from(delta: TransformDelta) -> Self {
        Self {
            translation: delta.translation.to_array(),
            rotation: delta.rotation.to_array(),
            scale: delta.scale.to_array(),
            position_override: delta.position_override,
            rotation_override: delta.rotation_override,
            scale_override: delta.scale_override,
        }
    }
}

/// What the tool reports for a frame, mirroring the controller's result. The host
/// reads this back from the custom-output event and acts on it: apply an `Update`
/// delta, finalise on `Commit`, restore on `Cancel`, or restore-but-keep-session
/// on `ConstraintChanged`.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub enum GizmoResult {
    /// A cumulative transform delta to apply to the selection this frame.
    Update(GizmoDelta),
    /// The drag or session finished; finalise.
    Commit,
    /// The session was cancelled; restore the start pose.
    Cancel,
    /// The axis constraint changed; restore to the start pose but keep dragging.
    ConstraintChanged,
}

/// When the readout panel is worth showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum ReadoutWhen {
    /// Always, so the field reads the object's values even at rest (the default).
    #[default]
    Always,
    /// Only while a drag or a keyboard session is running, so the panel is out of the
    /// way until there is something being changed.
    WhileActive,
}

/// The camera, selection, and resolved input the host supplies for a frame.
struct FrameState {
    camera: Camera,
    selection_center: Option<Vec3>,
    space_orientation: Quat,
    action_frame: ActionFrame,
}

impl Default for FrameState {
    fn default() -> Self {
        Self {
            camera: Camera::default(),
            selection_center: None,
            space_orientation: Quat::IDENTITY,
            action_frame: ActionFrame::default(),
        }
    }
}

/// The gizmo as a viewport-lib-ui custom widget. Construct one, register an `Arc`
/// of it, and drive it with [`set_frame`](Self::set_frame) each frame.
pub struct GizmoTool {
    driver: RefCell<GizmoDriver>,
    frame: RefCell<FrameState>,
    style: RefCell<GizmoStyle>,
    last_cursor: Cell<Option<Vec2>>,
    show_readout: Cell<bool>,
    readout_when: Cell<ReadoutWhen>,
}

impl Default for GizmoTool {
    fn default() -> Self {
        Self::new()
    }
}

impl GizmoTool {
    /// A tool with the default style and no active session.
    pub fn new() -> Self {
        Self::with_style(GizmoStyle::default())
    }

    /// A tool with a chosen style.
    pub fn with_style(style: GizmoStyle) -> Self {
        Self {
            driver: RefCell::new(GizmoDriver::new()),
            frame: RefCell::new(FrameState::default()),
            style: RefCell::new(style),
            last_cursor: Cell::new(None),
            show_readout: Cell::new(true),
            readout_when: Cell::new(ReadoutWhen::Always),
        }
    }

    /// Whether the numeric readout panel should be shown. Defaults to `true`. The
    /// host reads this to toggle the visibility of the panel built by
    /// [`readout_panel`](super::readout::readout_panel).
    pub fn show_readout(&self) -> bool {
        self.show_readout.get()
    }

    /// Set whether the readout panel is shown.
    pub fn set_show_readout(&self, show: bool) {
        self.show_readout.set(show);
    }

    /// When the readout panel is worth showing. Defaults to
    /// [`Always`](ReadoutWhen::Always).
    pub fn readout_when(&self) -> ReadoutWhen {
        self.readout_when.get()
    }

    /// Set when the readout panel is worth showing.
    pub fn set_readout_when(&self, when: ReadoutWhen) {
        self.readout_when.set(when);
    }

    /// Whether the readout panel should be visible right now: the two settings above
    /// answered together, so a host can set the panel's visibility from one call. Off
    /// whenever [`show_readout`](Self::show_readout) is off, and under
    /// [`WhileActive`](ReadoutWhen::WhileActive) only while a drag or a keyboard session
    /// is running.
    pub fn readout_visible(&self) -> bool {
        self.show_readout.get()
            && match self.readout_when.get() {
                ReadoutWhen::WhileActive => self.is_active(),
                _ => true,
            }
    }

    /// Set this frame's camera, selection pivot, and resolved input. Call once per
    /// frame before the toolkit's `update`. `selection_center` is the world-space
    /// gizmo centre (`None` hides the gizmo). `action_frame` is the host's resolved
    /// input for the frame; it carries the keyboard g/r/s, confirm/cancel, axis
    /// constraints, and numeric entry the controller acts on. Pass
    /// [`ActionFrame::default`] if the host does not resolve one.
    pub fn set_frame(
        &self,
        camera: Camera,
        selection_center: Option<Vec3>,
        action_frame: ActionFrame,
    ) {
        let mut frame = self.frame.borrow_mut();
        frame.camera = camera;
        frame.selection_center = selection_center;
        frame.action_frame = action_frame;
    }

    /// Set the orientation the handles are drawn and picked in: identity for world
    /// space, the object orientation for local space.
    pub fn set_space_orientation(&self, orientation: Quat) {
        self.frame.borrow_mut().space_orientation = orientation;
    }

    /// Replace the style the handles are drawn in.
    pub fn set_style(&self, style: GizmoStyle) {
        *self.style.borrow_mut() = style;
    }

    /// The current gizmo mode (translate / rotate / scale).
    pub fn mode(&self) -> GizmoMode {
        self.driver.borrow().mode()
    }

    /// Set the gizmo mode. Ignored while a session is active.
    pub fn set_mode(&self, mode: GizmoMode) {
        self.driver.borrow_mut().set_mode(mode);
    }

    /// Set the gizmo space (world / local).
    pub fn set_space(&self, space: GizmoSpace) {
        self.driver.borrow_mut().set_space(space);
    }

    /// Whether values are read relative to the start pose or as absolute
    /// (objective) world values.
    pub fn value_mode(&self) -> ValueMode {
        self.driver.borrow().value_mode()
    }

    /// Set the value mode. Pair this with the segmented control from
    /// [`value_mode_segmented`](super::readout::value_mode_segmented).
    pub fn set_value_mode(&self, mode: ValueMode) {
        self.driver.borrow_mut().set_value_mode(mode);
    }

    /// True while a drag or a keyboard session is in progress.
    pub fn is_active(&self) -> bool {
        self.driver.borrow().is_active()
    }

    /// The readout string for the active session, or `None` when idle. Feed it to a
    /// sibling text widget.
    pub fn readout(&self) -> Option<String> {
        self.driver.borrow().readout()
    }
}

impl CustomWidget for GizmoTool {
    fn measure(&self, _ctx: &CustomCtx, _style: &Style, _measure: &dyn TextMeasure) -> [f32; 2] {
        // The gizmo fills the viewport region it is placed over. This intrinsic size
        // is only a fallback; the host sizes the node to cover the viewport.
        [256.0, 256.0]
    }

    fn paint(&self, _ctx: &CustomCtx, _style: &Style, rect: Rect, out: &mut DrawList) {
        let frame = self.frame.borrow();
        let Some(centre) = frame.selection_center else {
            return;
        };
        let viewport = [rect.width(), rect.height()];
        if viewport[0] <= 0.0 || viewport[1] <= 0.0 {
            return;
        }

        let camera = &frame.camera;
        let view_proj = camera.proj_matrix() * camera.view_matrix();
        let forward = (camera.center - camera.eye_position()).normalize_or_zero();
        let scale =
            gizmo::compute_gizmo_scale(centre, camera.eye_position(), camera.fov_y, viewport[1]);

        let driver = self.driver.borrow();
        let mut shapes = Vec::new();
        let mut polylines = Vec::new();
        build_gizmo_overlays(
            &self.style.borrow(),
            driver.mode(),
            view_proj,
            forward,
            viewport,
            centre,
            scale,
            driver.hovered_axis(),
            frame.space_orientation,
            &mut shapes,
            &mut polylines,
        );

        // The builder projects into viewport-local pixels with the pane's top-left as the
        // origin, which is what the draw list takes: it hangs the items from the pane's rect
        // and orders them by their projected z_order, so the near handle draws over the far.
        out.push_overlay_items(rect, &shapes, &polylines, &[]);
    }

    fn input(&self, ctx: &CustomCtx, rect: Rect, input: &Input) -> CustomInput {
        let (camera, centre, action_frame) = {
            let frame = self.frame.borrow();
            (
                frame.camera.clone(),
                frame.selection_center,
                frame.action_frame.clone(),
            )
        };

        let viewport = Vec2::new(rect.width(), rect.height());
        let cursor_abs = Vec2::new(input.cursor[0], input.cursor[1]);
        let pointer_delta = match self.last_cursor.get() {
            Some(previous) => cursor_abs - previous,
            None => Vec2::ZERO,
        };
        self.last_cursor.set(Some(cursor_abs));

        // Whether the pointer is the gizmo's to act on. The pane covers the whole
        // viewport, so a panel or button drawn over it can sit on top of a handle;
        // the toolkit reports which widget the pointer is really on, and chrome above
        // the pane owns it. A live session keeps the pointer wherever it travels, so a
        // drag or a keyboard move carries on across a panel.
        let session_live = self.driver.borrow().is_active();
        let ours = ctx.state.hovered || session_live;
        // Withhold the cursor when it is not ours, so the hit-test clears the hovered
        // handle instead of lighting one up under a panel.
        let cursor_viewport = ours.then(|| cursor_abs - Vec2::new(rect.min[0], rect.min[1]));

        // The host's resolved action frame carries keyboard g/r/s and numeric entry;
        // the pointer flags below drive the gizmo drag. The controller reads each from
        // its own source, so they compose.
        let result = self.driver.borrow_mut().update(FrameInput {
            action_frame: &action_frame,
            camera: &camera,
            viewport_size: viewport,
            cursor_viewport,
            pointer_delta,
            selection_center: centre,
            drag_started: ours && input.primary_pressed,
            dragging: ours && input.primary_down,
            clicked: ours && input.primary_released,
        });

        let (active, hovered) = {
            let driver = self.driver.borrow();
            (driver.is_active(), driver.hovered_axis())
        };
        // Hold the pointer while a handle is hovered or a drag is live, so the camera
        // and panels yield only where the gizmo is actually interactive.
        let capture_pointer = active || hovered != GizmoAxis::None;
        // The shape over a handle: open while it can be taken, closed while it is being
        // dragged. Left alone elsewhere, so empty viewport and chrome keep their own.
        let cursor = if active {
            Some(Cursor::Grabbing)
        } else if hovered != GizmoAxis::None {
            Some(Cursor::Grab)
        } else {
            None
        };

        let output = gizmo_result(result)
            .map(|result| serde_json::to_value(result).expect("a gizmo result serialises"));

        CustomInput {
            output,
            capture_pointer,
            cursor,
            ..Default::default()
        }
    }
}

/// The JSON schema of the tool's output, for `CustomParamSchemas::register_output`.
/// A host that registers it can validate an emitted result at the edge.
pub fn output_schema() -> serde_json::Value {
    let vec3 = serde_json::json!({
        "type": "array", "items": { "type": "number" }, "minItems": 3, "maxItems": 3
    });
    let vec4 = serde_json::json!({
        "type": "array", "items": { "type": "number" }, "minItems": 4, "maxItems": 4
    });
    // A typed value per axis, null where nothing was typed.
    let overrides = serde_json::json!({
        "type": "array",
        "items": { "type": ["number", "null"] },
        "minItems": 3,
        "maxItems": 3
    });
    serde_json::json!({
        "oneOf": [
            {
                "type": "object",
                "required": ["Update"],
                "properties": {
                    "Update": {
                        "type": "object",
                        "required": ["translation", "rotation", "scale"],
                        "properties": {
                            "translation": vec3,
                            "rotation": vec4,
                            "scale": vec3,
                            "position_override": overrides,
                            "rotation_override": overrides,
                            "scale_override": overrides
                        }
                    }
                }
            },
            { "enum": ["Commit", "Cancel", "ConstraintChanged"] }
        ]
    })
}

fn gizmo_result(result: ManipResult) -> Option<GizmoResult> {
    match result {
        ManipResult::None => None,
        ManipResult::Update(delta) => Some(GizmoResult::Update(delta.into())),
        ManipResult::Commit => Some(GizmoResult::Commit),
        ManipResult::Cancel => Some(GizmoResult::Cancel),
        ManipResult::ConstraintChanged => Some(GizmoResult::ConstraintChanged),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use viewport_lib_ui_core::{ApproxMeasure, CustomState, NoData, NoSource};

    const VP: [f32; 2] = [1280.0, 720.0];

    fn camera() -> Camera {
        let mut camera = Camera::default();
        camera.center = Vec3::ZERO;
        camera.distance = 6.0;
        camera
    }

    /// Project a world point to a viewport pixel, matching the overlay builder.
    fn to_px(camera: &Camera, world: Vec3) -> Vec2 {
        let clip = (camera.proj_matrix() * camera.view_matrix()) * world.extend(1.0);
        let ndc = clip.truncate() / clip.w;
        Vec2::new(
            (ndc.x * 0.5 + 0.5) * VP[0],
            (1.0 - (ndc.y * 0.5 + 0.5)) * VP[1],
        )
    }

    /// A context with the pointer on the pane, the usual case.
    fn ctx<'a>(
        params: &'a serde_json::Value,
        src: &'a NoSource,
        data: &'a NoData,
    ) -> CustomCtx<'a> {
        ctx_hovered(params, src, data, true)
    }

    /// A context whose `hovered` says whether the pane is the widget the pointer is
    /// on, the way the toolkit's interaction walk settles it: false when chrome is
    /// drawn over the pane at the cursor.
    fn ctx_hovered<'a>(
        params: &'a serde_json::Value,
        src: &'a NoSource,
        data: &'a NoData,
        hovered: bool,
    ) -> CustomCtx<'a> {
        CustomCtx {
            params,
            values: src,
            data,
            content: &[],
            rect: None,
            state: CustomState {
                hovered,
                ..Default::default()
            },
        }
    }

    fn full_rect() -> Rect {
        Rect::new([0.0, 0.0], VP)
    }

    fn x_handle_cursor(camera: &Camera) -> Vec2 {
        let scale =
            gizmo::compute_gizmo_scale(Vec3::ZERO, camera.eye_position(), camera.fov_y, VP[1]);
        to_px(camera, Vec3::X * (scale * 0.5))
    }

    #[test]
    fn transform_delta_maps_to_arrays() {
        let delta = TransformDelta {
            translation: Vec3::new(1.0, 2.0, 3.0),
            ..Default::default()
        };
        let mapped: GizmoDelta = delta.into();
        assert_eq!(mapped.translation, [1.0, 2.0, 3.0]);
        assert_eq!(mapped.rotation, [0.0, 0.0, 0.0, 1.0]);
        assert_eq!(mapped.scale, [1.0, 1.0, 1.0]);
    }

    #[test]
    fn painting_with_a_selection_emits_draw_commands() {
        let tool = GizmoTool::new();
        tool.set_frame(camera(), Some(Vec3::ZERO), ActionFrame::default());
        let (params, src, data) = (serde_json::Value::Null, NoSource, NoData);
        let mut list = DrawList::new();
        tool.paint(
            &ctx(&params, &src, &data),
            &Style::default(),
            full_rect(),
            &mut list,
        );
        assert!(!list.is_empty(), "a selected gizmo paints its handles");
    }

    #[test]
    fn painting_without_a_selection_emits_nothing() {
        let tool = GizmoTool::new();
        let (params, src, data) = (serde_json::Value::Null, NoSource, NoData);
        let mut list = DrawList::new();
        tool.paint(
            &ctx(&params, &src, &data),
            &Style::default(),
            full_rect(),
            &mut list,
        );
        assert!(list.is_empty(), "no selection draws nothing");
    }

    #[test]
    fn hovering_a_handle_captures_the_pointer() {
        let tool = GizmoTool::new();
        let cam = camera();
        tool.set_frame(cam.clone(), Some(Vec3::ZERO), ActionFrame::default());
        let (params, src, data) = (serde_json::Value::Null, NoSource, NoData);

        // Cursor over empty space does not capture.
        let idle = tool.input(
            &ctx(&params, &src, &data),
            full_rect(),
            &Input::at([5.0, 5.0]),
        );
        assert!(!idle.capture_pointer, "empty space yields to the camera");

        // Cursor over the X arm captures.
        let cursor = x_handle_cursor(&cam);
        let over = tool.input(
            &ctx(&params, &src, &data),
            full_rect(),
            &Input::at(cursor.to_array()),
        );
        assert!(over.capture_pointer, "hovering a handle holds the pointer");
    }

    #[test]
    fn chrome_over_a_handle_takes_the_press_off_the_gizmo() {
        let tool = GizmoTool::new();
        let cam = camera();
        tool.set_frame(cam.clone(), Some(Vec3::ZERO), ActionFrame::default());
        let (params, src, data) = (serde_json::Value::Null, NoSource, NoData);
        let cursor = x_handle_cursor(&cam);

        // The same point on the X arm, but with a control drawn over the pane there, so
        // the pane is not the widget the pointer is on.
        let covered = ctx_hovered(&params, &src, &data, false);
        let over = tool.input(&covered, full_rect(), &Input::at(cursor.to_array()));
        assert!(
            !over.capture_pointer,
            "the control above the handle owns the pointer"
        );
        assert_eq!(over.cursor, None, "and keeps its own shape");

        // The press belongs to that control: it must not begin a gizmo drag.
        let pressed = tool.input(&covered, full_rect(), &Input::at(cursor.to_array()).press());
        assert!(!pressed.capture_pointer);
        assert!(pressed.output.is_none(), "no drag began under the chrome");
        assert!(!tool.is_active());

        // Uncovered, the same point is the gizmo's again.
        let over = tool.input(
            &ctx(&params, &src, &data),
            full_rect(),
            &Input::at(cursor.to_array()),
        );
        assert!(
            over.capture_pointer,
            "the handle is live where nothing covers it"
        );
    }

    #[test]
    fn a_drag_carries_on_across_chrome() {
        let tool = GizmoTool::new();
        let cam = camera();
        tool.set_frame(cam.clone(), Some(Vec3::ZERO), ActionFrame::default());
        let (params, src, data) = (serde_json::Value::Null, NoSource, NoData);
        let start = x_handle_cursor(&cam);

        // Begin the drag on the bare handle.
        tool.input(
            &ctx(&params, &src, &data),
            full_rect(),
            &Input::at(start.to_array()).press(),
        );
        assert!(tool.is_active());

        // The pointer travels over a panel mid-drag. The session owns it regardless of
        // what is drawn under the cursor, so the drag still reports its delta.
        let mut held = Input::at([start.x + 60.0, start.y]);
        held.primary_down = true;
        let dragged = tool.input(
            &ctx_hovered(&params, &src, &data, false),
            full_rect(),
            &held,
        );
        assert!(dragged.capture_pointer, "a live drag keeps the pointer");
        assert!(dragged.output.is_some(), "and keeps reporting its delta");
    }

    #[test]
    fn the_pointer_reads_as_a_grab_over_a_handle() {
        let tool = GizmoTool::new();
        let cam = camera();
        tool.set_frame(cam.clone(), Some(Vec3::ZERO), ActionFrame::default());
        let (params, src, data) = (serde_json::Value::Null, NoSource, NoData);

        // Empty viewport: the gizmo names no shape, so the host's own stands.
        let idle = tool.input(
            &ctx(&params, &src, &data),
            full_rect(),
            &Input::at([5.0, 5.0]),
        );
        assert_eq!(idle.cursor, None);

        // Over the X arm: open, the handle can be taken.
        let cursor = x_handle_cursor(&cam);
        let over = tool.input(
            &ctx(&params, &src, &data),
            full_rect(),
            &Input::at(cursor.to_array()),
        );
        assert_eq!(over.cursor, Some(Cursor::Grab));

        // Dragging it: closed.
        let dragging = tool.input(
            &ctx(&params, &src, &data),
            full_rect(),
            &Input::at(cursor.to_array()).press(),
        );
        assert_eq!(dragging.cursor, Some(Cursor::Grabbing));
    }

    #[test]
    fn the_toolkit_reports_the_grab_cursor_for_a_hovered_handle() {
        // The whole chain a host reads: the walk hovers the pane, the handler names the
        // shape for the handle under the pointer, and `UiState::cursor` reports it. A
        // host that applies that to its window gets the hand over the handle.
        use viewport_lib_ui_core::{Cursor, UiState, WidgetRegistry};
        use viewport_lib_ui_document::{Anchor, Layout, Screen, Widget, WidgetKind};

        let tool = std::sync::Arc::new(GizmoTool::new());
        let mut registry = WidgetRegistry::new();
        registry.register(GIZMO_KIND, tool.clone());
        let mut screen = Screen::new(
            Widget::new(0, WidgetKind::Panel)
                .with_layout(Layout::Free)
                .with_child(
                    Widget::custom(1, GIZMO_KIND, serde_json::Value::Null).placed(
                        Anchor::TopLeft,
                        [0.0, 0.0],
                        VP,
                    ),
                ),
        );

        let cam = camera();
        let mut ui = UiState::new();
        let mut frame = |input: Input| {
            tool.set_frame(cam.clone(), Some(Vec3::ZERO), ActionFrame::default());
            ui.update_customs(
                &mut screen,
                &input,
                VP,
                &ApproxMeasure::default(),
                &registry,
                &NoSource,
                &NoData,
            );
            ui.cursor()
        };

        let on_handle = x_handle_cursor(&cam);
        assert_eq!(
            frame(Input::at(on_handle.to_array())),
            Cursor::Grab,
            "an open hand over the handle"
        );
        assert_eq!(
            frame(Input::at(on_handle.to_array()).press()),
            Cursor::Grabbing,
            "and a closed one once it is taken"
        );
    }

    #[test]
    fn dragging_a_handle_reports_a_typed_update() {
        let tool = GizmoTool::new();
        let cam = camera();
        tool.set_frame(cam.clone(), Some(Vec3::ZERO), ActionFrame::default());
        let (params, src, data) = (serde_json::Value::Null, NoSource, NoData);
        let start = x_handle_cursor(&cam);

        // Press on the X arm to begin the drag.
        let press = tool.input(
            &ctx(&params, &src, &data),
            full_rect(),
            &Input::at(start.to_array()).press(),
        );
        assert!(press.capture_pointer, "a live drag holds the pointer");

        // Drag further along the screen-projected X direction, primary still held.
        let mut held = Input::at([start.x + 60.0, start.y]);
        held.primary_down = true;
        let dragged = tool.input(&ctx(&params, &src, &data), full_rect(), &held);

        let value = dragged.output.expect("a drag frame reports a result");
        match serde_json::from_value::<GizmoResult>(value).expect("the result deserialises") {
            GizmoResult::Update(delta) => {
                assert!(
                    delta.translation.iter().any(|v| v.abs() > 1e-4),
                    "the drag moved the object: {delta:?}"
                );
            }
            other => panic!("expected an update, got {other:?}"),
        }
    }

    #[test]
    fn a_typed_number_reaches_the_host_through_the_delta() {
        use viewport_lib_types::input::{Action, ResolvedActionState};

        let tool = GizmoTool::new();
        let cam = camera();
        let (params, src, data) = (serde_json::Value::Null, NoSource, NoData);
        let cursor = Input::at([640.0, 360.0]);

        // Press G to begin a keyboard move.
        let mut begin = ActionFrame::default();
        begin
            .actions
            .insert(Action::BeginMove, ResolvedActionState::Pressed);
        tool.set_frame(cam.clone(), Some(Vec3::ZERO), begin);
        tool.input(&ctx(&params, &src, &data), full_rect(), &cursor);
        assert!(tool.is_active(), "G starts a session");

        // Type "5". It lands on the first axis and must reach the host, not just the
        // readout: a number typed into a session is how the value is set at all.
        let mut typed = ActionFrame::default();
        typed.typed_chars = vec!['5'];
        tool.set_frame(cam.clone(), Some(Vec3::ZERO), typed);
        let out = tool.input(&ctx(&params, &src, &data), full_rect(), &cursor);

        let value = out.output.expect("the typed frame reports a result");
        match serde_json::from_value::<GizmoResult>(value).expect("the result deserialises") {
            GizmoResult::Update(delta) => {
                assert_eq!(
                    delta.position_override[0],
                    Some(5.0),
                    "the typed value carries"
                );
                assert_eq!(
                    delta.position()[0],
                    5.0,
                    "and stands in for the dragged one"
                );
                assert_eq!(delta.position_override[1], None, "untyped axes stay open");
            }
            other => panic!("expected an update, got {other:?}"),
        }
    }

    #[test]
    fn the_result_round_trips_and_the_schema_is_an_object() {
        // Update serialises as a tagged object; the unit variants as their names.
        let update = serde_json::to_value(GizmoResult::Update(GizmoDelta::default())).unwrap();
        assert!(update.get("Update").is_some(), "update is tagged: {update}");
        let back: GizmoResult = serde_json::from_value(update).unwrap();
        assert_eq!(back, GizmoResult::Update(GizmoDelta::default()));

        let commit = serde_json::to_value(GizmoResult::Commit).unwrap();
        assert_eq!(commit, serde_json::json!("Commit"));

        // The published schema is a JSON object a host can register as-is.
        assert!(output_schema().is_object());
    }
}
