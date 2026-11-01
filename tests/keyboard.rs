//! Keyboard sessions: G / R / S, axis constraints, typed values, confirm and
//! cancel.

mod common;

use common::*;
use glam::{Vec2, Vec3};
use viewport_lib_gizmo::{GizmoAxis, GizmoMode, ManipResult, ManipulationKind, ValueMode};
use viewport_lib_types::input::{Action, ActionFrame};

/// A rig with the cursor parked off the gizmo and a G session started.
fn moving() -> Rig {
    let mut rig = Rig::new(three_quarter(), Vec3::ZERO);
    rig.hover(rig.centre_px() + Vec2::new(250.0, 120.0));
    assert_eq!(rig.keys(&[Action::BeginMove]), ManipResult::None);
    assert!(rig.driver.is_active());
    rig
}

#[test]
fn g_moves_in_the_camera_plane_with_the_cursor() {
    let mut rig = moving();
    let from = rig.cursor.unwrap();
    let d = update(rig.hover(from + Vec2::new(80.0, -40.0)));
    assert!(d.translation.length() > 0.1, "the object moved: {d:?}");
    assert!(d.translation.dot(rig.camera.forward()).abs() < 1e-4);
}

#[test]
fn g_needs_a_selection() {
    let mut rig = Rig::new(three_quarter(), Vec3::ZERO);
    rig.centre = None;
    assert_eq!(rig.keys(&[Action::BeginMove]), ManipResult::None);
    assert!(!rig.driver.is_active());
}

#[test]
fn a_click_confirms_a_keyboard_session() {
    let mut rig = moving();
    rig.hover(rig.cursor.unwrap() + Vec2::new(30.0, 0.0));
    assert_eq!(rig.click(), ManipResult::Commit);
    assert!(!rig.driver.is_active());
}

#[test]
fn enter_confirms_and_escape_cancels() {
    let mut rig = moving();
    assert_eq!(rig.keys(&[Action::Confirm]), ManipResult::Commit);
    let mut rig = moving();
    assert_eq!(rig.keys(&[Action::Cancel]), ManipResult::Cancel);
    assert!(!rig.driver.is_active());
}

#[test]
fn a_click_does_not_end_a_gizmo_drag() {
    let mut rig = Rig::new(three_quarter(), Vec3::ZERO);
    let grab = rig.px(Vec3::X * rig.scale() * 0.5);
    rig.press(grab);
    let r = rig.click();
    assert!(matches!(r, ManipResult::Update(_)), "got {r:?}");
    assert!(rig.driver.is_active());
}

#[test]
fn changing_the_constraint_restores_and_restarts_from_the_cursor() {
    let mut rig = moving();
    let from = rig.cursor.unwrap();
    update(rig.hover(from + Vec2::new(90.0, 0.0)));
    assert_eq!(
        rig.keys(&[Action::ConstrainX]),
        ManipResult::ConstraintChanged
    );
    // The new constraint starts from where the cursor is now.
    let d = update(rig.idle());
    assert_eq!(d.translation, Vec3::ZERO);
    let d = update(rig.hover(rig.cursor.unwrap() + Vec2::new(60.0, 30.0)));
    assert!(d.translation.x.abs() > 0.0);
    assert_eq!(d.translation.y, 0.0);
    assert_eq!(d.translation.z, 0.0);
}

#[test]
fn pressing_the_same_constraint_again_is_not_a_change() {
    let mut rig = moving();
    assert_eq!(
        rig.keys(&[Action::ConstrainX]),
        ManipResult::ConstraintChanged
    );
    let r = rig.keys(&[Action::ConstrainX]);
    assert!(matches!(r, ManipResult::Update(_)), "got {r:?}");
}

#[test]
fn shift_z_moves_only_in_the_xy_plane() {
    let mut rig = moving();
    rig.keys(&[Action::ExcludeZ]);
    let d = update(rig.hover(rig.cursor.unwrap() + Vec2::new(70.0, -90.0)));
    assert_eq!(d.translation.z, 0.0);
    assert!(d.translation.truncate().length() > 0.1);
}

#[test]
fn typed_values_override_their_axis_and_freeze_the_drag() {
    let mut rig = moving();
    rig.keys(&[Action::ConstrainX]);
    let d = update(rig.type_chars("2.5"));
    assert_eq!(d.position_override, [Some(2.5), None, None]);
    // Moving the cursor while typing does not move the object.
    let d2 = update(rig.hover(rig.cursor.unwrap() + Vec2::new(100.0, 0.0)));
    assert_eq!(d2.translation, d.translation);
    assert_eq!(rig.keys(&[Action::Confirm]), ManipResult::Commit);
}

#[test]
fn typed_input_accepts_one_leading_minus_and_one_decimal_point() {
    let mut rig = moving();
    rig.keys(&[Action::ConstrainZ]);
    rig.type_chars("-1.2.5");
    let d = update(rig.type_chars("-"));
    assert_eq!(d.position_override, [None, None, Some(-1.25)]);
}

#[test]
fn backspace_erases_the_last_character() {
    let mut rig = moving();
    rig.keys(&[Action::ConstrainY]);
    rig.type_chars("42");
    let d = update(rig.keys(&[Action::NumericBackspace]));
    assert_eq!(d.position_override, [None, Some(4.0), None]);
}

#[test]
fn tab_walks_the_free_axes_of_a_plane_and_wraps() {
    let mut rig = moving();
    rig.keys(&[Action::ExcludeZ]);
    rig.type_chars("1");
    rig.keys(&[Action::NumericNextAxis]);
    rig.type_chars("2");
    rig.keys(&[Action::NumericNextAxis]);
    let d = update(rig.type_chars("3"));
    assert_eq!(d.position_override, [Some(13.0), Some(2.0), None]);
}

#[test]
fn changing_the_constraint_clears_typed_values() {
    let mut rig = moving();
    rig.keys(&[Action::ConstrainX]);
    rig.type_chars("7");
    rig.keys(&[Action::ConstrainY]);
    let d = update(rig.idle());
    assert_eq!(d.position_override, [None, None, None]);
    assert_eq!(rig.driver.numeric_display(), None);
}

#[test]
fn a_typed_character_that_is_not_a_number_does_not_freeze_the_drag() {
    // `typed_chars` should arrive filtered, but a custom resolver may pass every
    // character. The X that picks the axis must not lock the cursor out.
    let mut rig = moving();
    let mut frame = pressed(&[Action::ConstrainX]);
    frame.typed_chars.push('x');
    rig.frame_with(&frame, rig.cursor, Vec2::ZERO, false, false);
    let d = update(rig.hover(rig.cursor.unwrap() + Vec2::new(80.0, 0.0)));
    assert!(d.translation.x.abs() > 0.0, "the drag still moves: {d:?}");
    assert_eq!(rig.driver.numeric_display(), None);
}

#[test]
fn rotate_and_scale_report_typed_values_on_their_own_channels() {
    let mut rig = Rig::new(three_quarter(), Vec3::ZERO);
    rig.hover(rig.centre_px() + Vec2::new(200.0, 0.0));
    rig.keys(&[Action::BeginRotate]);
    rig.keys(&[Action::ConstrainZ]);
    let d = update(rig.type_chars("90"));
    assert_eq!(d.rotation_override, [None, None, Some(90.0)]);
    assert_eq!(d.position_override, [None; 3]);
    rig.keys(&[Action::Cancel]);

    rig.keys(&[Action::BeginScale]);
    let d = update(rig.type_chars("2"));
    assert_eq!(d.scale_override, [Some(2.0), None, None]);
}

#[test]
fn the_mode_follows_the_session_and_cannot_be_changed_mid_session() {
    let mut rig = Rig::new(three_quarter(), Vec3::ZERO);
    assert_eq!(rig.driver.mode(), GizmoMode::Translate);
    rig.hover(rig.centre_px() + Vec2::new(200.0, 0.0));
    rig.keys(&[Action::BeginRotate]);
    assert_eq!(rig.driver.mode(), GizmoMode::Rotate);
    rig.driver.set_mode(GizmoMode::Scale);
    rig.driver.cycle_mode();
    assert_eq!(rig.driver.mode(), GizmoMode::Rotate);
    assert_eq!(rig.driver.state().unwrap().kind, ManipulationKind::Rotate);
}

#[test]
fn a_second_begin_key_mid_session_is_ignored() {
    let mut rig = moving();
    let r = rig.keys(&[Action::BeginScale]);
    assert!(matches!(r, ManipResult::Update(_)));
    assert_eq!(rig.driver.state().unwrap().kind, ManipulationKind::Move);
}

#[test]
fn the_readout_names_the_mode_constraint_value_mode_and_typed_values() {
    let mut rig = moving();
    assert_eq!(rig.driver.readout().as_deref(), Some("Translate [rel]"));
    rig.keys(&[Action::ExcludeZ]);
    assert_eq!(rig.driver.readout().as_deref(), Some("Translate XY [rel]"));
    rig.driver.set_value_mode(ValueMode::Objective);
    rig.type_chars("3");
    assert_eq!(
        rig.driver.readout().as_deref(),
        Some("Translate XY [obj]:  *X: 3  Y: _")
    );
    rig.keys(&[Action::Confirm]);
    assert_eq!(rig.driver.readout(), None);
}

#[test]
fn the_value_mode_toggles_and_survives_sessions() {
    let mut rig = moving();
    rig.driver.toggle_value_mode();
    assert_eq!(rig.driver.value_mode(), ValueMode::Objective);
    rig.keys(&[Action::Confirm]);
    assert_eq!(rig.driver.value_mode(), ValueMode::Objective);
    rig.driver.toggle_value_mode();
    assert_eq!(rig.driver.value_mode(), ValueMode::Relative);
}

#[test]
fn every_view_highlights_the_constraint_while_a_session_runs() {
    let mut rig = moving();
    assert_eq!(rig.driver.highlight(false), GizmoAxis::None);
    rig.keys(&[Action::ConstrainY]);
    assert_eq!(rig.driver.highlight(true), GizmoAxis::Y);
    assert_eq!(rig.driver.highlight(false), GizmoAxis::Y);
    rig.keys(&[Action::ExcludeX]);
    assert_eq!(rig.driver.highlight(false), GizmoAxis::YZ);
}

#[test]
fn hover_only_lights_the_driving_view_when_idle() {
    let mut rig = Rig::new(three_quarter(), Vec3::ZERO);
    rig.hover(rig.px(Vec3::X * rig.scale() * 0.5));
    assert_eq!(rig.driver.highlight(true), GizmoAxis::X);
    assert_eq!(rig.driver.highlight(false), GizmoAxis::None);
}

#[test]
fn an_empty_frame_mid_session_changes_nothing() {
    let mut rig = moving();
    let a = update(rig.hover(rig.cursor.unwrap() + Vec2::new(40.0, 0.0)));
    let b = update(rig.frame_with(
        &ActionFrame::default(),
        rig.cursor,
        Vec2::ZERO,
        false,
        false,
    ));
    assert_eq!(a, b);
}

#[test]
fn erasing_every_typed_character_hands_the_drag_back_to_the_cursor() {
    let mut rig = moving();
    rig.keys(&[Action::ConstrainX]);
    rig.type_chars("5");
    rig.keys(&[Action::NumericBackspace]);
    assert_eq!(rig.driver.numeric_display(), None);
    let d = update(rig.hover(rig.cursor.unwrap() + Vec2::new(80.0, 0.0)));
    assert_eq!(d.position_override, [None; 3]);
    assert!(
        d.translation.x.abs() > 0.0,
        "the cursor moves it again: {d:?}"
    );
}

#[test]
fn a_keyboard_session_gives_the_mode_back_when_it_ends() {
    let mut rig = Rig::new(three_quarter(), Vec3::ZERO);
    rig.hover(rig.centre_px() + Vec2::new(200.0, 0.0));
    for end in [Action::Confirm, Action::Cancel] {
        rig.keys(&[Action::BeginScale]);
        assert_eq!(rig.driver.mode(), GizmoMode::Scale);
        rig.keys(&[end]);
        assert_eq!(rig.driver.mode(), GizmoMode::Translate, "after {end:?}");
    }
}

#[test]
fn removing_the_selection_cancels_a_keyboard_session() {
    let mut rig = moving();
    rig.centre = None;
    assert_eq!(rig.idle(), ManipResult::Cancel);
    assert!(!rig.driver.is_active());
}

#[test]
fn a_free_r_turns_with_the_cursor_sweep_about_the_view_axis() {
    let mut rig = Rig::new(three_quarter(), Vec3::ZERO);
    let c = rig.centre_px();
    // Counter-clockwise on screen (y runs down).
    let at = |deg: f32| {
        let a = deg.to_radians();
        c + Vec2::new(a.cos(), -a.sin()) * 150.0
    };
    rig.hover(at(0.0));
    rig.keys(&[Action::BeginRotate]);
    let mut last = ManipResult::None;
    for deg in (5..=60).step_by(5) {
        last = rig.hover(at(deg as f32));
    }
    let toward_camera = -rig.camera.forward();
    let (axis, angle) = update(last).rotation.to_axis_angle();
    assert!(
        axis.cross(toward_camera).length() < 1e-3,
        "about the view axis: {axis:?}"
    );
    let signed = angle.to_degrees() * axis.dot(toward_camera).signum();
    assert!(
        (signed - 60.0).abs() < 0.5,
        "a 60 degree sweep turns +60 seen from the camera, got {signed}"
    );
}
