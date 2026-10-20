//! Standalone transform gizmo for viewport-lib scenes.
//!
//! The gizmo is CPU-only. It hit-tests, projects screen drags into world-space
//! transforms, and builds overlay items that the application submits to its own
//! renderer. It depends on `viewport-lib-types` and `glam`; it never touches the
//! renderer or wgpu.
//!
//! The crate is layered so you can use as much or as little as you need:
//!
//! 1. [`maths`]: the drag projections, screen-constant scale, pivot centre, the
//!    hit-tester, and the snap helpers. Build a fully custom gizmo from these
//!    plus an [`ActionFrame`](viewport_lib_types::input::ActionFrame).
//! 2. [`overlay`]: style and overlay builder. Turn gizmo state into overlay
//!    items in a chosen look.
//! 3. [`controller`]: the input to session to transform-delta loop with axis
//!    constraints and snapping, for the common case.
//!
//! The world up axis is `+Z`, matching viewport-lib.
//!
//! # Build your own gizmo
//!
//! The [`maths`] items re-exported at the crate root are a committed surface for
//! building a custom gizmo without the controller: project a screen drag onto an
//! axis, plane, or rotation ([`project_drag_onto_axis`], [`project_drag_onto_plane`],
//! [`project_drag_onto_rotation`], [`project_drag_onto_screen_plane`]), size the
//! gizmo for the screen ([`compute_gizmo_scale`]), resolve the pivot centre
//! ([`gizmo_center_for_pivot`], [`PivotMode`]), hit-test the handles ([`Gizmo`]),
//! turn a cursor into a world ray ([`cursor_ray`]), and snap a value
//! ([`snap_value`] and friends).

pub mod controller;
pub mod maths;
pub mod overlay;

// The viewport-lib-ui frontend: draw the gizmo as a Custom widget through the UI
// toolkit's draw path. Enabled by the `ui` feature; the default build does not
// pull viewport-lib-ui.
#[cfg(feature = "ui")]
pub mod ui;

// Layer 1: the build-your-own-gizmo maths surface.
pub use maths::gizmo::{
    Gizmo, GizmoAxis, GizmoMode, GizmoSpace, PivotMode, ROTATION_RING_RADIUS, compute_gizmo_scale,
    cursor_ray, gizmo_center_for_pivot, gizmo_center_from_selection, project_drag_onto_axis,
    project_drag_onto_plane, project_drag_onto_rotation, project_drag_onto_screen_plane,
};
pub use maths::snap::{
    ConstraintOverlay, SnapConfig, snap_angle, snap_scale, snap_value, snap_vec3,
};
pub use maths::solvers::{
    angular_rotation_from_cursor, constrained_scale, constrained_translation,
};
pub use maths::{NodeId, Selection};

// Layer 2: style and overlay builder.
pub use overlay::builder::{build_gizmo_overlays, build_numeric_label};
pub use overlay::style::{AxisColour, GizmoStyle, RotationHandles};
// The label-placement vocabulary the style uses, re-exported for convenience.
pub use viewport_lib_types::overlay::{AnchorX, AnchorY, OverlayOrigin};

// Layer 3: the controller and the per-frame driver.
pub use controller::{
    ActiveView, FrameInput, GizmoDriver, GizmoInfo, ManipResult, ManipulationContext,
    ManipulationController, ManipulationKind, ManipulationState, TransformDelta, ValueMode,
};
