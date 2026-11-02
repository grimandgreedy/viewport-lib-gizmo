//! Translate drags through the gizmo handles: the grabbed point follows the
//! cursor, the constraint holds, and the session ends cleanly.

mod common;

use common::*;
use glam::{Vec2, Vec3};
use viewport_lib_gizmo::actions;
use viewport_lib_gizmo::{GizmoAxis, GizmoDriver, ManipResult, SnapConfig};
use viewport_lib_types::input::Action;

/// The point on the X arm a user would grab.
fn x_arm(rig: &Rig) -> Vec3 {
    rig.centre.unwrap() + Vec3::X * rig.scale() * 0.5
}

/// The centre of the XY plane handle.
fn xy_handle(rig: &Rig) -> Vec3 {
    rig.centre.unwrap() + (Vec3::X + Vec3::Y) * rig.scale() * 0.25
}

#[test]
fn pressing_the_x_arm_starts_a_drag_on_x() {
    let mut rig = Rig::new(three_quarter(), Vec3::ZERO);
    let grab = rig.px(x_arm(&rig));
    rig.hover(grab);
    assert_eq!(rig.driver.hovered_axis(), GizmoAxis::X);
    assert_eq!(rig.press(grab), ManipResult::None);
    let state = rig.driver.state().expect("a session");
    assert!(state.is_gizmo_drag);
    assert_eq!(state.axis, Some(GizmoAxis::X));
    assert!(!state.exclude_axis);
}

#[test]
fn the_grabbed_point_on_the_x_arm_stays_under_the_cursor() {
    let mut rig = Rig::new(three_quarter(), Vec3::ZERO);
    let grab = x_arm(&rig);
    rig.press(rig.px(grab));
    // Drag along the projected axis to where world x = 2.5 lands on screen.
    let target = Vec3::new(2.5, 0.0, 0.0);
    let d = update(rig.drag_path(rig.px(target), 12));
    assert_near(d.translation, target - grab, 0.01, "translation along X");
}

#[test]
fn a_y_constrained_move_follows_the_cursor_along_a_receding_axis() {
    // +Y runs away from the viewer, so a long move along it crosses a lot of
    // perspective foreshortening. Driven from the keyboard (G, then Y) so the
    // handles' pick order does not come into it.
    let mut rig = Rig::new(low(), Vec3::ZERO);
    rig.hover(rig.centre_px());
    rig.keys(&[actions::BEGIN_MOVE]);
    assert_eq!(
        rig.keys(&[actions::CONSTRAIN_Y]),
        ManipResult::ConstraintChanged
    );
    let target = Vec3::new(0.0, 6.0, 0.0);
    let d = update(rig.drag_path(rig.px(target), 20));
    assert_near(d.translation, target, 0.05, "translation along Y");
}

#[test]
fn dragging_across_the_x_arm_does_not_leave_the_axis() {
    let mut rig = Rig::new(three_quarter(), Vec3::ZERO);
    let grab = rig.px(x_arm(&rig));
    rig.press(grab);
    let d = update(rig.drag_path(grab + Vec2::new(40.0, 160.0), 8));
    assert_eq!(d.translation.y, 0.0);
    assert_eq!(d.translation.z, 0.0);
}

#[test]
fn the_xy_plane_handle_follows_the_cursor_on_the_plane() {
    let mut rig = Rig::new(three_quarter(), Vec3::ZERO);
    let grab = xy_handle(&rig);
    rig.hover(rig.px(grab));
    assert_eq!(rig.driver.hovered_axis(), GizmoAxis::XY);
    rig.press(rig.px(grab));
    let offset = Vec3::new(1.0, 1.5, 0.0);
    let d = update(rig.drag_path(rig.px(grab + offset), 10));
    assert_eq!(d.translation.z, 0.0, "the XY handle never leaves the plane");
    assert_near(d.translation, offset, 0.01, "translation in the XY plane");
}

#[test]
fn the_xy_plane_handle_follows_the_cursor_from_a_low_camera() {
    // Near grazing, a screen-plane move with the Z part dropped moves the object
    // much less than the cursor.
    let mut rig = Rig::new(low(), Vec3::ZERO);
    let grab = xy_handle(&rig);
    rig.hover(rig.px(grab));
    assert_eq!(rig.driver.hovered_axis(), GizmoAxis::XY);
    rig.press(rig.px(grab));
    let offset = Vec3::new(0.5, 2.0, 0.0);
    let d = update(rig.drag_path(rig.px(grab + offset), 10));
    assert_near(d.translation, offset, 0.02, "translation in the XY plane");
}

#[test]
fn dragging_a_plane_handle_past_the_horizon_stays_finite() {
    let mut rig = Rig::new(low(), Vec3::ZERO);
    let grab = rig.px(xy_handle(&rig));
    rig.press(grab);
    // Straight up the screen, well above where the XY plane meets the horizon.
    let mut last = None;
    for i in 1..=20 {
        let r = rig.drag_to(grab - Vec2::new(0.0, 20.0 * i as f32));
        let d = update(r);
        assert!(is_finite(&d), "frame {i}: {d:?}");
        assert!(
            d.translation.length() < 1.0e4,
            "frame {i}: the object ran off to {:?}",
            d.translation
        );
        last = Some(d);
    }
    assert_eq!(last.unwrap().translation.z, 0.0);
}

#[test]
fn the_screen_handle_follows_the_cursor_off_the_orbit_centre() {
    // The camera orbits the origin; the object sits well in front of it, so the
    // move must be scaled for the object's depth, not the orbit distance.
    let centre = Vec3::new(3.0, -4.0, 2.5);
    let mut rig = Rig::new(three_quarter(), centre);
    rig.hover(rig.centre_px());
    assert_eq!(rig.driver.hovered_axis(), GizmoAxis::Screen);
    rig.press(rig.centre_px());
    let offset = rig.camera.right() * 0.8 + rig.camera.up() * 0.4;
    let d = update(rig.drag_path(rig.px(centre + offset), 10));
    assert!(
        d.translation.dot(rig.camera.forward()).abs() < 1e-4,
        "a screen drag stays in the camera plane: {:?}",
        d.translation
    );
    assert_near(d.translation, offset, 0.01, "screen-plane translation");
}

#[test]
fn the_x_arm_follows_the_cursor_in_orthographic() {
    let mut rig = Rig::new(three_quarter().orthographic(), Vec3::new(1.0, 1.0, 0.0));
    let grab = x_arm(&rig);
    rig.hover(rig.px(grab));
    assert_eq!(rig.driver.hovered_axis(), GizmoAxis::X);
    rig.press(rig.px(grab));
    let target = grab + Vec3::X * 2.0;
    let d = update(rig.drag_path(rig.px(target), 6));
    assert_near(d.translation, Vec3::X * 2.0, 0.01, "orthographic X drag");
}

#[test]
fn releasing_commits_and_ends_the_session() {
    let mut rig = Rig::new(three_quarter(), Vec3::ZERO);
    let grab = rig.px(x_arm(&rig));
    rig.press(grab);
    rig.drag_path(grab + Vec2::new(50.0, 0.0), 4);
    assert_eq!(rig.release(), ManipResult::Commit);
    assert!(!rig.driver.is_active());
    assert_eq!(rig.idle(), ManipResult::None);
}

#[test]
fn escape_mid_drag_cancels_and_the_held_button_does_not_restart_it() {
    let mut rig = Rig::new(three_quarter(), Vec3::ZERO);
    let grab = rig.px(x_arm(&rig));
    rig.press(grab);
    rig.drag_path(grab + Vec2::new(50.0, 0.0), 4);
    assert_eq!(rig.keys(&[Action::Cancel]), ManipResult::Cancel);
    // The button is still down and the cursor keeps moving: nothing restarts
    // until a fresh press.
    for i in 1..=3 {
        let r = rig.drag_to(grab + Vec2::new(50.0 + 10.0 * i as f32, 0.0));
        assert_eq!(r, ManipResult::None);
    }
    assert_eq!(rig.release(), ManipResult::None);
}

#[test]
fn a_still_cursor_repeats_the_same_total() {
    let mut rig = Rig::new(three_quarter(), Vec3::ZERO);
    let grab = rig.px(x_arm(&rig));
    rig.press(grab);
    let a = update(rig.drag_to(grab + Vec2::new(60.0, 0.0)));
    let b = update(rig.idle());
    let c = update(rig.idle());
    assert_eq!(a.translation, b.translation);
    assert_eq!(b.translation, c.translation);
}

#[test]
fn bringing_the_cursor_back_returns_the_object_to_its_start() {
    let mut rig = Rig::new(three_quarter(), Vec3::ZERO);
    let grab = rig.px(xy_handle(&rig));
    rig.press(grab);
    rig.drag_path(grab + Vec2::new(180.0, -70.0), 17);
    rig.drag_path(grab + Vec2::new(-90.0, 40.0), 11);
    let d = update(rig.drag_path(grab, 13));
    assert_near(
        d.translation,
        Vec3::ZERO,
        1e-4,
        "no drift after a round trip",
    );
}

#[test]
fn the_cursor_leaving_the_viewport_mid_drag_does_not_jump_on_return() {
    let grab_and_target = |rig: &Rig| {
        let grab = rig.px(x_arm(rig));
        (grab, grab + Vec2::new(200.0, 0.0))
    };

    // Reference: a straight drag to the target.
    let mut straight = Rig::new(three_quarter(), Vec3::ZERO);
    let (grab, target) = grab_and_target(&straight);
    straight.press(grab);
    let expected = update(straight.drag_path(target, 4)).translation;

    // The same drag, but the cursor is reported outside for a few frames on the
    // way. The pointer deltas still describe the move.
    let mut rig = Rig::new(three_quarter(), Vec3::ZERO);
    rig.press(grab);
    let mid = grab.lerp(target, 0.25);
    rig.drag_to(mid);
    let step = (target - mid) / 4.0;
    for _ in 0..3 {
        rig.frame_with(&Default::default(), None, step, false, false);
    }
    let d = rig.frame_with(&Default::default(), Some(target), step, false, false);
    assert_near(
        update(d).translation,
        expected,
        0.01,
        "translation after the cursor came back",
    );
}

#[test]
fn pressing_off_the_gizmo_starts_nothing() {
    let mut rig = Rig::new(three_quarter(), Vec3::ZERO);
    let far = rig.centre_px() + Vec2::new(400.0, 300.0);
    assert_eq!(rig.press(far), ManipResult::None);
    assert!(!rig.driver.is_active());
    assert_eq!(
        rig.drag_path(far + Vec2::new(50.0, 0.0), 3),
        ManipResult::None
    );
}

#[test]
fn a_drag_needs_a_selection() {
    let mut rig = Rig::new(three_quarter(), Vec3::ZERO);
    let grab = rig.px(x_arm(&rig));
    rig.centre = None;
    assert_eq!(rig.press(grab), ManipResult::None);
    assert!(!rig.driver.is_active());
    assert_eq!(rig.driver.hovered_axis(), GizmoAxis::None);
}

#[test]
fn snapping_rounds_the_total_translation() {
    let driver = GizmoDriver::new().with_snap(SnapConfig::new().with_translation(0.5));
    let mut rig = Rig::with_driver(driver, three_quarter(), Vec3::ZERO);
    let grab = x_arm(&rig);
    rig.press(rig.px(grab));
    for k in 1..=8 {
        let target = grab + Vec3::X * (0.137 * k as f32);
        let d = update(rig.drag_to(rig.px(target)));
        let steps = d.translation.x / 0.5;
        assert!(
            (steps - steps.round()).abs() < 1e-4,
            "frame {k}: {} is not on the 0.5 grid",
            d.translation.x
        );
    }
}

#[test]
fn removing_the_selection_mid_drag_cancels_it() {
    let mut rig = Rig::new(three_quarter(), Vec3::ZERO);
    let grab = rig.px(x_arm(&rig));
    rig.press(grab);
    rig.drag_to(grab + Vec2::new(30.0, 0.0));
    rig.centre = None;
    assert_eq!(
        rig.drag_to(grab + Vec2::new(60.0, 0.0)),
        ManipResult::Cancel
    );
    assert!(!rig.driver.is_active());
    assert_eq!(rig.release(), ManipResult::None);
}

#[test]
fn a_camera_drag_crossing_the_gizmo_does_not_hover_it() {
    // A drag that starts in the scene is the camera's. Passing over a handle
    // must not light it, or the host hands the pointer to the gizmo mid-orbit.
    let mut rig = Rig::new(three_quarter(), Vec3::ZERO);
    let handle = rig.px(x_arm(&rig));
    let scene = handle + Vec2::new(0.0, 250.0);
    assert_eq!(rig.press(scene), ManipResult::None);
    assert!(!rig.driver.is_active());
    rig.drag_path(handle, 6);
    assert_eq!(rig.driver.hovered_axis(), GizmoAxis::None);
    assert_eq!(rig.driver.highlight(true), GizmoAxis::None);
    // Let go over the handle: from the next frame it hovers as usual.
    rig.release();
    assert_eq!(rig.driver.hovered_axis(), GizmoAxis::X);
}

#[test]
fn a_button_held_from_before_the_driver_saw_it_does_not_hover() {
    let mut rig = Rig::new(three_quarter(), Vec3::ZERO);
    rig.button_down = true;
    rig.drag_to(rig.px(x_arm(&rig)));
    assert_eq!(rig.driver.hovered_axis(), GizmoAxis::None);
    assert!(!rig.driver.is_active());
}
