//! Controller: the input to session to transform-delta loop.
//!
//! `ManipulationController` runs move, rotate, and scale sessions with axis
//! constraints and snapping. The default most embedders use.

pub mod types;

mod session;
