//! Maths toolkit: pure functions over camera, cursor, and glam.
//!
//! Hit-testing, screen-constant scale, drag projection, pivot centres, and the
//! snap helpers. Build a fully custom gizmo from these plus an
//! [`ActionFrame`](viewport_lib_types::input::ActionFrame), with no controller
//! and no fixed look.

pub mod gizmo;
pub mod snap;
pub mod solvers;

mod selection;
