//! Sibling widgets for the gizmo: a numeric readout and a value-mode toggle.
//!
//! These are ordinary viewport-lib-ui widgets the host places beside the gizmo
//! pane, not part of what the custom paints. The readout is a vector-editor field
//! showing the current x/y/z values, a nicer replacement for the old plain label.
//! The value-mode toggle is a two-option segmented control bound to the gizmo's
//! relative/objective mode.
//!
//! The host builds these once, updates the readout each frame with [`set_readout`]
//! from the delta it reads back, and applies the toggle with the tool's
//! [`set_value_mode`](super::widget::GizmoTool::set_value_mode).

use glam::{EulerRot, Quat};
use viewport_lib_ui_document::{
    Align, Axis, Decoration, EdgeInsets, Layout, Metric, Paint, Style, Widget, WidgetId,
    WidgetKind, roles,
};

use crate::controller::ValueMode;
use crate::maths::gizmo::GizmoMode;

use super::widget::GizmoDelta;

/// Default inner padding of the readout panel, in logical pixels.
pub const READOUT_PADDING: f32 = 6.0;
/// Default corner radius of the readout panel.
pub const READOUT_CORNER_RADIUS: f32 = 6.0;
/// Default gap between the readout field and the value-mode toggle.
pub const READOUT_SPACING: f32 = 6.0;
/// Default font size for the value-mode toggle. The segmented control derives its
/// padding from the font size, so a smaller font keeps it compact.
pub const READOUT_SEGMENTED_FONT: f32 = 13.0;

/// The x/y/z values to show for a mode's delta: translation for translate, scale
/// for scale, and euler angles in degrees for rotate.
pub fn readout_values(mode: GizmoMode, delta: &GizmoDelta) -> [f32; 3] {
    match mode {
        GizmoMode::Translate => delta.translation,
        GizmoMode::Scale => delta.scale,
        GizmoMode::Rotate => {
            let (x, y, z) = Quat::from_array(delta.rotation).to_euler(EulerRot::XYZ);
            [x.to_degrees(), y.to_degrees(), z.to_degrees()]
        }
    }
}

/// Build the numeric readout as a vector-editor field showing x/y/z. The host
/// places it beside the gizmo and refreshes it with [`set_readout`] each frame.
pub fn readout_field(id: WidgetId) -> Widget {
    Widget::vector_field(id, [0.0, 0.0, 0.0], 0.01, 2)
}

/// Set the readout field's displayed x/y/z, finding it under `root` by id. A no-op
/// if the id is missing or is not a vector field.
pub fn set_readout(root: &mut Widget, id: WidgetId, values: [f32; 3]) {
    if let Some(widget) = root.find_mut(id) {
        if let WidgetKind::VectorField { value, .. } = &mut widget.kind {
            *value = values.to_vec();
        }
    }
}

/// Build the value-mode toggle as a two-option segmented control, selected to
/// match `mode`. A pick reports the new index; map it back with
/// [`value_mode_from_index`] and pass it to
/// [`set_value_mode`](super::widget::GizmoTool::set_value_mode).
pub fn value_mode_segmented(id: WidgetId, mode: ValueMode) -> Widget {
    Widget::segmented(id, ["Relative", "Objective"], value_mode_index(mode))
}

/// The segmented index for a value mode.
pub fn value_mode_index(mode: ValueMode) -> u32 {
    match mode {
        ValueMode::Objective => 1,
        _ => 0,
    }
}

/// The value mode for a segmented index.
pub fn value_mode_from_index(index: u32) -> ValueMode {
    match index {
        1 => ValueMode::Objective,
        _ => ValueMode::Relative,
    }
}

/// Build the gizmo's readout panel: a compact, rounded box that hugs a vector
/// readout field and a value-mode toggle, centred, with the crate's default
/// padding and styling. The host places it (for example bottom-centre of the
/// viewport), refreshes the field with [`set_readout`] each frame, and applies the
/// toggle. `panel_id`, `field_id`, and `mode_id` are the caller's widget ids for
/// the three nodes, so it can address them afterwards.
///
/// Colours resolve against the active theme (surface, text), so the panel matches
/// the surrounding UI. Toggle whether it is shown with
/// [`GizmoTool::show_readout`](super::widget::GizmoTool::show_readout).
pub fn readout_panel(
    panel_id: WidgetId,
    field_id: WidgetId,
    mode_id: WidgetId,
    value_mode: ValueMode,
) -> Widget {
    let field = readout_field(field_id);
    let mode = value_mode_segmented(mode_id, value_mode).with_style(Style {
        font_size: Metric::px(READOUT_SEGMENTED_FONT),
        foreground: Paint::role(roles::TEXT),
        ..Style::default()
    });

    Widget::new(panel_id, WidgetKind::Panel)
        .with_style(Style {
            background: Paint::role_alpha(roles::SURFACE, 0.95),
            foreground: Paint::role(roles::TEXT),
            padding: EdgeInsets::all(READOUT_PADDING),
            ..Style::default()
        })
        .with_decoration(Decoration {
            corner_radius: Metric::px(READOUT_CORNER_RADIUS),
            ..Decoration::default()
        })
        .with_layout(Layout::Stack {
            axis: Axis::Vertical,
            spacing: READOUT_SPACING,
            align: Align::Centre,
        })
        .with_children([field, mode])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readout_values_pick_the_vector_for_the_mode() {
        let delta = GizmoDelta {
            translation: [1.0, 2.0, 3.0],
            scale: [4.0, 5.0, 6.0],
            rotation: Quat::from_rotation_z(std::f32::consts::FRAC_PI_2).to_array(),
        };
        assert_eq!(readout_values(GizmoMode::Translate, &delta), [1.0, 2.0, 3.0]);
        assert_eq!(readout_values(GizmoMode::Scale, &delta), [4.0, 5.0, 6.0]);

        let [rx, ry, rz] = readout_values(GizmoMode::Rotate, &delta);
        assert!(rx.abs() < 1e-3 && ry.abs() < 1e-3);
        assert!((rz - 90.0).abs() < 1e-3, "a quarter turn about Z reads 90 degrees");
    }

    #[test]
    fn the_readout_field_is_a_three_channel_vector_field() {
        match readout_field(7).kind {
            WidgetKind::VectorField { value, precision, .. } => {
                assert_eq!(value, vec![0.0, 0.0, 0.0]);
                assert_eq!(precision, 2);
            }
            other => panic!("expected a vector field, got {other:?}"),
        }
    }

    #[test]
    fn set_readout_writes_the_field_value() {
        let mut root = Widget::new(0, WidgetKind::Panel).with_child(readout_field(7));
        set_readout(&mut root, 7, [1.5, -2.0, 0.25]);
        match &root.find(7).expect("field exists").kind {
            WidgetKind::VectorField { value, .. } => assert_eq!(value, &vec![1.5, -2.0, 0.25]),
            other => panic!("expected a vector field, got {other:?}"),
        }
    }

    #[test]
    fn value_mode_round_trips_through_the_segmented_index() {
        assert_eq!(value_mode_index(ValueMode::Relative), 0);
        assert_eq!(value_mode_index(ValueMode::Objective), 1);
        assert_eq!(value_mode_from_index(0), ValueMode::Relative);
        assert_eq!(value_mode_from_index(1), ValueMode::Objective);

        match value_mode_segmented(9, ValueMode::Objective).kind {
            WidgetKind::Segmented { options, selected } => {
                assert_eq!(options, vec!["Relative".to_string(), "Objective".to_string()]);
                assert_eq!(selected, 1);
            }
            other => panic!("expected a segmented control, got {other:?}"),
        }
    }
}
