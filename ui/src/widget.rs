//! A viewport-lib-ui `Custom` widget that draws and drives the gizmo.
//!
//! [`GizmoTool`] runs the existing [`GizmoDriver`] behind the toolkit's custom
//! widget path. Each frame the host puts the camera, the selection pivot and its
//! resolved input on the frame's host map as a [`GizmoFrame`]; the widget then
//! paints the projected handles through the draw path, hit-tests and drags itself from the toolkit's
//! pointer input, captures the pointer only while a handle is hovered or a drag is
//! live, and reports the transform as a typed [`GizmoResult`] the host reads back.
//!
//! The 3D maths, the camera, and the controller stay in the gizmo core; this type
//! only bridges them to the toolkit.
//!
//! # One tool, any number of panes
//!
//! Register one tool under [`GIZMO_KIND`] and keep an `Arc` of it to set its mode
//! and read its session back. Every `Custom` node of that kind is a pane of the same tool:
//! the panes show one selection and share one driver, so a drag or a G / R / S
//! session moves the selection once however many views show it. The pane the
//! pointer is on drives the driver, a session stays with the pane it started in
//! until it ends, and the other panes paint the gizmo sized for their own camera
//! and show the axis being changed. What a pane keeps between frames (the last
//! cursor, the handle it has hovered) lives in the toolkit's per-node state.
//!
//! Each pane draws through the camera of the frame it is in, so a host with
//! several views builds each view's frame with that view's [`GizmoFrame`]. The
//! tool tells the driving pane by its widget id, so panes on separate screens
//! still need ids of their own.
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

use std::sync::{LazyLock, Mutex, MutexGuard};

use glam::Vec2;
use serde::{Deserialize, Serialize};

use viewport_lib_ui_extend::custom::{CustomChrome, CustomCtx, CustomInput, CustomWidget};
use viewport_lib_ui_extend::document::{Style, WidgetId};
use viewport_lib_ui_extend::draw::DrawList;
use viewport_lib_ui_extend::geometry::Rect;
use viewport_lib_ui_extend::input::{Cursor, Input};
use viewport_lib_ui_extend::layout::TextMeasure;
use viewport_lib_ui_extend::serde_json;

use crate::frame::GizmoFrame;
use viewport_lib_gizmo::controller::FrameInput;
use viewport_lib_gizmo::controller::{GizmoDriver, ManipResult, TransformDelta, ValueMode};
use viewport_lib_gizmo::maths::gizmo::{self, GizmoAxis, GizmoMode, GizmoSpace};
use viewport_lib_gizmo::overlay::builder::build_gizmo_overlays;
use viewport_lib_gizmo::overlay::style::GizmoStyle;

/// The `kind` string to register [`GizmoTool`] under. It carries the `vpl.` prefix
/// because the gizmo is part of the viewport-lib family, the same namespace the
/// toolkit's own kinds use.
pub const GIZMO_KIND: &str = "vpl.gizmo";

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
    /// one on its axis. In [`ValueMode::Objective`] a typed value is an absolute
    /// world coordinate, so add the start position back yourself rather than using
    /// this.
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

/// The driver every pane shares, and which pane drives it.
struct Session {
    driver: GizmoDriver,
    /// The pane that drives the driver: the one last hovered while no session ran.
    /// A session stays with the pane it started in. `None` until a pane has taken it.
    holder: Option<WidgetId>,
}

/// How the tool looks and when its readout shows.
struct Look {
    style: GizmoStyle,
    show_readout: bool,
    readout_when: ReadoutWhen,
}

/// What the tool holds for all its panes: the shared session and the look.
struct Shared {
    session: Session,
    look: Look,
}

/// What one pane keeps between frames, in the toolkit's state for its node.
struct PaneState {
    /// Where the pointer was on the pane's last driving frame, for the pointer delta.
    last_cursor: Option<Vec2>,
    /// The handle under the pointer as of the pane's last driving frame.
    hovered: GizmoAxis,
}

impl Default for PaneState {
    fn default() -> Self {
        Self {
            last_cursor: None,
            hovered: GizmoAxis::None,
        }
    }
}

/// The gizmo as a viewport-lib-ui custom widget. Construct one, register an `Arc`
/// of it under [`GIZMO_KIND`], and give each frame a [`GizmoFrame`] on its host
/// map.
///
/// One tool serves every gizmo pane on the screen, and on any other screen it is
/// registered for. The panes share one driver; see the module docs.
pub struct GizmoTool {
    shared: Mutex<Shared>,
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
            shared: Mutex::new(Shared {
                session: Session {
                    driver: GizmoDriver::new(),
                    holder: None,
                },
                look: Look {
                    style,
                    show_readout: true,
                    readout_when: ReadoutWhen::Always,
                },
            }),
        }
    }

    fn shared(&self) -> MutexGuard<'_, Shared> {
        // Nothing here is left half-written by a panic, so a poisoned lock is still
        // a usable one.
        self.shared
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// The pane driving the shared driver, by widget id: the one last hovered
    /// while no session ran, or the one the running session started in. `None`
    /// before any pane has had the pointer.
    pub fn driving_view(&self) -> Option<WidgetId> {
        self.shared().session.holder
    }

    /// Whether the numeric readout panel should be shown. Defaults to `true`. The
    /// host reads this to toggle the visibility of the panel built by
    /// [`readout_panel`](crate::readout::readout_panel).
    pub fn show_readout(&self) -> bool {
        self.shared().look.show_readout
    }

    /// Set whether the readout panel is shown.
    pub fn set_show_readout(&self, show: bool) {
        self.shared().look.show_readout = show;
    }

    /// When the readout panel is worth showing. Defaults to
    /// [`Always`](ReadoutWhen::Always).
    pub fn readout_when(&self) -> ReadoutWhen {
        self.shared().look.readout_when
    }

    /// Set when the readout panel is worth showing.
    pub fn set_readout_when(&self, when: ReadoutWhen) {
        self.shared().look.readout_when = when;
    }

    /// Whether the readout panel should be visible right now: the two settings above
    /// answered together, so a host can set the panel's visibility from one call. Off
    /// whenever [`show_readout`](Self::show_readout) is off, and under
    /// [`WhileActive`](ReadoutWhen::WhileActive) only while a drag or a keyboard session
    /// is running.
    pub fn readout_visible(&self) -> bool {
        let shared = self.shared();
        shared.look.show_readout
            && match shared.look.readout_when {
                ReadoutWhen::WhileActive => shared.session.driver.is_active(),
                _ => true,
            }
    }

    /// Replace the style the handles are drawn in.
    pub fn set_style(&self, style: GizmoStyle) {
        self.shared().look.style = style;
    }

    /// The current gizmo mode (translate / rotate / scale).
    pub fn mode(&self) -> GizmoMode {
        self.shared().session.driver.mode()
    }

    /// Set the gizmo mode. Ignored while a session is active.
    pub fn set_mode(&self, mode: GizmoMode) {
        self.shared().session.driver.set_mode(mode);
    }

    /// Set the gizmo space (world / local).
    pub fn set_space(&self, space: GizmoSpace) {
        self.shared().session.driver.set_space(space);
    }

    /// Whether values are read relative to the start pose or as absolute
    /// (objective) world values.
    pub fn value_mode(&self) -> ValueMode {
        self.shared().session.driver.value_mode()
    }

    /// Set the value mode. Pair this with the segmented control from
    /// [`value_mode_segmented`](crate::readout::value_mode_segmented).
    pub fn set_value_mode(&self, mode: ValueMode) {
        self.shared().session.driver.set_value_mode(mode);
    }

    /// True while a drag or a keyboard session is in progress.
    pub fn is_active(&self) -> bool {
        self.shared().session.driver.is_active()
    }

    /// The readout string for the active session, or `None` when idle. Feed it to a
    /// sibling text widget.
    pub fn readout(&self) -> Option<String> {
        self.shared().session.driver.readout()
    }
}

impl CustomWidget for GizmoTool {
    fn measure(&self, _ctx: &CustomCtx, _style: &Style, _measure: &dyn TextMeasure) -> [f32; 2] {
        // The gizmo fills the viewport region it is placed over. This intrinsic size
        // is only a fallback; the host sizes the node to cover the viewport.
        [256.0, 256.0]
    }

    /// Faded while disabled, as a control is, but never ringed: the pane covers
    /// the whole viewport, so a ring would outline the view, and its keyboard
    /// sessions come from the host's resolved input rather than from focus.
    fn chrome(&self) -> CustomChrome {
        CustomChrome::toolkit().with_focus_ring(false)
    }

    fn paint(&self, ctx: &CustomCtx, _style: &Style, rect: Rect, out: &mut DrawList) {
        let frame = frame(ctx);
        let Some(centre) = frame.selection_centre else {
            return;
        };
        let shared = self.shared();
        let viewport = [rect.width(), rect.height()];
        if viewport[0] <= 0.0 || viewport[1] <= 0.0 {
            return;
        }

        let camera = &frame.camera;
        let view_proj = camera.proj_matrix() * camera.view_matrix();
        let forward = camera.forward();
        let scale = gizmo::gizmo_scale_for_camera(centre, camera, viewport[1]);

        // A running session lights the axis being changed in every pane. Otherwise a
        // pane lights the handle it last saw hovered, and only while the pointer is
        // still on it, so a pane the pointer has left does not keep a stale highlight.
        let driver = &shared.session.driver;
        let highlight = if driver.is_active() {
            driver.highlight(false)
        } else if ctx.interaction.hovered {
            ctx.state::<PaneState>().hovered
        } else {
            GizmoAxis::None
        };

        let space_orientation = match driver.space() {
            GizmoSpace::Local => frame.selection_orientation,
            _ => glam::Quat::IDENTITY,
        };

        let mut shapes = Vec::new();
        let mut polylines = Vec::new();
        build_gizmo_overlays(
            &shared.look.style,
            driver.mode(),
            view_proj,
            forward,
            viewport,
            centre,
            scale,
            highlight,
            space_orientation,
            &mut shapes,
            &mut polylines,
        );

        // The builder projects into viewport-local pixels with the pane's top-left as the
        // origin, which is what the draw list takes: it hangs the items from the pane's rect
        // and orders them by their projected z_order, so the near handle draws over the far.
        out.push_overlay_items(rect, &shapes, &polylines, &[]);
    }

    fn input(&self, ctx: &CustomCtx, rect: Rect, input: &Input) -> CustomInput {
        let frame = frame(ctx);
        let mut pane = ctx.state::<PaneState>();
        let mut guard = self.shared();
        let session = &mut guard.session;
        let view = ctx.id();

        // The hovered pane takes over the driver while no session runs, and only the
        // holder drives, so the driver sees each frame's input once. A pane with no
        // other to compete with takes it on its first frame.
        if !session.driver.is_active() && (ctx.interaction.hovered || session.holder.is_none()) {
            session.holder = Some(view);
        }
        if session.holder != Some(view) {
            // Forget the cursor, so the first frame back does not read the whole way
            // the pointer travelled meanwhile as one move.
            *pane = PaneState::default();
            return CustomInput::new();
        }

        let viewport = Vec2::new(rect.width(), rect.height());
        let cursor_abs = Vec2::new(input.pointer.cursor[0], input.pointer.cursor[1]);
        let pointer_delta = match pane.last_cursor {
            Some(previous) => cursor_abs - previous,
            None => Vec2::ZERO,
        };
        pane.last_cursor = Some(cursor_abs);

        // Whether the pointer is the gizmo's to act on. The pane covers the whole
        // viewport, so a panel or button drawn over it can sit on top of a handle;
        // the toolkit reports which widget the pointer is really on, and chrome above
        // the pane owns it. A live session keeps the pointer wherever it travels, so a
        // drag or a keyboard move carries on across a panel.
        let ours = ctx.interaction.hovered || session.driver.is_active();
        // Withhold the cursor when it is not ours, so the hit-test clears the hovered
        // handle instead of lighting one up under a panel.
        let cursor_viewport = ours.then(|| cursor_abs - Vec2::new(rect.min[0], rect.min[1]));

        // The host's resolved action frame carries keyboard g/r/s and numeric entry;
        // the pointer flags below drive the gizmo drag. The controller reads each from
        // its own source, so they compose.
        let result = session.driver.update(FrameInput {
            action_frame: &frame.action_frame,
            camera: &frame.camera,
            viewport_size: viewport,
            cursor_viewport,
            pointer_delta,
            selection_centre: frame.selection_centre,
            selection_orientation: frame.selection_orientation,
            drag_started: ours && input.buttons.primary_pressed,
            dragging: ours && input.buttons.primary_down,
            clicked: ours && input.buttons.primary_released,
        });

        let active = session.driver.is_active();
        let hovered = session.driver.hovered_axis();
        pane.hovered = hovered;
        // Hold the pointer while a handle is hovered or a drag is live, so the camera
        // and panels yield only where the gizmo is actually interactive.
        let mut out = CustomInput::new().with_pointer_capture(active || hovered != GizmoAxis::None);
        // The shape over a handle: open while it can be taken, closed while it is being
        // dragged. Left alone elsewhere, so empty viewport and chrome keep their own.
        if active {
            out = out.with_cursor(Cursor::Grabbing);
        } else if hovered != GizmoAxis::None {
            out = out.with_cursor(Cursor::Grab);
        }
        if let Some(result) = gizmo_result(result) {
            out = out.with_output(serde_json::to_value(result).expect("a gizmo result serialises"));
        }
        out
    }
}

/// The frame `ctx` is in, or an empty one (no selection) when the host set none.
fn frame<'a>(ctx: &CustomCtx<'a>) -> &'a GizmoFrame {
    static NONE: LazyLock<GizmoFrame> = LazyLock::new(GizmoFrame::default);
    ctx.context::<GizmoFrame>().unwrap_or(&NONE)
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
        // A result added to the non-exhaustive ManipResult is not reported yet.
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Vec3;
    use viewport_lib_types::camera::Camera;
    use viewport_lib_ui_extend::custom::{CustomHost, CustomNode};
    use viewport_lib_ui_extend::sources::HostMap;

    const VP: [f32; 2] = [1280.0, 720.0];

    fn camera() -> Camera {
        Camera::orbit(glam::DVec3::ZERO, 6.0, Camera::default().orientation())
    }

    /// A frame through `camera` with the gizmo at the origin.
    fn selected(camera: Camera) -> HostMap<'static> {
        HostMap::new().with_context(GizmoFrame::new(camera).with_selection_centre(Some(Vec3::ZERO)))
    }

    fn paint(tool: &GizmoTool, map: &HostMap) -> DrawList {
        let params = serde_json::Value::Null;
        let mut list = DrawList::new();
        let host = CustomHost::new().with_map(map);
        tool.paint(
            &CustomCtx::new(CustomNode::new(1, GIZMO_KIND, &params)).with_host(host),
            &Style::default(),
            Rect::new([0.0, 0.0], VP),
            &mut list,
        );
        list
    }

    #[test]
    fn the_tool_can_be_shared_across_threads() {
        fn send_sync<T: Send + Sync>() {}
        send_sync::<GizmoTool>();
    }

    #[test]
    fn transform_delta_maps_to_arrays() {
        let mut delta = TransformDelta::default();
        delta.translation = Vec3::new(1.0, 2.0, 3.0);
        let mapped: GizmoDelta = delta.into();
        assert_eq!(mapped.translation, [1.0, 2.0, 3.0]);
        assert_eq!(mapped.rotation, [0.0, 0.0, 0.0, 1.0]);
        assert_eq!(mapped.scale, [1.0, 1.0, 1.0]);
    }

    #[test]
    fn painting_with_a_selection_emits_draw_commands() {
        let tool = GizmoTool::new();
        assert!(
            !paint(&tool, &selected(camera())).is_empty(),
            "a selected gizmo paints its handles"
        );
    }

    #[test]
    fn painting_without_a_selection_emits_nothing() {
        let tool = GizmoTool::new();
        let unselected = HostMap::new().with_context(GizmoFrame::new(camera()));
        assert!(
            paint(&tool, &unselected).is_empty(),
            "no selection draws nothing"
        );
        assert!(
            paint(&tool, &HostMap::new()).is_empty(),
            "a frame with no gizmo context draws nothing"
        );
    }

    #[test]
    fn each_frame_draws_through_its_own_camera() {
        let tool = GizmoTool::new();
        let near = paint(&tool, &selected(camera()));
        let mut far = camera();
        far.set_orbit_distance(40.0);
        far.set_orbit_orientation(glam::Quat::from_rotation_y(0.8));
        assert_ne!(
            paint(&tool, &selected(far)),
            near,
            "another frame's camera moves the handles"
        );
        assert_eq!(
            paint(&tool, &selected(camera())),
            near,
            "the tool keeps no camera of its own between frames"
        );
    }

    #[test]
    fn the_pane_fades_when_disabled_but_draws_no_focus_ring() {
        let chrome = GizmoTool::new().chrome();
        assert!(!chrome.focus_ring);
        assert!(chrome.disabled_dim);
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
