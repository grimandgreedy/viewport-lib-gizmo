//! Draw the gizmo through viewport-lib-ui's toolkit instead of submitting overlay
//! items to a renderer directly.
//!
//! This frontend is for consumers already using viewport-lib-ui. It converts the
//! gizmo's projected overlay items into the toolkit's `DrawCommand` vocabulary so
//! the handles draw in the same pass as the rest of the UI, layer with the
//! panels, and arbitrate the pointer through one capture signal. The 3D maths,
//! the camera, and [`build_gizmo_overlays`](crate::build_gizmo_overlays) stay in
//! the core crate: this module only translates their output.
//!
//! It depends only on `viewport-lib-ui-core` and `viewport-lib-ui-document`, both
//! renderer-free, so enabling the `ui` feature does not pull viewport-lib or wgpu.
//!
//! The base frontend ([`build_gizmo_overlays`](crate::build_gizmo_overlays), which
//! produces `viewport-lib-types` overlay items) is unchanged and remains the path
//! for consumers who submit to a renderer without the UI toolkit.

pub mod convert;

pub use convert::{polyline_command, push_gizmo_overlays, shape_command};
