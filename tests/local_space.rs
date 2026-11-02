//! Local space: the handles are drawn, picked and dragged along the
//! selection's own axes.

mod common;

use common::*;
use glam::{Quat, Vec2, Vec3};
use viewport_lib_gizmo::actions;
use viewport_lib_gizmo::{GizmoAxis, GizmoDriver, GizmoMode, GizmoSpace, ROTATION_RING_RADIUS};

/// Turned 40 degrees about Z, then tipped 25 degrees about the new X.
fn turned() -> Quat {
    Quat::from_rotation_z(40f32.to_radians()) * Quat::from_rotation_x(25f32.to_radians())
}

/// Turned 40 degrees about Z only. Seen from the top camera no handle overlaps
/// another, so a pick lands on the one aimed at.
fn spun() -> Quat {
    Quat::from_rotation_z(40f32.to_radians())
}

fn rig_with(camera: viewport_lib_types::camera::Camera, orientation: Quat, mode: GizmoMode) -> Rig {
    let mut driver = GizmoDriver::new();
    driver.set_space(GizmoSpace::Local);
    driver.set_mode(mode);
    let mut rig = Rig::with_driver(driver, camera, Vec3::ZERO);
    rig.orientation = orientation;
    rig.idle();
    rig
}

fn local_rig(mode: GizmoMode) -> Rig {
    rig_with(three_quarter(), turned(), mode)
}

#[test]
fn local_space_reports_the_selection_orientation() {
    let rig = local_rig(GizmoMode::Translate);
    assert_eq!(rig.driver.space_orientation(), turned());
    assert_eq!(rig.driver.gizmo_info().unwrap().orientation, turned());
}

#[test]
fn world_space_ignores_the_selection_orientation() {
    let mut rig = Rig::new(three_quarter(), Vec3::ZERO);
    rig.orientation = turned();
    rig.hover(rig.px(Vec3::X * rig.scale() * 0.6));
    assert_eq!(rig.driver.space_orientation(), Quat::IDENTITY);
    assert_eq!(rig.driver.hovered_axis(), GizmoAxis::X);
}

#[test]
fn dragging_the_local_x_arm_moves_along_local_x_under_the_cursor() {
    let mut rig = local_rig(GizmoMode::Translate);
    let local_x = turned() * Vec3::X;
    let grab = local_x * rig.scale() * 0.6;
    rig.press(rig.px(grab));
    assert_eq!(rig.driver.state().unwrap().axis, Some(GizmoAxis::X));
    let target = local_x * 2.5;
    let d = update(rig.drag_path(rig.px(target), 10));
    assert_near(d.translation, target - grab, 0.01, "along local X");
}

#[test]
fn g_then_x_in_local_space_moves_along_local_x() {
    let mut rig = local_rig(GizmoMode::Translate);
    rig.hover(rig.centre_px() + Vec2::new(200.0, 100.0));
    rig.keys(&[actions::BEGIN_MOVE]);
    rig.keys(&[actions::CONSTRAIN_X]);
    let d = update(rig.hover(rig.cursor.unwrap() + Vec2::new(90.0, -30.0)));
    let local_x = turned() * Vec3::X;
    assert!(d.translation.length() > 0.05);
    assert!(
        d.translation.normalize().cross(local_x).length() < 1e-4,
        "parallel to local X: {:?}",
        d.translation
    );
}

#[test]
fn local_space_draws_a_different_overlay_from_world_space() {
    let local = local_rig(GizmoMode::Translate);
    let mut world = Rig::new(three_quarter(), Vec3::ZERO);
    world.orientation = turned();
    world.idle();
    let build = |rig: &Rig| {
        let (mut shapes, mut lines) = (Vec::new(), Vec::new());
        rig.driver.build_view_overlays(
            &Default::default(),
            &rig.camera,
            VP,
            true,
            &mut shapes,
            &mut lines,
        );
        format!("{shapes:?}")
    };
    assert_ne!(build(&local), build(&world));
}

#[test]
fn the_local_x_arm_is_picked_where_it_is_drawn() {
    let mut rig = rig_with(top(), spun(), GizmoMode::Translate);
    rig.hover(rig.px(spun() * Vec3::X * rig.scale() * 0.6));
    assert_eq!(rig.driver.hovered_axis(), GizmoAxis::X);
    // Where the world X arm would be is empty.
    rig.hover(rig.px(Vec3::X * rig.scale() * 0.6));
    assert_eq!(rig.driver.hovered_axis(), GizmoAxis::None);
}

#[test]
fn the_local_xy_handle_is_picked_where_it_is_drawn() {
    let mut rig = rig_with(top(), spun(), GizmoMode::Translate);
    rig.hover(rig.px(spun() * (Vec3::X + Vec3::Y) * rig.scale() * 0.25));
    assert_eq!(rig.driver.hovered_axis(), GizmoAxis::XY);
}

#[test]
fn shift_z_in_local_space_stays_in_the_local_plane() {
    let mut rig = local_rig(GizmoMode::Translate);
    rig.hover(rig.centre_px() + Vec2::new(150.0, 80.0));
    rig.keys(&[actions::BEGIN_MOVE]);
    rig.keys(&[actions::EXCLUDE_Z]);
    let lz = turned() * Vec3::Z;
    for step in [
        Vec2::new(60.0, 0.0),
        Vec2::new(0.0, -70.0),
        Vec2::new(-30.0, 40.0),
    ] {
        let d = update(rig.hover(rig.cursor.unwrap() + step));
        assert!(d.translation.length() > 0.01);
        assert!(
            d.translation.dot(lz).abs() < 1e-4,
            "off the local plane: {:?}",
            d.translation
        );
    }
}

#[test]
fn the_local_z_ring_turns_about_local_z() {
    // Tipped so local Z is not world Z, seen from above.
    let tipped = Quat::from_rotation_x(30f32.to_radians());
    let mut rig = rig_with(top(), tipped, GizmoMode::Rotate);
    let (lx, ly, lz) = (tipped * Vec3::X, tipped * Vec3::Y, tipped * Vec3::Z);
    let radius = rig.scale() * ROTATION_RING_RADIUS;
    let ring = |deg: f32| {
        let a = deg.to_radians();
        (lx * a.cos() + ly * a.sin()) * radius
    };
    rig.hover(rig.px(ring(45.0)));
    assert_eq!(rig.driver.hovered_axis(), GizmoAxis::Z);
    rig.press(rig.px(ring(45.0)));
    let mut last = None;
    for deg in (50..=120).step_by(5) {
        last = Some(rig.drag_to(rig.px(ring(deg as f32))));
    }
    let axis = update(last.unwrap()).rotation.to_axis_angle().0;
    assert!(axis.cross(lz).length() < 1e-3, "about local Z: {axis:?}");
}
