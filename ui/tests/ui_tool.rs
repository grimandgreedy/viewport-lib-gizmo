//! The gizmo tool driven through the toolkit's frame loop: hover, capture, the
//! cursor shape, drags, keyboard sessions, and one tool behind several panes.

use std::sync::Arc;

use glam::{Vec2, Vec3};
use viewport_lib_gizmo::GizmoMode;
use viewport_lib_gizmo::compute_gizmo_scale;
use viewport_lib_gizmo_ui::{GIZMO_KIND, GizmoFrame, GizmoResult, GizmoTool};
use viewport_lib_types::camera::Camera;
use viewport_lib_types::input::{Action, ActionFrame, ResolvedActionState};
use viewport_lib_ui::{ApproxMeasure, Frame, HostSources};
use viewport_lib_ui_document::{Alignment, Layout, Screen, Widget, WidgetKind};
use viewport_lib_ui_extend::custom::WidgetRegistry;
use viewport_lib_ui_extend::draw::DrawCommand;
use viewport_lib_ui_extend::events::UiEvent;
use viewport_lib_ui_extend::input::{Cursor, Input};
use viewport_lib_ui_extend::serde_json;
use viewport_lib_ui_extend::sources::HostMap;
use viewport_lib_ui_test::Harness;

const VP: [f32; 2] = [1280.0, 720.0];
const ROOT: u32 = 100;
const PANE: u32 = 1;
/// A button laid over the X handle, for the tests about chrome above the pane.
const COVER: u32 = 50;

fn camera() -> Camera {
    Camera::orbit(glam::DVec3::ZERO, 6.0, Camera::default().orientation())
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

/// A point on the X arm of a gizmo at the origin.
fn x_handle() -> [f32; 2] {
    let camera = camera();
    let scale = compute_gizmo_scale(Vec3::ZERO, camera.eye().as_vec3(), camera.fov_y(), VP[1]);
    to_px(&camera, Vec3::X * (scale * 0.5)).to_array()
}

fn tool() -> Arc<GizmoTool> {
    Arc::new(GizmoTool::new())
}

/// A frame through `camera` on a selection at the origin, with `action` as the
/// host's resolved input.
fn framed(camera: Camera, action: ActionFrame) -> GizmoFrame {
    GizmoFrame::new(camera)
        .with_selection_centre(Some(Vec3::ZERO))
        .with_action_frame(action)
}

/// The map a host hands a frame through `camera` on a selection at the origin.
fn view(camera: Camera) -> HostMap<'static> {
    HostMap::new().with_context(framed(camera, ActionFrame::default()))
}

fn pane(id: u32) -> Widget {
    Widget::custom(id, GIZMO_KIND, serde_json::Value::Null).placed(
        Alignment::TOP_LEFT,
        [0.0, 0.0],
        VP,
    )
}

fn registry(tool: &Arc<GizmoTool>) -> WidgetRegistry {
    let mut registry = WidgetRegistry::new();
    registry
        .register(GIZMO_KIND, tool.clone())
        .expect("the registry is empty");
    registry
}

/// A harness over a screen holding `children`, with `tool` registered.
fn harness(tool: &Arc<GizmoTool>, children: impl IntoIterator<Item = Widget>) -> Harness {
    let root = children.into_iter().fold(
        Widget::new(ROOT, WidgetKind::Panel).with_layout(Layout::Free),
        Widget::with_child,
    );
    Harness::new(Screen::new(root))
        .viewport(VP)
        .with_registry(registry(tool))
        .with_map(view(camera()))
}

/// What the harness's screen draws through its UI state in a frame whose map is
/// `map`, the way a host renders a frame: with the hover and each node's state
/// behind it, which the harness's own draw list is built without.
fn rendered_in(h: &Harness, tool: &Arc<GizmoTool>, map: &HostMap) -> Vec<DrawCommand> {
    let registry = registry(tool);
    let measure = ApproxMeasure::default();
    let frame = Frame::new(VP, &measure, &registry).with_host(HostSources::default().with_map(map));
    h.ui().render(h.screen(), &frame).commands.to_vec()
}

/// What the harness's screen draws through the default camera.
fn rendered(h: &Harness, tool: &Arc<GizmoTool>) -> Vec<DrawCommand> {
    rendered_in(h, tool, &view(camera()))
}

/// The pane `id` alone, covering the viewport.
fn one_pane(tool: &Arc<GizmoTool>, id: u32) -> Harness {
    harness(tool, [pane(id)])
}

/// The pane with a button laid over its X handle.
fn covered_pane(tool: &Arc<GizmoTool>) -> Harness {
    let at = x_handle();
    let cover = Widget::button(COVER, "Cover", "cover.pressed").placed(
        Alignment::TOP_LEFT,
        [at[0] - 20.0, at[1] - 20.0],
        [40.0, 40.0],
    );
    harness(tool, [pane(PANE), cover])
}

/// The result node `id` reported in `events`, if it reported one.
fn result_of(events: &[UiEvent], id: u32) -> Option<GizmoResult> {
    events.iter().find_map(|e| match e {
        UiEvent::CustomOutput {
            id: from, value, ..
        } if *from == id => {
            Some(serde_json::from_value(value.clone()).expect("the result deserialises"))
        }
        _ => None,
    })
}

fn moved(at: [f32; 2], dx: f32) -> [f32; 2] {
    [at[0] + dx, at[1]]
}

#[test]
fn hovering_a_handle_captures_the_pointer() {
    let tool = tool();
    let mut h = one_pane(&tool, PANE);

    h.feed(Input::at([5.0, 5.0]));
    assert!(!h.capture().pointer, "empty space yields to the camera");

    h.feed(Input::at(x_handle()));
    assert!(h.capture().pointer, "hovering a handle holds the pointer");
}

#[test]
fn the_pointer_reads_as_a_grab_over_a_handle() {
    let tool = tool();
    let mut h = one_pane(&tool, PANE);

    h.feed(Input::at([5.0, 5.0]));
    assert_ne!(
        h.ui().cursor(),
        Cursor::Grab,
        "empty viewport keeps its own shape"
    );

    h.feed(Input::at(x_handle()));
    assert_eq!(
        h.ui().cursor(),
        Cursor::Grab,
        "an open hand over the handle"
    );

    h.feed(Input::at(x_handle()).press());
    assert_eq!(
        h.ui().cursor(),
        Cursor::Grabbing,
        "and a closed one once it is taken"
    );
}

#[test]
fn chrome_over_a_handle_takes_the_press_off_the_gizmo() {
    let tool = tool();
    let mut h = covered_pane(&tool);

    h.feed(Input::at(x_handle()));
    assert_ne!(
        h.ui().cursor(),
        Cursor::Grab,
        "the control keeps its own shape"
    );
    let events = h.feed(Input::at(x_handle()).press());
    assert!(
        result_of(&events, PANE).is_none(),
        "no drag began under the chrome"
    );
    assert!(!tool.is_active());

    // Uncovered, the same point is the gizmo's again.
    let mut h = one_pane(&tool, PANE);
    h.feed(Input::at(x_handle()));
    assert!(
        h.capture().pointer,
        "the handle is live where nothing covers it"
    );
}

#[test]
fn dragging_a_handle_reports_a_typed_update() {
    let tool = tool();
    let mut h = one_pane(&tool, PANE);
    let start = x_handle();

    h.feed(Input::at(start));
    h.feed(Input::at(start).press());
    assert!(h.capture().pointer, "a live drag holds the pointer");

    let events = h.feed(Input::at(moved(start, 60.0)).hold());
    match result_of(&events, PANE).expect("a drag frame reports a result") {
        GizmoResult::Update(delta) => assert!(
            delta.translation.iter().any(|v| v.abs() > 1e-4),
            "the drag moved the object: {delta:?}"
        ),
        other => panic!("expected an update, got {other:?}"),
    }

    let events = h.feed(Input::at(moved(start, 60.0)).release());
    assert_eq!(result_of(&events, PANE), Some(GizmoResult::Commit));
    assert!(!tool.is_active());
}

#[test]
fn a_drag_carries_on_across_chrome() {
    let tool = tool();
    // The cover sits a little way along the arm, so the press lands on bare handle
    // and the drag then travels over the button.
    let start = moved(x_handle(), -60.0);
    let mut h = covered_pane(&tool);

    h.feed(Input::at(start));
    h.feed(Input::at(start).press());
    assert!(
        tool.is_active(),
        "the press on the bare handle began a drag"
    );

    let events = h.feed(Input::at(x_handle()).hold());
    assert!(h.capture().pointer, "a live drag keeps the pointer");
    assert!(
        result_of(&events, PANE).is_some(),
        "and keeps reporting its delta over the chrome"
    );
}

#[test]
fn a_typed_number_reaches_the_host_through_the_delta() {
    let tool = tool();
    let mut h = one_pane(&tool, PANE);
    let centre = [VP[0] / 2.0, VP[1] / 2.0];

    // Press G to begin a keyboard move.
    let mut begin = ActionFrame::default();
    begin
        .actions
        .insert(Action::BeginMove, ResolvedActionState::Pressed);
    h.set_context(framed(camera(), begin));
    h.feed(Input::at(centre));
    assert!(tool.is_active(), "G starts a session");

    // Type "5". It lands on the first axis and must reach the host, not just the
    // readout: a number typed into a session is how the value is set at all.
    let mut typed = ActionFrame::default();
    typed.typed_chars = vec!['5'];
    h.set_context(framed(camera(), typed));
    let events = h.feed(Input::at(centre));
    match result_of(&events, PANE).expect("the typed frame reports a result") {
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
fn a_lone_pane_drives() {
    let tool = tool();
    let mut h = one_pane(&tool, PANE);
    h.feed(Input::at([5.0, 5.0]));
    assert_eq!(tool.driving_view(), Some(PANE));
}

#[test]
fn panes_on_separate_screens_drive_from_the_hovered_one() {
    // Two views, each with its own screen and UI state, both panes of one tool.
    let tool = tool();
    let (mut a, mut b) = (one_pane(&tool, 1), one_pane(&tool, 2));

    a.feed(Input::at(x_handle()).without_pointer());
    b.feed(Input::at(x_handle()));
    assert_eq!(
        tool.driving_view(),
        Some(2),
        "the hovered view takes the driver"
    );
    assert!(!a.capture().pointer, "the other view yields");
    assert!(b.capture().pointer, "the hovered view takes the handle");
}

#[test]
fn a_drag_stays_with_the_view_it_started_in() {
    let tool = tool();
    let (mut a, mut b) = (one_pane(&tool, 1), one_pane(&tool, 2));
    let start = x_handle();

    // Begin the drag in A.
    a.feed(Input::at(start));
    a.feed(Input::at(start).press());
    assert!(tool.is_active());

    // The pointer crosses into B's pane with the button held. B does not take over;
    // A keeps reporting the drag.
    let held = Input::at(moved(start, 60.0)).hold();
    let from_b = b.feed(held.clone());
    let from_a = a.feed(held);
    assert_eq!(tool.driving_view(), Some(1));
    assert!(
        result_of(&from_b, 2).is_none(),
        "the other view stays quiet"
    );
    assert!(
        result_of(&from_a, 1).is_some(),
        "the starting view keeps the drag"
    );

    // Release ends the session; the hovered view drives again.
    a.feed(Input::at(moved(start, 60.0)).release());
    assert!(!tool.is_active());
    b.feed(Input::at([5.0, 5.0]));
    assert_eq!(tool.driving_view(), Some(2));
}

#[test]
fn every_pane_of_one_tool_draws_its_gizmo() {
    // Four views, as in the four-view example: one handler, four panes.
    let tool = tool();
    let mut views: Vec<Harness> = (1..=4).map(|id| one_pane(&tool, id)).collect();
    for view in &mut views {
        view.feed(Input::at([0.0, 0.0]).without_pointer());
        assert!(
            !view.draw().commands.is_empty(),
            "each pane paints the handles"
        );
    }
}

#[test]
fn two_views_with_two_cameras_draw_differently_from_one_tool() {
    let tool = tool();
    let mut side = camera();
    side.set_orbit_orientation(glam::Quat::from_rotation_y(0.9));
    let (mut a, mut b) = (one_pane(&tool, 1), one_pane(&tool, 2));
    b.set_context(framed(side.clone(), ActionFrame::default()));
    a.feed(Input::at([0.0, 0.0]).without_pointer());
    b.feed(Input::at([0.0, 0.0]).without_pointer());
    assert_ne!(
        a.draw().commands,
        b.draw().commands,
        "each view draws through its own camera"
    );
    assert_eq!(
        rendered_in(&a, &tool, &view(side.clone())),
        rendered_in(&b, &tool, &view(side)),
        "a pane's drawing follows the frame's camera, not the pane"
    );
}

#[test]
fn a_view_picks_through_its_own_camera() {
    // A view looking past the selection shows the gizmo off to one side, so the
    // point over the X arm from the default camera is empty space there.
    let tool = tool();
    let mut side = camera();
    side.set_orbit_center(glam::DVec3::new(3.0, 0.0, 0.0));
    let mut h = one_pane(&tool, PANE);
    h.set_context(framed(side, ActionFrame::default()));
    h.feed(Input::at(x_handle()));
    assert!(!h.capture().pointer, "the other view has no handle there");

    h.set_context(framed(camera(), ActionFrame::default()));
    h.feed(Input::at(x_handle()));
    assert!(h.capture().pointer, "the default view does");
}

#[test]
fn a_frame_with_no_gizmo_context_shows_and_takes_nothing() {
    let tool = tool();
    let mut h = one_pane(&tool, PANE);
    h.set_map(HostMap::new());
    h.feed(Input::at(x_handle()));
    assert!(!h.capture().pointer);
    assert!(rendered_in(&h, &tool, &HostMap::new()).is_empty());
}

#[test]
fn only_the_hovered_pane_lights_its_handle() {
    let tool = tool();
    let (mut a, mut b) = (one_pane(&tool, 1), one_pane(&tool, 2));
    a.feed(Input::at([0.0, 0.0]).without_pointer());
    b.feed(Input::at([0.0, 0.0]).without_pointer());
    let rest = rendered(&b, &tool);

    // Hover the handle in A, then leave A for B's empty space.
    a.feed(Input::at(x_handle()));
    assert_ne!(rendered(&a, &tool), rest, "the hovered handle draws lit");
    assert_eq!(rendered(&b, &tool), rest, "the other pane stays at rest");
    a.feed(Input::at([0.0, 0.0]).without_pointer());
    b.feed(Input::at([5.0, 5.0]));
    assert_eq!(
        rendered(&a, &tool),
        rest,
        "the pane the pointer left goes back to rest"
    );
}

#[test]
fn panes_share_the_mode() {
    let tool = tool();
    tool.set_mode(GizmoMode::Rotate);
    assert_eq!(tool.mode(), GizmoMode::Rotate);
    let mut h = one_pane(&tool, PANE);
    h.feed(Input::at([5.0, 5.0]));
    assert_eq!(
        tool.mode(),
        GizmoMode::Rotate,
        "every pane reads the tool's mode"
    );
}
