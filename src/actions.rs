//! The gizmo's input actions and their default key bindings.
//!
//! Each action is an [`Action::Custom`] value. This crate reserves
//! `0x1000_0000..0x1000_0100`; an application picks its own values below
//! `0x1000_0000`. [`Action::Confirm`] and [`Action::Cancel`] are not here: the
//! controller reads the shared variants.
//!
//! The resolver only reports what it has bindings for, so add
//! [`manipulation_bindings`] to the resolver the gizmo reads from:
//!
//! ```rust,ignore
//! let input = ViewportInput::from_preset(BindingPreset::Default)
//!     .with_bindings(viewport_lib_gizmo::manipulation_bindings());
//! ```

use viewport_lib_types::input::{
    Action, KeyCode, Modifiers, ModifiersMatch, ViewportBinding, ViewportGesture,
};

/// Begin a keyboard move session (G).
pub const BEGIN_MOVE: Action = Action::Custom(0x1000_0000);
/// Begin a keyboard rotate session (R).
pub const BEGIN_ROTATE: Action = Action::Custom(0x1000_0001);
/// Begin a keyboard scale session (S).
pub const BEGIN_SCALE: Action = Action::Custom(0x1000_0002);
/// Constrain the session to the X axis (X).
pub const CONSTRAIN_X: Action = Action::Custom(0x1000_0003);
/// Constrain the session to the Y axis (Y).
pub const CONSTRAIN_Y: Action = Action::Custom(0x1000_0004);
/// Constrain the session to the Z axis (Z).
pub const CONSTRAIN_Z: Action = Action::Custom(0x1000_0005);
/// Exclude the X axis and work in the YZ plane (Shift+X).
pub const EXCLUDE_X: Action = Action::Custom(0x1000_0006);
/// Exclude the Y axis and work in the XZ plane (Shift+Y).
pub const EXCLUDE_Y: Action = Action::Custom(0x1000_0007);
/// Exclude the Z axis and work in the XY plane (Shift+Z).
pub const EXCLUDE_Z: Action = Action::Custom(0x1000_0008);
/// Erase the last typed character of the numeric value (Backspace).
pub const NUMERIC_BACKSPACE: Action = Action::Custom(0x1000_0009);
/// Move numeric entry to the next axis (Tab).
pub const NUMERIC_NEXT_AXIS: Action = Action::Custom(0x1000_000A);
/// Cycle the gizmo mode: translate -> rotate -> scale (Tab).
pub const CYCLE_GIZMO_MODE: Action = Action::Custom(0x1000_000B);
/// Cycle the pivot mode forward (`[`).
pub const CYCLE_PIVOT_FORWARD: Action = Action::Custom(0x1000_000C);
/// Cycle the pivot mode backward (`]`).
pub const CYCLE_PIVOT_BACKWARD: Action = Action::Custom(0x1000_000D);
/// Toggle the gizmo between world and local space.
///
/// Not bound by [`manipulation_bindings`]: bind it to a key of your choice and
/// call `GizmoDriver::set_space` when it fires.
pub const TOGGLE_SPACE: Action = Action::Custom(0x1000_000E);

/// The gizmo's default key bindings, to add to the resolver the gizmo reads.
///
/// - G, R and S begin a move, rotate or scale session.
/// - X, Y and Z constrain to an axis; with Shift they exclude it.
/// - Backspace and Tab edit a typed value during a session.
/// - Tab cycles the gizmo mode outside a session.
/// - `[` and `]` cycle the pivot mode forward and backward.
///
/// The letters are bound by character ([`ViewportGesture::CharPress`]), so they
/// follow the keyboard layout: on QWERTZ the Z constraint is the key labelled Z.
/// Gate them on a selection or a running session; the controller does this for
/// the session keys.
pub fn manipulation_bindings() -> Vec<ViewportBinding> {
    let none = ModifiersMatch::Exact(Modifiers::NONE);
    let shift = ModifiersMatch::Contains(Modifiers::SHIFT);
    let char_press = |action, ch, modifiers| {
        ViewportBinding::new(action, ViewportGesture::CharPress { ch, modifiers })
    };
    let key_press = |action, key| {
        ViewportBinding::new(
            action,
            ViewportGesture::KeyPress {
                key,
                modifiers: none,
            },
        )
    };
    vec![
        // Begin a session.
        char_press(BEGIN_MOVE, 'g', none),
        char_press(BEGIN_ROTATE, 'r', none),
        char_press(BEGIN_SCALE, 's', none),
        // During a session: constrain to an axis, or with Shift exclude it.
        char_press(CONSTRAIN_X, 'x', none),
        char_press(CONSTRAIN_Y, 'y', none),
        char_press(CONSTRAIN_Z, 'z', none),
        char_press(EXCLUDE_X, 'x', shift),
        char_press(EXCLUDE_Y, 'y', shift),
        char_press(EXCLUDE_Z, 'z', shift),
        // During a session: edit the typed value. These keys type no character.
        key_press(NUMERIC_BACKSPACE, KeyCode::Backspace),
        key_press(NUMERIC_NEXT_AXIS, KeyCode::Tab),
        // Outside a session: the gizmo mode and the pivot.
        key_press(CYCLE_GIZMO_MODE, KeyCode::Tab),
        key_press(CYCLE_PIVOT_FORWARD, KeyCode::LeftBracket),
        key_press(CYCLE_PIVOT_BACKWARD, KeyCode::RightBracket),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constants_are_distinct_and_in_the_reserved_range() {
        let all = [
            BEGIN_MOVE,
            BEGIN_ROTATE,
            BEGIN_SCALE,
            CONSTRAIN_X,
            CONSTRAIN_Y,
            CONSTRAIN_Z,
            EXCLUDE_X,
            EXCLUDE_Y,
            EXCLUDE_Z,
            NUMERIC_BACKSPACE,
            NUMERIC_NEXT_AXIS,
            CYCLE_GIZMO_MODE,
            CYCLE_PIVOT_FORWARD,
            CYCLE_PIVOT_BACKWARD,
            TOGGLE_SPACE,
        ];
        for (i, a) in all.iter().enumerate() {
            let Action::Custom(v) = *a else {
                panic!("not a Custom action");
            };
            assert_eq!(v, 0x1000_0000 + i as u32);
            assert!(all[i + 1..].iter().all(|b| b != a));
        }
    }

    #[test]
    fn bindings_cover_the_session_keys() {
        let bindings = manipulation_bindings();
        assert!(bindings.iter().any(|b| b.action == BEGIN_MOVE));
        assert!(bindings.iter().any(|b| b.action == EXCLUDE_Z));
        assert!(bindings.iter().all(|b| b.action != TOGGLE_SPACE));
    }
}
