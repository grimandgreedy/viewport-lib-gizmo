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

pub mod controller;
pub mod maths;
pub mod overlay;

pub use maths::snap::{
    ConstraintOverlay, SnapConfig, snap_angle, snap_scale, snap_value, snap_vec3,
};
