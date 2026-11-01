//! The overlay the driver builds for each view, the gizmo's on-screen size, and
//! inputs at the edges: degenerate cameras, empty viewports, and selections
//! behind the eye.

mod common;

use common::*;
use glam::{DVec3, Vec2, Vec3};
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
    rig.keys(&[Action::BeginMove]);
    rig.keys(&[Action::ConstrainZ]);
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
        selection_center: Some(Vec3::ZERO),
        drag_started: started,
        dragging,
        clicked: false,
    };
    driver.update(input(&af, Some(Vec2::ZERO), false, false));
    assert!(driver.gizmo_scale().is_finite(), "{}", driver.gizmo_scale());
    let g = pressed(&[Action::BeginMove]);
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
    rig.keys(&[Action::BeginMove]);
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
    assert_eq!(info.center, Vec3::new(1.0, 2.0, 0.5));
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
