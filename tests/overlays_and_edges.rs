//! The overlay the driver builds for each view, the gizmo's on-screen size, and
//! inputs at the edges: degenerate cameras, empty viewports, and selections
//! behind the eye.

mod common;

use common::*;
use glam::{DVec3, Vec2, Vec3};
use viewport_lib_gizmo::actions;
use viewport_lib_gizmo::{
    ActiveView, FrameInput, GizmoAxis, GizmoDriver, GizmoMode, GizmoStyle, ManipResult,
};
use viewport_lib_types::camera::Camera;
use viewport_lib_types::input::{Action, ActionFrame};

fn overlays(rig: &Rig, camera: &Camera, driving: bool) -> (usize, usize) {
    let (mut shapes, mut lines) = (Vec::new(), Vec::new());
    rig.driver.build_view_overlays(
        &GizmoStyle::default(),
        camera,
        VP,
        driving,
        &mut shapes,
        &mut lines,
    );
    (shapes.len(), lines.len())
}

/// How many pixels the X arm spans on screen.
fn arm_px(rig: &Rig) -> f32 {
    let c = rig.centre.unwrap();
    (rig.px(c + Vec3::X * rig.scale()) - rig.px(c)).length()
}

#[test]
fn no_selection_builds_no_overlay() {
    let mut rig = Rig::new(three_quarter(), Vec3::ZERO);
    rig.centre = None;
    rig.idle();
    let camera = rig.camera.clone();
    assert_eq!(overlays(&rig, &camera, true), (0, 0));
}

#[test]
fn every_view_builds_an_overlay_from_one_driver() {
    let rig = Rig::new(three_quarter(), Vec3::ZERO);
    for camera in [three_quarter(), top(), top().orthographic(), low()] {
        let (shapes, _) = overlays(&rig, &camera, false);
        assert!(shapes > 0, "a view of the selection draws the gizmo");
    }
}

#[test]
fn the_gizmo_keeps_its_screen_size_at_any_distance() {
    let near = Rig::new(three_quarter(), Vec3::ZERO);
    let far = Rig::new(three_quarter(), Vec3::new(-12.0, 16.0, -10.0));
    let (a, b) = (arm_px(&near), arm_px(&far));
    assert!((a - b).abs() / a < 0.05, "near {a} px, far {b} px");
}

#[test]
fn an_orthographic_gizmo_keeps_its_screen_size_at_any_depth() {
    // In orthographic, depth does not change on-screen size, so neither should
    // the object's distance from the eye.
    let camera = three_quarter().orthographic();
    let at_focus = Rig::new(camera.clone(), Vec3::ZERO);
    let behind_focus = Rig::new(camera, Vec3::new(-6.0, 8.0, -5.0));
    let (a, b) = (arm_px(&at_focus), arm_px(&behind_focus));
    assert!((a - b).abs() / a < 0.05, "at focus {a} px, deeper {b} px");
}

#[test]
fn hovering_works_in_an_orthographic_top_view() {
    let mut rig = Rig::new(top().orthographic(), Vec3::new(2.0, 1.0, 0.0));
    rig.hover(rig.px(rig.centre.unwrap() + Vec3::Y * rig.scale() * 0.5));
    assert_eq!(rig.driver.hovered_axis(), GizmoAxis::Y);
}

#[test]
fn the_z_arm_seen_end_on_cannot_be_dragged_into_infinity() {
    // From straight above, Z points at the camera and projects to a point.
    let mut rig = Rig::new(top(), Vec3::ZERO);
    rig.hover(rig.centre_px() + Vec2::new(200.0, 0.0));
    rig.keys(&[actions::BEGIN_MOVE]);
    rig.keys(&[actions::CONSTRAIN_Z]);
    for i in 1..=5 {
        let d = update(rig.hover(rig.centre_px() + Vec2::new(200.0 + 30.0 * i as f32, 10.0)));
        assert!(is_finite(&d), "{d:?}");
        assert!(
            d.translation.length() < 100.0,
            "ran off to {:?}",
            d.translation
        );
    }
}

#[test]
fn a_zero_sized_viewport_does_not_produce_nan() {
    let camera = three_quarter();
    let mut driver = GizmoDriver::new();
    let af = ActionFrame::default();
    let input = |af, cursor, started, dragging| FrameInput {
        action_frame: af,
        camera: &camera,
        viewport_size: Vec2::ZERO,
        cursor_viewport: cursor,
        pointer_delta: Vec2::new(3.0, 1.0),
        selection_centre: Some(Vec3::ZERO),
        selection_orientation: glam::Quat::IDENTITY,
        drag_started: started,
        dragging,
        clicked: false,
    };
    driver.update(input(&af, Some(Vec2::ZERO), false, false));
    assert!(driver.gizmo_scale().is_finite(), "{}", driver.gizmo_scale());
    let g = pressed(&[actions::BEGIN_MOVE]);
    driver.update(input(&g, Some(Vec2::ZERO), false, false));
    for _ in 0..3 {
        if let ManipResult::Update(d) = driver.update(input(&af, Some(Vec2::ONE), false, false)) {
            assert!(is_finite(&d), "{d:?}");
        }
    }
}

#[test]
fn a_selection_behind_the_camera_builds_no_overlay_and_moves_finitely() {
    let camera = three_quarter();
    let behind = camera.eye().as_vec3() - camera.forward() * 3.0;
    let mut rig = Rig::new(camera.clone(), behind);
    assert_eq!(overlays(&rig, &camera, true).0, 0);
    rig.hover(Vec2::new(600.0, 400.0));
    rig.keys(&[actions::BEGIN_MOVE]);
    let d = update(rig.hover(Vec2::new(650.0, 380.0)));
    assert!(is_finite(&d), "{d:?}");
}

#[test]
fn a_camera_moving_mid_drag_does_not_break_the_session() {
    let mut rig = Rig::new(three_quarter(), Vec3::ZERO);
    let grab = rig.px(Vec3::X * rig.scale() * 0.5);
    rig.press(grab);
    rig.drag_to(grab + Vec2::new(40.0, 0.0));
    rig.camera =
        Camera::look_at(DVec3::new(7.0, -7.0, 6.0), DVec3::ZERO, Vec3::Z).with_aspect(VP.x / VP.y);
    let d = update(rig.drag_to(grab + Vec2::new(80.0, 0.0)));
    assert!(is_finite(&d));
    assert_eq!(d.translation.y, 0.0);
    assert_eq!(rig.release(), ManipResult::Commit);
}

#[test]
fn a_drag_started_in_one_view_keeps_that_view_driving() {
    // Two views of one scene. The drag starts in view 0; the cursor then moves
    // over view 1, but view 0's camera keeps driving until release.
    let cameras = [three_quarter(), top()];
    let mut rig = Rig::new(cameras[0].clone(), Vec3::ZERO);
    let mut views = ActiveView::new();
    let grab = rig.px(Vec3::X * rig.scale() * 0.5);

    let driving = views.current(Some(0)).unwrap();
    rig.camera = cameras[driving].clone();
    rig.press(grab);
    views.end_frame(Some(driving), rig.driver.is_active());

    let driving = views.current(Some(1)).unwrap();
    assert_eq!(driving, 0, "view 0 keeps the drag");
    rig.camera = cameras[driving].clone();
    update(rig.drag_to(grab + Vec2::new(50.0, 0.0)));
    views.end_frame(Some(driving), rig.driver.is_active());

    assert_eq!(rig.release(), ManipResult::Commit);
    views.end_frame(Some(0), rig.driver.is_active());
    assert_eq!(views.current(Some(1)), Some(1));
}

#[test]
fn gizmo_info_reports_the_resolved_centre_scale_and_mode() {
    let mut driver = GizmoDriver::new();
    driver.set_mode(GizmoMode::Scale);
    let rig = Rig::with_driver(driver, three_quarter(), Vec3::new(1.0, 2.0, 0.5));
    let info = rig.driver.gizmo_info().expect("a selection");
    assert_eq!(info.centre, Vec3::new(1.0, 2.0, 0.5));
    assert_eq!(info.scale, rig.scale());
    assert_eq!(info.mode, GizmoMode::Scale);
}

#[test]
fn cycle_mode_wraps_through_all_three() {
    let mut d = GizmoDriver::new();
    let seen: Vec<_> = (0..4)
        .map(|_| {
            d.cycle_mode();
            d.mode()
        })
        .collect();
    assert_eq!(
        seen,
        [
            GizmoMode::Rotate,
            GizmoMode::Scale,
            GizmoMode::Translate,
            GizmoMode::Rotate
        ]
    );
}

#[test]
fn from_action_reads_the_pointer_frame() {
    let camera = three_quarter();
    let mut af = ActionFrame::default();
    af.pointer.cursor = Some(Vec2::new(10.0, 20.0));
    af.pointer.delta = Vec2::new(3.0, -1.0);
    af.pointer.pressed = true;
    af.pointer.dragging = true;
    let input = FrameInput::from_action(&af, &camera, VP, Some(Vec3::X));
    assert_eq!(input.cursor_viewport, Some(Vec2::new(10.0, 20.0)));
    assert_eq!(input.pointer_delta, Vec2::new(3.0, -1.0));
    assert!(input.drag_started && input.dragging && !input.clicked);
    assert_eq!(input.selection_centre, Some(Vec3::X));
    assert_eq!(input.selection_orientation, glam::Quat::IDENTITY);
}

#[test]
fn from_action_takes_a_tap_as_a_click_at_the_touch() {
    let camera = three_quarter();
    let mut af = ActionFrame::default();
    af.pointer.tapped = true;
    af.pointer.touch_position = Some(Vec2::new(5.0, 6.0));
    let input = FrameInput::from_action(&af, &camera, VP, None);
    assert!(input.clicked);
    assert_eq!(input.cursor_viewport, Some(Vec2::new(5.0, 6.0)));
}

#[test]
fn a_drag_driven_through_from_action_moves_the_object() {
    // The same handle drag the rig runs, with every frame built by from_action.
    let camera = three_quarter();
    let mut driver = GizmoDriver::new();
    let frame = |cursor: Vec2, pressed: bool, held: bool| {
        let mut af = ActionFrame::default();
        af.pointer.cursor = Some(cursor);
        af.pointer.pressed = pressed;
        af.pointer.dragging = held;
        af
    };
    let idle = frame(Vec2::ZERO, false, false);
    driver.update(FrameInput::from_action(
        &idle,
        &camera,
        VP,
        Some(Vec3::ZERO),
    ));
    let grab = to_px(&camera, Vec3::X * driver.gizmo_scale() * 0.5);
    for af in [frame(grab, false, false), frame(grab, true, true)] {
        driver.update(FrameInput::from_action(&af, &camera, VP, Some(Vec3::ZERO)));
    }
    let moved = frame(grab + Vec2::new(60.0, 0.0), false, true);
    let r = driver.update(FrameInput::from_action(
        &moved,
        &camera,
        VP,
        Some(Vec3::ZERO),
    ));
    assert!(update(r).translation.x.abs() > 0.0);
    let up = frame(grab + Vec2::new(60.0, 0.0), false, false);
    let r = driver.update(FrameInput::from_action(&up, &camera, VP, Some(Vec3::ZERO)));
    assert_eq!(r, ManipResult::Commit);
}

#[test]
fn the_driver_cycles_the_pivot_and_holds_it_during_a_session() {
    use viewport_lib_gizmo::PivotMode;
    let mut rig = Rig::new(three_quarter(), Vec3::ZERO);
    assert_eq!(rig.driver.pivot_mode(), PivotMode::SelectionCentroid);
    rig.driver.cycle_pivot_forward();
    assert_eq!(rig.driver.pivot_mode(), PivotMode::IndividualOrigins);
    rig.driver.cycle_pivot_backward();
    rig.driver.cycle_pivot_backward();
    assert_eq!(rig.driver.pivot_mode(), PivotMode::WorldOrigin);

    rig.hover(rig.centre_px() + Vec2::new(200.0, 0.0));
    rig.keys(&[actions::BEGIN_MOVE]);
    rig.driver.cycle_pivot_forward();
    rig.driver.set_pivot_mode(PivotMode::MedianPoint);
    assert_eq!(rig.driver.pivot_mode(), PivotMode::WorldOrigin);
    rig.keys(&[Action::Cancel]);
    rig.driver.set_pivot_mode(PivotMode::MedianPoint);
    assert_eq!(rig.driver.pivot_mode(), PivotMode::MedianPoint);
}
