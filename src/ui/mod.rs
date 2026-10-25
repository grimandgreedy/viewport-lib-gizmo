//! Draw the gizmo through viewport-lib-ui's toolkit instead of submitting overlay
//! items to a renderer directly.
//!
//! This frontend is for consumers already using viewport-lib-ui. It hands the
//! gizmo's projected overlay items to the toolkit's draw list as they are, so the
//! handles draw in the same pass as the rest of the UI, layer with the panels, and
//! arbitrate the pointer through one capture signal. The 3D maths, the camera, and
//! [`build_gizmo_overlays`](crate::build_gizmo_overlays) stay in the core crate:
//! this module only places their output.
//!
//! The widget is written against `viewport-lib-ui-extend`, and the readout panel
//! against `viewport-lib-ui-document`. Both are renderer-free, so enabling the `ui`
//! feature does not pull viewport-lib or wgpu.
//!
//! The base frontend ([`build_gizmo_overlays`](crate::build_gizmo_overlays), which
//! produces `viewport-lib-types` overlay items) is unchanged and remains the path
//! for consumers who submit to a renderer without the UI toolkit.

mod frame;
pub mod readout;
pub mod widget;

pub use frame::GizmoFrame;
pub use readout::{
    ObjectPose, READOUT_BADGE_FONT, READOUT_CORNER_RADIUS, READOUT_PADDING, READOUT_SEGMENTED_FONT,
    READOUT_SPACING, ReadoutIds, mode_badge, mode_tag, readout_field, readout_panel,
    readout_values, readout_values_in, set_mode_badge, set_readout, value_mode_from_index,
    value_mode_index, value_mode_segmented,
};
pub use widget::{GIZMO_KIND, GizmoDelta, GizmoResult, GizmoTool, ReadoutWhen, output_schema};
