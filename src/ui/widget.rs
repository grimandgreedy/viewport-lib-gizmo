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

use std::cell::{Cell, RefCell};

use glam::{Quat, Vec2, Vec3};
use serde::{Deserialize, Serialize};
use viewport_lib_types::camera::Camera;
use viewport_lib_types::input::ActionFrame;

use viewport_lib_ui_core::serde_json;
use viewport_lib_ui_core::{
    CustomCtx, CustomInput, CustomWidget, DrawList, Input, Rect, Style, TextMeasure,
};

use crate::controller::driver::FrameInput;
use crate::controller::{GizmoDriver, ManipResult, TransformDelta, ValueMode};
use crate::maths::gizmo::{self, GizmoAxis, GizmoMode, GizmoSpace};
use crate::overlay::builder::build_gizmo_overlays;
use crate::overlay::style::GizmoStyle;

use super::convert::push_gizmo_overlays;

/// The default `kind` string to register [`GizmoTool`] under.
pub const GIZMO_KIND: &str = "gizmo";

/// A transform delta as plain arrays, so it serialises without a glam serde
/// dependency. `rotation` is a quaternion in `[x, y, z, w]` order.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct GizmoDelta {
    /// Total translation since the drag began.
    pub translation: [f32; 3],
    /// Total rotation since the drag began, as a quaternion `[x, y, z, w]`.
    pub rotation: [f32; 4],
    /// Total scale since the drag began.
    pub scale: [f32; 3],
}

impl Default for GizmoDelta {
    fn default() -> Self {
        Self {
            translation: [0.0; 3],
            rotation: [0.0, 0.0, 0.0, 1.0],
            scale: [1.0; 3],
        }
    }
}

impl From<TransformDelta> for GizmoDelta {
    fn from(delta: TransformDelta) -> Self {
        Self {
            translation: delta.translation.to_array(),
            rotation: delta.rotation.to_array(),
            scale: delta.scale.to_array(),
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

/// The camera and selection the host supplies for a frame.
struct FrameState {
    camera: Camera,
    selection_center: Option<Vec3>,
    space_orientation: Quat,
}

impl Default for FrameState {
    fn default() -> Self {
        Self {
            camera: Camera::default(),
            selection_center: None,
            space_orientation: Quat::IDENTITY,
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

    /// Set this frame's camera and selection pivot. Call once per frame before the
    /// toolkit's `update`. `selection_center` is the world-space gizmo centre;
    /// `None` hides the gizmo.
    pub fn set_frame(&self, camera: Camera, selection_center: Option<Vec3>) {
        let mut frame = self.frame.borrow_mut();
        frame.camera = camera;
        frame.selection_center = selection_center;
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

        // The builder projects into viewport-local pixels (pane top-left is the
        // origin). Shift into the draw list's space so the handles land where the
        // pane sits.
        let [ox, oy] = rect.min;
        for shape in &mut shapes {
            shape.position = [shape.position[0] + ox, shape.position[1] + oy];
        }
        for line in &mut polylines {
            for point in &mut line.points {
                point[0] += ox;
                point[1] += oy;
            }
        }

        push_gizmo_overlays(out, &shapes, &polylines);
    }

    fn input(&self, _ctx: &CustomCtx, rect: Rect, input: &Input) -> CustomInput {
        let (camera, centre) = {
            let frame = self.frame.borrow();
            (frame.camera.clone(), frame.selection_center)
        };

        let viewport = Vec2::new(rect.width(), rect.height());
        let cursor_abs = Vec2::new(input.cursor[0], input.cursor[1]);
        let cursor_viewport = Some(cursor_abs - Vec2::new(rect.min[0], rect.min[1]));
        let pointer_delta = match self.last_cursor.get() {
            Some(previous) => cursor_abs - previous,
            None => Vec2::ZERO,
        };
        self.last_cursor.set(Some(cursor_abs));

        // Keyboard g/r/s is handled host-side for now, so the action frame is empty
        // and the drag is driven purely by the pointer.
        let action_frame = ActionFrame::default();
        let result = self.driver.borrow_mut().update(FrameInput {
            action_frame: &action_frame,
            camera: &camera,
            viewport_size: viewport,
            cursor_viewport,
            pointer_delta,
            selection_center: centre,
            drag_started: input.primary_pressed,
            dragging: input.primary_down,
            clicked: input.primary_released,
        });

        let (active, hovered) = {
            let driver = self.driver.borrow();
            (driver.is_active(), driver.hovered_axis())
        };
        // Hold the pointer while a handle is hovered or a drag is live, so the camera
        // and panels yield only where the gizmo is actually interactive.
        let capture_pointer = active || hovered != GizmoAxis::None;

        let output = gizmo_result(result)
            .map(|result| serde_json::to_value(result).expect("a gizmo result serialises"));

        CustomInput {
            output,
            capture_pointer,
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
    serde_json::json!({
        "oneOf": [
            {
                "type": "object",
                "required": ["Update"],
                "properties": {
                    "Update": {
                        "type": "object",
                        "required": ["translation", "rotation", "scale"],
                        "properties": { "translation": vec3, "rotation": vec4, "scale": vec3 }
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
    use viewport_lib_ui_core::{NoData, NoSource};

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
        Vec2::new((ndc.x * 0.5 + 0.5) * VP[0], (1.0 - (ndc.y * 0.5 + 0.5)) * VP[1])
    }

    fn ctx<'a>(params: &'a serde_json::Value, src: &'a NoSource, data: &'a NoData) -> CustomCtx<'a> {
        CustomCtx {
            params,
            values: src,
            data,
            content: &[],
            rect: None,
        }
    }

    fn full_rect() -> Rect {
        Rect::new([0.0, 0.0], VP)
    }

    fn x_handle_cursor(camera: &Camera) -> Vec2 {
        let scale = gizmo::compute_gizmo_scale(Vec3::ZERO, camera.eye_position(), camera.fov_y, VP[1]);
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
        tool.set_frame(camera(), Some(Vec3::ZERO));
        let (params, src, data) = (serde_json::Value::Null, NoSource, NoData);
        let mut list = DrawList::new();
        tool.paint(&ctx(&params, &src, &data), &Style::default(), full_rect(), &mut list);
        assert!(!list.is_empty(), "a selected gizmo paints its handles");
    }

    #[test]
    fn painting_without_a_selection_emits_nothing() {
        let tool = GizmoTool::new();
        let (params, src, data) = (serde_json::Value::Null, NoSource, NoData);
        let mut list = DrawList::new();
        tool.paint(&ctx(&params, &src, &data), &Style::default(), full_rect(), &mut list);
        assert!(list.is_empty(), "no selection draws nothing");
    }

    #[test]
    fn hovering_a_handle_captures_the_pointer() {
        let tool = GizmoTool::new();
        let cam = camera();
        tool.set_frame(cam.clone(), Some(Vec3::ZERO));
        let (params, src, data) = (serde_json::Value::Null, NoSource, NoData);

        // Cursor over empty space does not capture.
        let idle = tool.input(&ctx(&params, &src, &data), full_rect(), &Input::at([5.0, 5.0]));
        assert!(!idle.capture_pointer, "empty space yields to the camera");

        // Cursor over the X arm captures.
        let cursor = x_handle_cursor(&cam);
        let over = tool.input(&ctx(&params, &src, &data), full_rect(), &Input::at(cursor.to_array()));
        assert!(over.capture_pointer, "hovering a handle holds the pointer");
    }

    #[test]
    fn dragging_a_handle_reports_a_typed_update() {
        let tool = GizmoTool::new();
        let cam = camera();
        tool.set_frame(cam.clone(), Some(Vec3::ZERO));
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
