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
//!
//! What the field counts follows the value mode. Relative reads the delta, so it
//! starts at zero (or one, for scale) and shows how far the drag has gone.
//! Objective reads the object's own world values, so it shows where the object
//! actually is. The gizmo does not know the object's pose, so the host passes it in
//! as an [`ObjectPose`].

use glam::{EulerRot, Mat4, Quat};
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

/// An object's pose in world space, as plain arrays, for a readout that shows real
/// values rather than a delta. `rotation` is a quaternion in `[x, y, z, w]` order.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ObjectPose {
    /// World position.
    pub translation: [f32; 3],
    /// World orientation as a quaternion `[x, y, z, w]`.
    pub rotation: [f32; 4],
    /// World scale.
    pub scale: [f32; 3],
}

impl Default for ObjectPose {
    fn default() -> Self {
        Self {
            translation: [0.0; 3],
            rotation: [0.0, 0.0, 0.0, 1.0],
            scale: [1.0; 3],
        }
    }
}

impl ObjectPose {
    /// The pose a transform matrix holds, decomposed. A host that keeps its objects
    /// as matrices can hand one straight over.
    pub fn from_matrix(transform: Mat4) -> Self {
        let (scale, rotation, translation) = transform.to_scale_rotation_translation();
        Self {
            translation: translation.to_array(),
            rotation: rotation.to_array(),
            scale: scale.to_array(),
        }
    }
}

/// The x/y/z values to show for a mode's delta: translation for translate, scale
/// for scale, and euler angles in degrees for rotate. A value typed for an axis is
/// shown in place of the dragged one, so the field reads back what was entered.
///
/// This is the relative reading, the offset since the drag began. For a field that
/// follows the value mode, use [`readout_values_in`].
pub fn readout_values(mode: GizmoMode, delta: &GizmoDelta) -> [f32; 3] {
    match mode {
        GizmoMode::Translate => delta.position(),
        GizmoMode::Scale => delta.scale_values(),
        GizmoMode::Rotate => {
            let (x, y, z) = Quat::from_array(delta.rotation).to_euler(EulerRot::XYZ);
            override_into([x.to_degrees(), y.to_degrees(), z.to_degrees()], &degrees(delta.rotation_override))
        }
    }
}

/// The x/y/z values to show for a mode under `values`: the delta in
/// [`Relative`](ValueMode::Relative), the object's own world values in
/// [`Objective`](ValueMode::Objective), so the field reads where the object is
/// rather than how far it has moved. A value typed for an axis is shown as typed in
/// either mode.
pub fn readout_values_in(
    mode: GizmoMode,
    delta: &GizmoDelta,
    values: ValueMode,
    pose: &ObjectPose,
) -> [f32; 3] {
    if !matches!(values, ValueMode::Objective) {
        return readout_values(mode, delta);
    }
    match mode {
        GizmoMode::Translate => override_into(pose.translation, &delta.position_override),
        GizmoMode::Scale => override_into(pose.scale, &delta.scale_override),
        GizmoMode::Rotate => {
            let (x, y, z) = Quat::from_array(pose.rotation).to_euler(EulerRot::XYZ);
            override_into([x.to_degrees(), y.to_degrees(), z.to_degrees()], &degrees(delta.rotation_override))
        }
    }
}

/// `base` with each typed value standing in for its axis.
fn override_into(base: [f32; 3], typed: &[Option<f32>; 3]) -> [f32; 3] {
    let mut out = base;
    for (i, value) in typed.iter().enumerate() {
        if let Some(value) = value {
            out[i] = *value;
        }
    }
    out
}

/// Typed angles in radians read as degrees, to match the displayed axes.
fn degrees(typed: [Option<f32>; 3]) -> [Option<f32>; 3] {
    typed.map(|angle| angle.map(f32::to_degrees))
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
            ..Default::default()
        };
        assert_eq!(readout_values(GizmoMode::Translate, &delta), [1.0, 2.0, 3.0]);
        assert_eq!(readout_values(GizmoMode::Scale, &delta), [4.0, 5.0, 6.0]);

        let [rx, ry, rz] = readout_values(GizmoMode::Rotate, &delta);
        assert!(rx.abs() < 1e-3 && ry.abs() < 1e-3);
        assert!((rz - 90.0).abs() < 1e-3, "a quarter turn about Z reads 90 degrees");
    }

    #[test]
    fn a_typed_value_shows_in_the_readout() {
        let delta = GizmoDelta {
            translation: [1.0, 2.0, 3.0],
            position_override: [Some(5.0), None, None],
            ..Default::default()
        };
        assert_eq!(
            readout_values(GizmoMode::Translate, &delta),
            [5.0, 2.0, 3.0],
            "the typed X stands in, the dragged Y and Z stay"
        );
    }

    #[test]
    fn objective_mode_reads_the_object_not_the_offset() {
        // Halfway through a two-unit drag on X, from an object standing at x = 10.
        let delta = GizmoDelta { translation: [2.0, 0.0, 0.0], ..Default::default() };
        let pose = ObjectPose {
            translation: [12.0, 3.0, 0.0],
            scale: [2.0, 2.0, 2.0],
            ..Default::default()
        };

        assert_eq!(
            readout_values_in(GizmoMode::Translate, &delta, ValueMode::Relative, &pose),
            [2.0, 0.0, 0.0],
            "relative counts the drag"
        );
        assert_eq!(
            readout_values_in(GizmoMode::Translate, &delta, ValueMode::Objective, &pose),
            [12.0, 3.0, 0.0],
            "objective reads where the object stands"
        );
        assert_eq!(
            readout_values_in(GizmoMode::Scale, &delta, ValueMode::Objective, &pose),
            [2.0, 2.0, 2.0],
            "and its real scale, not the factor applied"
        );
    }

    #[test]
    fn objective_rotation_reads_the_object_angle() {
        let pose = ObjectPose {
            rotation: Quat::from_rotation_z(std::f32::consts::FRAC_PI_2).to_array(),
            ..Default::default()
        };
        let delta = GizmoDelta::default();
        let [x, y, z] = readout_values_in(GizmoMode::Rotate, &delta, ValueMode::Objective, &pose);
        assert!(x.abs() < 1e-3 && y.abs() < 1e-3);
        assert!((z - 90.0).abs() < 1e-3, "a quarter turn about Z reads 90 degrees");
    }

    #[test]
    fn a_typed_value_still_shows_as_typed_in_objective_mode() {
        let delta = GizmoDelta {
            translation: [2.0, 0.0, 0.0],
            position_override: [Some(7.0), None, None],
            ..Default::default()
        };
        let pose = ObjectPose { translation: [12.0, 3.0, 0.0], ..Default::default() };
        assert_eq!(
            readout_values_in(GizmoMode::Translate, &delta, ValueMode::Objective, &pose),
            [7.0, 3.0, 0.0],
            "the typed X reads back, the other axes read the object"
        );
    }

    #[test]
    fn a_pose_comes_out_of_a_matrix() {
        let m = glam::Mat4::from_scale_rotation_translation(
            glam::Vec3::new(2.0, 2.0, 2.0),
            Quat::IDENTITY,
            glam::Vec3::new(1.0, 2.0, 3.0),
        );
        let pose = ObjectPose::from_matrix(m);
        assert_eq!(pose.translation, [1.0, 2.0, 3.0]);
        assert_eq!(pose.scale, [2.0, 2.0, 2.0]);
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
