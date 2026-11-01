//! Private session state machine for the manipulation controller.
//!
//! This module is not part of the public API. All types are `pub(crate)`.

use crate::maths::gizmo::GizmoAxis;
use viewport_lib_types::input::{Action, ActionFrame};

use super::types::{ManipulationKind, ManipulationState};

/// Buffers numeric input from the keyboard during a manipulation session.
///
/// Activated when the user types a digit during an active session.
/// Each active axis gets its own string buffer.
///
/// Fields and methods are unused until `Action::NumericDigit/Backspace/Tab` are added.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub(crate) struct NumericInputState {
    /// Which axis indices are accepting input (depends on constraint).
    pub(crate) active_axes: Vec<usize>,
    /// Index into `active_axes` pointing at the currently focused axis.
    pub(crate) current_axis_idx: usize,
    /// Digit-string buffers for each of the three axes (X=0, Y=1, Z=2).
    pub(crate) axis_inputs: [String; 3],
}

#[allow(dead_code)]
impl NumericInputState {
    /// Create a new numeric input state for the given axis constraint.
    pub(crate) fn new(axis: Option<GizmoAxis>, excluded: bool) -> Self {
        let active_axes = match axis {
            None => vec![0, 1, 2],
            Some(GizmoAxis::X) => {
                if excluded {
                    vec![1, 2]
                } else {
                    vec![0]
                }
            }
            Some(GizmoAxis::Y) => {
                if excluded {
                    vec![0, 2]
                } else {
                    vec![1]
                }
            }
            Some(GizmoAxis::Z) | Some(GizmoAxis::None) => {
                if excluded {
                    vec![0, 1]
                } else {
                    vec![2]
                }
            }
            _ => vec![0, 1, 2],
        };
        Self {
            active_axes,
            current_axis_idx: 0,
            axis_inputs: [String::new(), String::new(), String::new()],
        }
    }

    /// Index of the axis currently receiving typed input.
    pub(crate) fn current_axis(&self) -> usize {
        self.active_axes[self.current_axis_idx]
    }

    /// Parse each axis buffer into an `f32`, returning `None` for empty or invalid input.
    pub(crate) fn parsed_values(&self) -> [Option<f32>; 3] {
        core::array::from_fn(|i| self.axis_inputs[i].parse::<f32>().ok())
    }

    /// Format a HUD-friendly display string, e.g. "X: 2.50 Y: _ Z: _".
    pub(crate) fn display_string(&self) -> String {
        let labels = ["X", "Y", "Z"];
        let current = self.current_axis();
        let mut parts = Vec::new();
        for (i, label) in labels.iter().enumerate() {
            if self.active_axes.contains(&i) {
                let val = if self.axis_inputs[i].is_empty() {
                    "_".to_string()
                } else {
                    self.axis_inputs[i].clone()
                };
                // Mark the field currently accepting input.
                let marker = if i == current { "*" } else { "" };
                parts.push(format!("{marker}{label}: {val}"));
            }
        }
        parts.join("  ")
    }
}

/// Private session state for an in-progress manipulation.
#[derive(Debug, Clone)]
pub(crate) struct ManipulationSession {
    /// Which transform operation is active.
    pub(crate) kind: ManipulationKind,
    /// Active axis constraint, if any.
    pub(crate) axis: Option<GizmoAxis>,
    /// Whether the constrained axis is excluded (Shift+axis).
    pub(crate) exclude_axis: bool,
    /// Numeric input state, active once the user begins typing digits.
    pub(crate) numeric: Option<NumericInputState>,
    /// Whether this session began from a gizmo drag.
    pub(crate) is_gizmo_drag: bool,
    /// World-space center captured when the session began.
    pub(crate) gizmo_center: glam::Vec3,
    /// Viewport-local cursor position when the session started or the constraint last changed.
    ///
    /// Used to compute true per-frame increments from the absolute cursor position,
    /// so that changing constraints resets the delta accumulation cleanly.
    pub(crate) cursor_anchor: Option<glam::Vec2>,
    /// Total cursor displacement (current - anchor) that was used on the previous frame.
    ///
    /// The per-frame delta passed to solvers is `(current - anchor) - cursor_last_total`.
    /// Reset to zero whenever `cursor_anchor` is updated.
    pub(crate) cursor_last_total: glam::Vec2,
    /// The cumulative distance-ratio scale factor (current_dist / anchor_dist)
    /// since the session (or last constraint change) began. Reset to 1.0 whenever
    /// `cursor_anchor` is updated. Emitted as the total [`TransformDelta::scale`].
    pub(crate) cumulative_scale: f32,
    /// Running sum of the world translation since the session (or last constraint
    /// change) began. Snapping rounds this cumulative value, not a per-frame
    /// increment, so the object stays grid-locked without drift. Emitted as the
    /// total [`TransformDelta::translation`].
    pub(crate) cumulative_translation: glam::Vec3,
    /// Running sum of the rotation angle (radians) about the constrained axis,
    /// since the session began. Only tracked for single-axis rotation, where angle
    /// snapping is meaningful.
    pub(crate) cumulative_angle: f32,
    /// Accumulated rotation for the excluded-axis and unconstrained modes, which
    /// have no single scalar angle. Emitted as the total [`TransformDelta::rotation`].
    pub(crate) cumulative_rotation: glam::Quat,
}

impl Default for ManipulationSession {
    fn default() -> Self {
        Self {
            kind: ManipulationKind::Move,
            axis: None,
            exclude_axis: false,
            numeric: None,
            is_gizmo_drag: false,
            gizmo_center: glam::Vec3::ZERO,
            cursor_anchor: None,
            cursor_last_total: glam::Vec2::ZERO,
            cumulative_scale: 1.0,
            cumulative_translation: glam::Vec3::ZERO,
            cumulative_angle: 0.0,
            cumulative_rotation: glam::Quat::IDENTITY,
        }
    }
}

impl ManipulationSession {
    /// Convert to the public inspectable snapshot.
    pub(crate) fn to_state(&self) -> ManipulationState {
        let numeric_display = self
            .numeric
            .as_ref()
            .map(|n| n.display_string())
            .filter(|s| !s.is_empty());
        ManipulationState {
            kind: self.kind,
            axis: self.axis,
            exclude_axis: self.exclude_axis,
            is_gizmo_drag: self.is_gizmo_drag,
            center: self.gizmo_center,
            numeric_display,
        }
    }
}

/// Update axis constraint on the session.
///
/// Resets numeric input whenever the constraint changes so that previously
/// typed values do not carry over to the new axis.
pub(crate) fn update_constraint(
    session: &mut ManipulationSession,
    constrain_x: bool,
    constrain_y: bool,
    constrain_z: bool,
    exclude_x: bool,
    exclude_y: bool,
    exclude_z: bool,
) {
    let mut set_axis = |axis: GizmoAxis, exclude: bool| {
        session.axis = Some(axis);
        session.exclude_axis = exclude;
        session.numeric = None;
    };

    if constrain_x {
        set_axis(GizmoAxis::X, false);
    }
    if constrain_y {
        set_axis(GizmoAxis::Y, false);
    }
    if constrain_z {
        set_axis(GizmoAxis::Z, false);
    }
    if exclude_x {
        set_axis(GizmoAxis::X, true);
    }
    if exclude_y {
        set_axis(GizmoAxis::Y, true);
    }
    if exclude_z {
        set_axis(GizmoAxis::Z, true);
    }
}

/// Update numeric buffering for the session.
///
/// Reads `frame.typed_chars` (digits, `.`, `-`) and `Action::NumericBackspace` /
/// `Action::NumericNextAxis` to maintain per-axis string buffers. The first digit
/// typed automatically initialises the numeric state.
pub(crate) fn update_numeric_state(session: &mut ManipulationSession, frame: &ActionFrame) {
    // Bootstrap: create numeric state on the first typed digit, sign, or point.
    // Other characters are ignored, so one that slips through an unfiltered
    // resolver does not freeze the drag with an empty buffer.
    let numeric_char = |c: &char| c.is_ascii_digit() || *c == '.' || *c == '-';
    if session.numeric.is_none() && frame.typed_chars.iter().any(numeric_char) {
        session.numeric = Some(NumericInputState::new(session.axis, session.exclude_axis));
    }

    let Some(ref mut numeric) = session.numeric else {
        return;
    };
    let axis_idx = numeric.current_axis();

    // Append typed characters to the current axis buffer.
    for &c in &frame.typed_chars {
        let buf = &mut numeric.axis_inputs[axis_idx];
        // Allow '-' only as the first character.
        if c == '-' && buf.is_empty() {
            buf.push(c);
        } else if c.is_ascii_digit() || c == '.' {
            // Allow at most one decimal point.
            if c != '.' || !buf.contains('.') {
                buf.push(c);
            }
        }
    }

    // Backspace: pop the last character.
    if frame.is_active(Action::NumericBackspace) {
        numeric.axis_inputs[axis_idx].pop();
    }

    // Tab: advance to the next active axis.
    if frame.is_active(Action::NumericNextAxis) {
        let len = numeric.active_axes.len();
        if len > 1 {
            numeric.current_axis_idx = (numeric.current_axis_idx + 1) % len;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- NumericInputState ---

    #[test]
    fn numeric_input_no_constraint_all_axes() {
        let s = NumericInputState::new(None, false);
        assert_eq!(s.active_axes, vec![0, 1, 2]);
    }

    #[test]
    fn numeric_input_constrain_x() {
        let s = NumericInputState::new(Some(GizmoAxis::X), false);
        assert_eq!(s.active_axes, vec![0]);
    }

    #[test]
    fn numeric_input_exclude_x() {
        let s = NumericInputState::new(Some(GizmoAxis::X), true);
        assert_eq!(s.active_axes, vec![1, 2]);
    }

    #[test]
    fn numeric_input_constrain_y() {
        let s = NumericInputState::new(Some(GizmoAxis::Y), false);
        assert_eq!(s.active_axes, vec![1]);
    }

    #[test]
    fn numeric_input_exclude_y() {
        let s = NumericInputState::new(Some(GizmoAxis::Y), true);
        assert_eq!(s.active_axes, vec![0, 2]);
    }

    #[test]
    fn numeric_input_constrain_z() {
        let s = NumericInputState::new(Some(GizmoAxis::Z), false);
        assert_eq!(s.active_axes, vec![2]);
    }

    #[test]
    fn numeric_input_exclude_z() {
        let s = NumericInputState::new(Some(GizmoAxis::Z), true);
        assert_eq!(s.active_axes, vec![0, 1]);
    }

    #[test]
    fn numeric_input_parsed_values_empty() {
        let s = NumericInputState::new(None, false);
        let vals = s.parsed_values();
        assert!(vals[0].is_none());
        assert!(vals[1].is_none());
        assert!(vals[2].is_none());
    }

    #[test]
    fn numeric_input_parsed_values_with_input() {
        let mut s = NumericInputState::new(None, false);
        s.axis_inputs[0] = "2.5".to_string();
        s.axis_inputs[2] = "-1".to_string();
        let vals = s.parsed_values();
        assert!((vals[0].unwrap() - 2.5).abs() < 1e-6);
        assert!(vals[1].is_none());
        assert!((vals[2].unwrap() - (-1.0)).abs() < 1e-6);
    }

    #[test]
    fn numeric_input_display_string() {
        let mut s = NumericInputState::new(Some(GizmoAxis::X), true);
        // Active axes: [1, 2] (Y, Z)
        s.axis_inputs[1] = "3.0".to_string();
        let display = s.display_string();
        assert!(display.contains("Y: 3.0"));
        assert!(display.contains("Z: _"));
        assert!(!display.contains("X:"));
    }

    #[test]
    fn numeric_input_current_axis() {
        let s = NumericInputState::new(Some(GizmoAxis::Y), true);
        // active_axes = [0, 2], current_axis_idx = 0
        assert_eq!(s.current_axis(), 0); // X
    }

    // --- update_constraint ---

    #[test]
    fn update_constraint_sets_axis() {
        let mut session = ManipulationSession {
            kind: ManipulationKind::Move,
            axis: None,
            exclude_axis: false,
            numeric: None,
            is_gizmo_drag: false,
            gizmo_center: glam::Vec3::ZERO,
            cursor_anchor: None,
            cursor_last_total: glam::Vec2::ZERO,
            cumulative_scale: 1.0,
            ..Default::default()
        };
        update_constraint(&mut session, true, false, false, false, false, false);
        assert_eq!(session.axis, Some(GizmoAxis::X));
        assert!(!session.exclude_axis);
    }

    #[test]
    fn update_constraint_exclude_mode() {
        let mut session = ManipulationSession {
            kind: ManipulationKind::Scale,
            axis: None,
            exclude_axis: false,
            numeric: None,
            is_gizmo_drag: false,
            gizmo_center: glam::Vec3::ZERO,
            cursor_anchor: None,
            cursor_last_total: glam::Vec2::ZERO,
            cumulative_scale: 1.0,
            ..Default::default()
        };
        update_constraint(&mut session, false, false, false, false, true, false);
        assert_eq!(session.axis, Some(GizmoAxis::Y));
        assert!(session.exclude_axis);
    }

    #[test]
    fn update_constraint_clears_numeric() {
        let mut session = ManipulationSession {
            kind: ManipulationKind::Move,
            axis: Some(GizmoAxis::X),
            exclude_axis: false,
            numeric: Some(NumericInputState::new(Some(GizmoAxis::X), false)),
            is_gizmo_drag: false,
            gizmo_center: glam::Vec3::ZERO,
            cursor_anchor: None,
            cursor_last_total: glam::Vec2::ZERO,
            cumulative_scale: 1.0,
            ..Default::default()
        };
        update_constraint(&mut session, false, true, false, false, false, false);
        assert!(session.numeric.is_none());
    }
}
