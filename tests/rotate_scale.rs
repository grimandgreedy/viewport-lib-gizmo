//! Rotate and scale drags through the gizmo handles.

mod common;

use common::*;
use glam::{Quat, Vec2, Vec3};
use viewport_lib_gizmo::actions;
use viewport_lib_gizmo::{
    GizmoAxis, GizmoDriver, GizmoMode, ManipResult, ROTATION_RING_RADIUS, SnapConfig,
};

fn rig_in(camera: viewport_lib_types::camera::Camera, mode: GizmoMode) -> Rig {
    let mut driver = GizmoDriver::new();
    driver.set_mode(mode);
    Rig::with_driver(driver, camera, Vec3::ZERO)
}

/// A point on the Z ring (the circle in the XY plane) at `deg` from +X.
fn z_ring(rig: &Rig, deg: f32) -> Vec3 {
    let a = deg.to_radians();
    Vec3::new(a.cos(), a.sin(), 0.0) * rig.scale() * ROTATION_RING_RADIUS
}

/// The signed angle `q` turns +X through, about +Z.
fn angle_about_z(q: Quat) -> f32 {
    let v = q * Vec3::X;
    v.y.atan2(v.x).to_degrees()
}

/// Sweep the cursor around the Z ring from `from` to `to` degrees in 5 degree
/// steps, starting with a press on the ring.
fn sweep_z_ring(rig: &mut Rig, from: f32, to: f32) -> ManipResult {
    rig.hover(rig.px(z_ring(rig, from)));
    assert_eq!(rig.driver.hovered_axis(), GizmoAxis::Z);
    rig.press(rig.px(z_ring(rig, from)));
    let steps = ((to - from).abs() / 5.0).round() as usize;
    let mut last = ManipResult::None;
    for i in 1..=steps {
        let deg = from + (to - from) * i as f32 / steps as f32;
        last = rig.drag_to(rig.px(z_ring(rig, deg)));
    }
    last
}

#[test]
fn sweeping_the_z_ring_counter_clockwise_from_above_turns_positive_about_z() {
    let mut rig = rig_in(top(), GizmoMode::Rotate);
    let d = update(sweep_z_ring(&mut rig, 45.0, 135.0));
    let axis = d.rotation.to_axis_angle().0;
    assert!(
        axis.cross(Vec3::Z).length() < 1e-3,
        "rotation stays about Z: {axis:?}"
    );
    assert!(
        (angle_about_z(d.rotation) - 90.0).abs() < 0.5,
        "a 90 degree sweep turns 90 degrees, got {}",
        angle_about_z(d.rotation)
    );
}

#[test]
fn sweeping_the_z_ring_clockwise_turns_negative() {
    let mut rig = rig_in(top(), GizmoMode::Rotate);
    let d = update(sweep_z_ring(&mut rig, 45.0, -15.0));
    assert!((angle_about_z(d.rotation) + 60.0).abs() < 0.5);
}

#[test]
fn a_full_turn_of_the_z_ring_comes_back_to_identity() {
    let mut rig = rig_in(three_quarter(), GizmoMode::Rotate);
    let d = update(sweep_z_ring(&mut rig, 45.0, 405.0));
    assert!(
        d.rotation.dot(Quat::IDENTITY).abs() > 0.9999,
        "a full turn ends where it began: {:?}",
        d.rotation
    );
}

#[test]
fn rotation_snapping_steps_the_total_angle() {
    let mut driver =
        GizmoDriver::new().with_snap(SnapConfig::new().with_rotation(15f32.to_radians()));
    driver.set_mode(GizmoMode::Rotate);
    let mut rig = Rig::with_driver(driver, top(), Vec3::ZERO);
    let d = update(sweep_z_ring(&mut rig, 45.0, 82.0));
    let deg = angle_about_z(d.rotation);
    assert!(
        (deg - 30.0).abs() < 1e-2,
        "37 degrees snaps to 30, got {deg}"
    );
}

#[test]
fn r_then_x_turns_about_x_with_the_cursor_sweep() {
    // In this view +X points toward the camera, so a counter-clockwise sweep on
    // screen is a positive turn about X.
    let mut rig = Rig::new(three_quarter(), Vec3::ZERO);
    let c = rig.centre_px();
    let at = |deg: f32| {
        let a = deg.to_radians();
        c + Vec2::new(a.cos(), -a.sin()) * 150.0
    };
    rig.hover(at(0.0));
    rig.keys(&[actions::BEGIN_ROTATE]);
    rig.keys(&[actions::CONSTRAIN_X]);
    let mut last = ManipResult::None;
    for deg in (5..=60).step_by(5) {
        last = rig.hover(at(deg as f32));
    }
    let (axis, angle) = update(last).rotation.to_axis_angle();
    let signed = angle.to_degrees() * axis.dot(Vec3::X).signum();
    assert!(axis.cross(Vec3::X).length() < 1e-3, "about X: {axis:?}");
    assert!(
        (signed - 60.0).abs() < 0.5,
        "turns +60 degrees, got {signed}"
    );
}

#[test]
fn rotate_release_commits() {
    let mut rig = rig_in(top(), GizmoMode::Rotate);
    sweep_z_ring(&mut rig, 45.0, 90.0);
    assert_eq!(rig.release(), ManipResult::Commit);
    assert!(!rig.driver.is_active());
}

#[test]
fn dragging_the_x_scale_handle_to_twice_the_distance_doubles_x() {
    let mut rig = rig_in(three_quarter(), GizmoMode::Scale);
    let tip = rig.px(Vec3::X * rig.scale() * 0.9);
    rig.hover(tip);
    assert_eq!(rig.driver.hovered_axis(), GizmoAxis::X);
    rig.press(tip);
    let c = rig.centre_px();
    let d = update(rig.drag_path(c + (tip - c) * 2.0, 10));
    assert_near(d.scale, Vec3::new(2.0, 1.0, 1.0), 1e-3, "x doubles");
}

#[test]
fn scaling_through_the_centre_bottoms_out_above_zero() {
    let mut rig = rig_in(three_quarter(), GizmoMode::Scale);
    let tip = rig.px(Vec3::X * rig.scale() * 0.9);
    rig.press(tip);
    let c = rig.centre_px();
    let d = update(rig.drag_path(c, 10));
    assert!(d.scale.x > 0.0, "scale stays positive: {}", d.scale.x);
    assert!(d.scale.x < 0.01);
}

#[test]
fn the_centre_handle_scales_uniformly_when_grabbed_dead_centre() {
    let mut rig = rig_in(three_quarter(), GizmoMode::Scale);
    let c = rig.centre_px();
    rig.hover(c);
    assert_eq!(rig.driver.hovered_axis(), GizmoAxis::Screen);
    rig.press(c);
    let d = update(rig.drag_path(c + Vec2::new(120.0, 0.0), 10));
    assert!(d.scale.x > 1.5, "dragging out grows: {:?}", d.scale);
    assert_eq!(d.scale.x, d.scale.y);
    assert_eq!(d.scale.y, d.scale.z);
}

#[test]
fn the_centre_handle_grabbed_just_off_centre_does_not_explode() {
    let mut rig = rig_in(three_quarter(), GizmoMode::Scale);
    let c = rig.centre_px();
    rig.press(c + Vec2::new(3.0, 0.0));
    let d = update(rig.drag_path(c + Vec2::new(120.0, 0.0), 10));
    assert!(
        d.scale.x < 10.0,
        "a 120 px drag should not scale by {}",
        d.scale.x
    );
}

#[test]
fn scale_mode_does_not_pick_the_plane_handles_it_does_not_draw() {
    // The overlay draws no plane handles in scale mode, so the spot where the
    // XY handle sits in translate mode is empty there.
    let mut rig = rig_in(three_quarter(), GizmoMode::Scale);
    let spot = rig.px((Vec3::X + Vec3::Y) * rig.scale() * 0.25);
    rig.hover(spot);
    assert_ne!(rig.driver.hovered_axis(), GizmoAxis::XY);
}

#[test]
fn scale_snapping_steps_the_total_factor() {
    let mut driver = GizmoDriver::new().with_snap(SnapConfig::new().with_scale(0.25));
    driver.set_mode(GizmoMode::Scale);
    let mut rig = Rig::with_driver(driver, three_quarter(), Vec3::ZERO);
    let tip = rig.px(Vec3::X * rig.scale() * 0.9);
    rig.press(tip);
    let c = rig.centre_px();
    let d = update(rig.drag_path(c + (tip - c) * 1.6, 6));
    assert!(
        (d.scale.x - 1.5).abs() < 1e-4,
        "1.6 snaps to 1.5, got {}",
        d.scale.x
    );
}

#[test]
fn a_keyboard_scale_session_started_without_a_cursor_scales_once_the_cursor_moves() {
    // S is pressed while the cursor is outside the viewport, so the session has
    // no anchor. Once the cursor is over the view, moving away from the centre
    // should grow the object.
    let mut rig = Rig::new(three_quarter(), Vec3::ZERO);
    rig.cursor = None;
    rig.frame_with(
        &pressed(&[actions::BEGIN_SCALE]),
        None,
        Vec2::ZERO,
        false,
        false,
    );
    assert!(rig.driver.is_active());
    let c = rig.centre_px();
    rig.hover(c + Vec2::new(100.0, 0.0));
    let d = update(rig.hover(c + Vec2::new(200.0, 0.0)));
    assert!(
        (d.scale.x - 2.0).abs() < 1e-3,
        "twice as far, twice the size: {:?}",
        d.scale
    );
}
