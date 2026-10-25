//! What a gizmo pane is drawn and driven against in one frame, as the host hands it over.

use std::fmt;

use glam::{Quat, Vec3};
use viewport_lib_types::camera::Camera;
use viewport_lib_types::input::ActionFrame;

/// The camera, selection and resolved input a gizmo pane works with this frame, set on the
/// frame's host map:
///
/// ```ignore
/// let frame_context = GizmoFrame::new(camera)
///     .with_selection_centre(Some(centre))
///     .with_action_frame(action);
/// let map = HostMap::over(&shared).with_context(frame_context);
/// let frame = Frame::new(size, &measure, &registry)
///     .with_host(HostSources::default().with_map(&map));
/// ```
///
/// The map belongs to the frame, so a layout with several views builds one per view, each with
/// that view's camera, and every pane draws and picks through the camera of the frame it is in.
/// A frame with none set shows no gizmo.
#[derive(Clone, Default)]
#[non_exhaustive]
pub struct GizmoFrame {
    /// The camera the pane draws and picks through.
    pub camera: Camera,
    /// The world-space gizmo centre, or `None` to hide the gizmo.
    pub selection_centre: Option<Vec3>,
    /// The orientation the handles are drawn and picked in: identity for world space, the
    /// object's orientation for local space.
    pub space_orientation: Quat,
    /// The host's resolved input for the frame: the keyboard G / R / S, confirm and cancel, axis
    /// constraints and numeric entry the controller acts on. Only the update frame needs it.
    pub action_frame: ActionFrame,
}

impl GizmoFrame {
    /// A frame drawn through `camera`, with nothing selected, world space and no resolved input.
    pub fn new(camera: Camera) -> Self {
        GizmoFrame {
            camera,
            ..GizmoFrame::default()
        }
    }

    /// The same, with the gizmo centred on `centre`, or hidden for `None`.
    pub fn with_selection_centre(mut self, centre: Option<Vec3>) -> Self {
        self.selection_centre = centre;
        self
    }

    /// The same, with the handles drawn in `orientation`.
    pub fn with_space_orientation(mut self, orientation: Quat) -> Self {
        self.space_orientation = orientation;
        self
    }

    /// The same, with the host's resolved input for the frame.
    pub fn with_action_frame(mut self, action_frame: ActionFrame) -> Self {
        self.action_frame = action_frame;
        self
    }
}

/// Written out by hand because [`Camera`] is not `Debug`. Lists every field the gizmo is drawn
/// from, since a cached reader of the context compares frames by this form.
impl fmt::Debug for GizmoFrame {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let c = &self.camera;
        f.debug_struct("GizmoFrame")
            .field("projection", &c.projection)
            .field("center", &c.center)
            .field("distance", &c.distance)
            .field("orientation", &c.orientation)
            .field("fov_y", &c.fov_y)
            .field("aspect", &c.aspect)
            .field("znear", &c.znear)
            .field("zfar", &c.zfar)
            .field("selection_centre", &self.selection_centre)
            .field("space_orientation", &self.space_orientation)
            .field("action_frame", &self.action_frame)
            .finish()
    }
}
