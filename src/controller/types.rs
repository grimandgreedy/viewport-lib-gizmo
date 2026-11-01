//! Public types for the manipulation controller: kinds, results, state, and input.

use glam::{Quat, Vec2, Vec3};
use viewport_lib_types::camera::Camera;
use viewport_lib_types::input::ActionFrame;

use crate::maths::gizmo::{GizmoAxis, GizmoMode};

/// Which kind of transform manipulation is in progress.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManipulationKind {
    /// Move (translate) selected objects.
    Move,
    /// Rotate selected objects.
    Rotate,
    /// Scale selected objects.
    Scale,
}

/// Whether the gizmo's values are read relative to the start pose, or as absolute
/// (objective) world values.
///
/// The gizmo only tracks and displays this; the app decides how to apply numeric
/// input and drags accordingly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum ValueMode {
    /// Values are offsets from the start pose (the default).
    #[default]
    Relative,
    /// Values are absolute world coordinates.
    Objective,
}

impl ValueMode {
    /// The other mode.
    pub fn toggled(self) -> Self {
        match self {
            ValueMode::Relative => ValueMode::Objective,
            _ => ValueMode::Relative,
        }
    }

    /// A short tag for the readout label (`"rel"` / `"obj"`).
    pub fn tag(self) -> &'static str {
        match self {
            ValueMode::Relative => "rel",
            ValueMode::Objective => "obj",
        }
    }
}

/// The total transform since the drag began. Zero/identity values mean "no change".
///
/// This is cumulative, not a per-frame increment: each frame reports the whole
/// change since the session started. Apply it to the object's drag-start snapshot
/// around the drag-start pivot, and you get the object's current pose; recomputing
/// from the snapshot each frame means no drift and a trivial cancel (restore the
/// snapshot).
///
/// The `*_override` arrays carry raw numeric input, one value per axis. They are
/// interpretation-agnostic: the controller reports the typed numbers, and the app
/// decides whether to apply them as absolute values or relative to the drag-start
/// transform (an app-managed toggle). `None` means no override on that axis.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TransformDelta {
    /// Total translation since the drag began, added to the drag-start position.
    pub translation: glam::Vec3,
    /// Total rotation since the drag began (world-space), composed with the
    /// drag-start rotation.
    pub rotation: glam::Quat,
    /// Total scale since the drag began, multiplied with the drag-start scale.
    pub scale: glam::Vec3,
    /// Numeric input for each position axis. None = no override.
    pub position_override: [Option<f32>; 3],
    /// Numeric input for each rotation axis, as an angle. None = no override.
    pub rotation_override: [Option<f32>; 3],
    /// Numeric input for each scale axis. None = no override.
    pub scale_override: [Option<f32>; 3],
}

impl Default for TransformDelta {
    fn default() -> Self {
        Self {
            translation: glam::Vec3::ZERO,
            rotation: glam::Quat::IDENTITY,
            scale: glam::Vec3::ONE,
            position_override: [None; 3],
            rotation_override: [None; 3],
            scale_override: [None; 3],
        }
    }
}

/// What the manipulation controller produced this frame.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ManipResult {
    /// No manipulation is active or no change this frame.
    None,
    /// Active session produced a transform delta : apply to selected objects.
    Update(TransformDelta),
    /// Session completed (Enter or left-click). Apply and finalize.
    Commit,
    /// Session cancelled (Escape). Restore original transforms.
    Cancel,
    /// Axis constraint changed this frame (e.g. free->X, X->Y, X->Shift+Z).
    ///
    /// The app should restore selected objects to the transforms they had at
    /// session start : the same restore it would perform on `Cancel` : but
    /// **without** ending the session or clearing the undo snapshot.
    /// The controller has reset its cursor anchor so the next `Update` delta
    /// will be computed relative to the current cursor position with the new
    /// constraint applied.
    ConstraintChanged,
}

/// Inspectable snapshot of the current manipulation session.
#[derive(Debug, Clone)]
pub struct ManipulationState {
    /// Which transform operation is active.
    pub kind: ManipulationKind,
    /// Active axis constraint, if any.
    pub axis: Option<GizmoAxis>,
    /// Whether the axis is excluded (Shift+axis) rather than constrained.
    pub exclude_axis: bool,
    /// Whether the session began from a gizmo drag (vs keyboard G/R/S).
    pub is_gizmo_drag: bool,
    /// World-space centre of the manipulation (selection centroid at session start).
    pub centre: glam::Vec3,
    /// Numeric input display string for HUD rendering, e.g. "X: 2.50 Y: _ Z: _".
    pub numeric_display: Option<String>,
}

/// What the app knows about the gizmo this frame.
///
/// Pass `None` to skip gizmo drag detection.
#[derive(Debug, Clone, Copy)]
pub struct GizmoInfo {
    /// World-space centre of the gizmo.
    pub centre: glam::Vec3,
    /// World-space arm length (screen-size scale factor).
    pub scale: f32,
    /// Gizmo orientation (for local-space gizmos).
    pub orientation: glam::Quat,
    /// Current gizmo mode.
    pub mode: GizmoMode,
}

/// The per-frame input the controller and the [`GizmoDriver`](super::GizmoDriver)
/// read.
///
/// `selection_centre` is the world-space gizmo pivot, which the consumer computes
/// from its selection (for example with
/// [`gizmo_centre_for_pivot`](crate::maths::gizmo::gizmo_centre_for_pivot) and
/// [`GizmoDriver::pivot_mode`](super::GizmoDriver::pivot_mode)). The pointer
/// fields come from the consumer's pointer handling; [`from_action`](Self::from_action)
/// reads them from the resolved input.
#[derive(Clone, Copy)]
pub struct FrameInput<'a> {
    /// The resolved input for this frame.
    pub action_frame: &'a ActionFrame,
    /// The viewport camera.
    pub camera: &'a Camera,
    /// Viewport size in logical pixels.
    pub viewport_size: Vec2,
    /// Cursor position in viewport pixels (top-left origin), or `None` if outside.
    pub cursor_viewport: Option<Vec2>,
    /// Pointer movement since the last frame, in pixels.
    pub pointer_delta: Vec2,
    /// World-space centre of the selection. `None` hides the gizmo, and ends a
    /// session in progress with [`ManipResult::Cancel`].
    pub selection_centre: Option<Vec3>,
    /// The selection's orientation (for one object, its rotation). In local
    /// space the handles are drawn, picked and dragged along its axes; world
    /// space ignores it. Pass `Quat::IDENTITY` if you only use world space.
    pub selection_orientation: Quat,
    /// True on the frame the primary button goes down.
    pub drag_started: bool,
    /// True while the primary button is held.
    pub dragging: bool,
    /// True on a completed click.
    pub clicked: bool,
}

impl<'a> FrameInput<'a> {
    /// A frame read from resolved viewport input: the cursor, pointer delta and
    /// primary-button state come from `action_frame.pointer`, and the selection
    /// is in world space (see
    /// [`with_selection_orientation`](Self::with_selection_orientation)).
    ///
    /// A tap confirms a keyboard session like a click. Touch reports no button,
    /// so a touch drag on a handle needs the host to set
    /// [`drag_started`](Self::drag_started) and [`dragging`](Self::dragging)
    /// itself.
    pub fn from_action(
        action_frame: &'a ActionFrame,
        camera: &'a Camera,
        viewport_size: Vec2,
        selection_centre: Option<Vec3>,
    ) -> Self {
        let p = &action_frame.pointer;
        Self {
            action_frame,
            camera,
            viewport_size,
            cursor_viewport: p.cursor.or(p.touch_position),
            pointer_delta: p.delta,
            selection_centre,
            selection_orientation: Quat::IDENTITY,
            drag_started: p.pressed,
            dragging: p.dragging,
            clicked: p.clicked || p.tapped,
        }
    }

    /// The same frame with the selection turned to `orientation`, for local
    /// space.
    pub fn with_selection_orientation(mut self, orientation: Quat) -> Self {
        self.selection_orientation = orientation;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transform_delta_default_is_identity() {
        let d = TransformDelta::default();
        assert_eq!(d.translation, glam::Vec3::ZERO);
        assert!((d.rotation.x).abs() < 1e-6);
        assert!((d.rotation.y).abs() < 1e-6);
        assert!((d.rotation.z).abs() < 1e-6);
        assert!((d.rotation.w - 1.0).abs() < 1e-6);
        assert_eq!(d.scale, glam::Vec3::ONE);
        assert!(d.position_override.iter().all(|o| o.is_none()));
        assert!(d.rotation_override.iter().all(|o| o.is_none()));
        assert!(d.scale_override.iter().all(|o| o.is_none()));
    }

    #[test]
    fn manipulation_kind_variants() {
        // Ensure all three variants are distinct
        assert_ne!(ManipulationKind::Move, ManipulationKind::Rotate);
        assert_ne!(ManipulationKind::Rotate, ManipulationKind::Scale);
        assert_ne!(ManipulationKind::Move, ManipulationKind::Scale);
    }

    #[test]
    fn manip_result_none_variant() {
        let r = ManipResult::None;
        assert!(matches!(r, ManipResult::None));
    }

    #[test]
    fn manip_result_update_carries_delta() {
        let delta = TransformDelta {
            translation: glam::Vec3::new(1.0, 2.0, 3.0),
            ..Default::default()
        };
        let r = ManipResult::Update(delta);
        if let ManipResult::Update(d) = r {
            assert!((d.translation.x - 1.0).abs() < 1e-6);
        } else {
            panic!("expected Update variant");
        }
    }
}
