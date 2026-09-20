//! `GizmoStyle`: colours, proportions, and layering for the gizmo look.
//!
//! [`build_gizmo_overlays`](super::builder::build_gizmo_overlays) takes a
//! `&GizmoStyle` and draws the handles in that look. [`GizmoStyle::default`]
//! reproduces the standard viewport-lib appearance; [`GizmoStyle::thin`] and
//! [`GizmoStyle::bold`] are ready-made variations. A consumer can also fill in
//! the fields directly.
//!
//! The geometry fields (shaft length, plane offset and size, ring radius) default
//! to the same values [`Gizmo::hit_test`](crate::maths::gizmo::Gizmo::hit_test)
//! uses, so the default look is drawn exactly where it is picked. Changing those
//! fields in a custom style changes only what is drawn, not what is picked.

use crate::maths::gizmo::{self, GizmoAxis};
use viewport_lib_types::overlay::{Alignment, AnchorX, AnchorY, OverlayOrigin};

/// Base and hover colours for one axis (RGBA, alpha significant).
#[derive(Clone, Copy, Debug)]
pub struct AxisColour {
    /// Colour when the axis is not hovered.
    pub base: [f32; 4],
    /// Colour when the axis is hovered.
    pub hover: [f32; 4],
}

/// How the rotation handles are drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum RotationHandles {
    /// Three orthogonal world-space rings, one per axis.
    Rings,
}

/// Colours, proportions, and layering for one gizmo look.
///
/// Build from [`default`](Self::default) (or a preset) and adjust fields; the
/// struct is `#[non_exhaustive]`, so construct it that way rather than with a
/// struct literal.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct GizmoStyle {
    /// X axis colour (red by default).
    pub x: AxisColour,
    /// Y axis colour (green by default).
    pub y: AxisColour,
    /// Z axis colour (blue by default).
    pub z: AxisColour,
    /// Screen-space centre handle colour.
    pub screen: AxisColour,
    /// Plane-handle fill opacity when not hovered.
    pub plane_alpha: f32,
    /// Plane-handle fill opacity when hovered.
    pub plane_alpha_hover: f32,
    /// Brightness multiplier applied to a hovered plane handle's blended colour.
    pub plane_hover_brightness: f32,

    /// Arrow shaft radius in gizmo-local units.
    pub shaft_radius: f32,
    /// Arrow shaft length in gizmo-local units.
    pub shaft_length: f32,
    /// Minimum drawn shaft thickness in pixels.
    pub shaft_min_px: f32,
    /// Shaft gradient highlight factor.
    pub shaft_grad_hi: f32,
    /// Shaft gradient shadow factor.
    pub shaft_grad_lo: f32,
    /// How far the shaft tucks under the head, as a fraction of the head vector.
    pub shaft_undertuck: f32,

    /// Cone head base radius in gizmo-local units.
    pub cone_radius: f32,
    /// Cone head length in gizmo-local units.
    pub cone_length: f32,
    /// Minimum drawn cone-head width in pixels.
    pub cone_head_min_px: f32,
    /// Minimum cone base ellipse minor axis, as a fraction of the head width.
    pub cone_minor_floor: f32,
    /// Cone base disc centre brightness factor.
    pub cone_base_grad_hi: f32,
    /// Cone base disc edge brightness factor.
    pub cone_base_grad_lo: f32,
    /// Cone side highlight factor.
    pub cone_side_grad_hi: f32,
    /// Cone side shadow factor.
    pub cone_side_grad_lo: f32,
    /// Scale cube half-extent in gizmo-local units.
    pub cube_half: f32,
    /// Ambient floor for cube-face shading (the constant term in `ambient + (1 - ambient) * facing`).
    pub cube_shade_ambient: f32,
    /// Cube face outline darkening factor.
    pub cube_stroke_scale: f32,

    /// Distance from the gizmo centre to a plane handle, in gizmo-local units.
    pub plane_offset: f32,
    /// Plane handle half-extent in gizmo-local units.
    pub plane_size: f32,
    /// Plane handle outline thickness in pixels.
    pub plane_stroke_width: f32,

    /// How the rotation handles are drawn.
    pub rotation: RotationHandles,
    /// Rotation ring major radius in gizmo-local units.
    pub ring_radius: f32,
    /// Rotation ring tube (minor) radius in gizmo-local units.
    pub ring_tube_radius: f32,
    /// Minimum drawn ring thickness in pixels.
    pub ring_min_px: f32,
    /// Number of segments used to draw each ring.
    pub ring_segments: usize,

    /// Drawn half-extent of the screen-space centre handle, in gizmo-local units.
    pub screen_handle_size: f32,
    /// Centre handle outline colour.
    pub screen_handle_stroke: [f32; 4],
    /// Centre handle outline thickness in pixels.
    pub screen_stroke_width: f32,

    /// Base added to every gizmo overlay z-order. Raise it to draw the gizmo over
    /// other overlays, lower it to draw the gizmo under them.
    pub z_base: i32,

    /// Where the numeric readout sits (see
    /// [`build_numeric_label`](super::builder::build_numeric_label)).
    /// `OverlayOrigin::World(_)` follows the gizmo centre (the placeholder
    /// position is replaced with the live centre at build time);
    /// `OverlayOrigin::Viewport(_)` pins it to a screen edge or corner.
    pub label_anchor: OverlayOrigin,
    /// Horizontal alignment of the readout box relative to its anchor.
    pub label_align_x: AnchorX,
    /// Vertical alignment of the readout box relative to its anchor.
    pub label_align_y: AnchorY,
    /// Readout offset from its anchor, in pixels, applied after alignment.
    pub label_offset: [f32; 2],
    /// Numeric-readout font size.
    pub label_size: f32,
    /// Numeric-readout colour.
    pub label_colour: [f32; 4],
    /// Width of a dark contour hugging the readout's glyphs, in pixels. `0.0` draws none.
    ///
    /// The readout floats over whatever the scene happens to be, so a colour that reads
    /// against one background fails against another. A contour holds contrast against
    /// both, which a flat backing panel behind the text does less well at this size.
    pub label_contour: f32,
}

impl Default for GizmoStyle {
    fn default() -> Self {
        Self {
            x: AxisColour {
                base: [0.75, 0.06, 0.07, 1.0],
                hover: [0.975, 0.078, 0.091, 1.0],
            },
            y: AxisColour {
                base: [0.18, 0.55, 0.03, 1.0],
                hover: [0.234, 0.715, 0.039, 1.0],
            },
            z: AxisColour {
                base: [0.05, 0.27, 0.72, 1.0],
                hover: [0.065, 0.351, 0.936, 1.0],
            },
            screen: AxisColour {
                base: [0.9, 0.9, 0.9, 0.6],
                hover: [1.0, 1.0, 1.0, 0.8],
            },
            plane_alpha: 0.3,
            plane_alpha_hover: 0.5,
            plane_hover_brightness: 1.3,

            shaft_radius: gizmo::SHAFT_RADIUS,
            shaft_length: gizmo::SHAFT_LENGTH,
            shaft_min_px: 2.0,
            shaft_grad_hi: 1.2,
            shaft_grad_lo: 0.65,
            shaft_undertuck: 0.35,

            cone_radius: gizmo::CONE_RADIUS,
            cone_length: gizmo::CONE_LENGTH,
            cone_head_min_px: 4.0,
            cone_minor_floor: 0.12,
            cone_base_grad_hi: 1.2,
            cone_base_grad_lo: 0.55,
            cone_side_grad_hi: 1.3,
            cone_side_grad_lo: 0.6,
            cube_half: gizmo::CUBE_HALF,
            cube_shade_ambient: 0.5,
            cube_stroke_scale: 0.4,

            plane_offset: gizmo::PLANE_OFFSET,
            plane_size: gizmo::PLANE_SIZE,
            plane_stroke_width: 1.0,

            rotation: RotationHandles::Rings,
            ring_radius: gizmo::ROTATION_RING_RADIUS,
            ring_tube_radius: gizmo::RING_TUBE_RADIUS,
            ring_min_px: 2.0,
            ring_segments: 48,

            screen_handle_size: gizmo::SCREEN_HANDLE_SIZE,
            screen_handle_stroke: [1.0, 1.0, 1.0, 1.0],
            screen_stroke_width: 1.0,

            z_base: 0,

            // Bottom-centre of the viewport by default, lifted off the edge. Set
            // a World anchor instead to float the readout at the gizmo.
            label_anchor: OverlayOrigin::Viewport(Alignment::new(
                AnchorX::Middle,
                AnchorY::Bottom,
            )),
            label_align_x: AnchorX::Middle,
            label_align_y: AnchorY::Bottom,
            label_offset: [0.0, -24.0],
            label_size: 18.0,
            label_colour: [0.95, 0.95, 0.95, 1.0],
            label_contour: 2.0,
        }
    }
}

impl GizmoStyle {
    /// A thin, flat look: slimmer shafts and tubes, small handles, no gradient
    /// shading. Suits a CAD or technical viewport. The handle layout (ring radius,
    /// plane offset and size) is left at the default so picking still lines up.
    pub fn thin() -> Self {
        Self {
            shaft_radius: 0.02,
            shaft_min_px: 1.0,
            shaft_grad_hi: 1.0,
            shaft_grad_lo: 1.0,
            cone_radius: 0.06,
            cone_head_min_px: 2.0,
            cone_base_grad_hi: 1.0,
            cone_base_grad_lo: 1.0,
            cone_side_grad_hi: 1.0,
            cone_side_grad_lo: 1.0,
            cube_shade_ambient: 1.0,
            ring_tube_radius: 0.012,
            ring_min_px: 1.0,
            ..Self::default()
        }
    }

    /// A bold, solid look: thicker shafts and tubes, larger handles, stronger
    /// shading. Suits a DCC or game-editor viewport. The handle layout is left at
    /// the default so picking still lines up.
    pub fn bold() -> Self {
        Self {
            shaft_radius: 0.05,
            shaft_min_px: 3.0,
            shaft_grad_hi: 1.3,
            shaft_grad_lo: 0.5,
            cone_radius: 0.12,
            cone_head_min_px: 6.0,
            cube_half: 0.08,
            plane_alpha: 0.4,
            plane_alpha_hover: 0.6,
            ring_tube_radius: 0.04,
            ring_min_px: 3.0,
            ..Self::default()
        }
    }

    /// Base or hover colour for an axis, given the hovered axis.
    pub fn axis_colour(&self, axis: GizmoAxis, hovered: GizmoAxis) -> [f32; 4] {
        let is_hovered = axis == hovered;
        let pick = |c: &AxisColour| if is_hovered { c.hover } else { c.base };
        match axis {
            GizmoAxis::X => pick(&self.x),
            GizmoAxis::Y => pick(&self.y),
            GizmoAxis::Z => pick(&self.z),
            GizmoAxis::Screen => pick(&self.screen),
            _ => [1.0; 4],
        }
    }

    /// Colour for a plane handle, blending the two axis base colours. On hover the
    /// blend is brightened by [`plane_hover_brightness`](Self::plane_hover_brightness)
    /// in addition to the alpha bump.
    pub fn plane_colour(&self, axis: GizmoAxis, hovered: GizmoAxis) -> [f32; 4] {
        let is_hovered = axis == hovered;
        let alpha = if is_hovered {
            self.plane_alpha_hover
        } else {
            self.plane_alpha
        };
        let brightness = if is_hovered {
            self.plane_hover_brightness
        } else {
            1.0
        };
        let (c1, c2) = match axis {
            GizmoAxis::XY => (self.x.base, self.y.base),
            GizmoAxis::XZ => (self.x.base, self.z.base),
            GizmoAxis::YZ => (self.y.base, self.z.base),
            _ => return [1.0, 1.0, 1.0, alpha],
        };
        [
            ((c1[0] + c2[0]) * 0.5 * brightness).min(1.0),
            ((c1[1] + c2[1]) * 0.5 * brightness).min(1.0),
            ((c1[2] + c2[2]) * 0.5 * brightness).min(1.0),
            alpha,
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_axis_colours_match_the_standard_look() {
        let s = GizmoStyle::default();
        assert_eq!(s.axis_colour(GizmoAxis::X, GizmoAxis::None), [0.75, 0.06, 0.07, 1.0]);
        assert_eq!(s.axis_colour(GizmoAxis::X, GizmoAxis::X), [0.975, 0.078, 0.091, 1.0]);
        assert_eq!(s.axis_colour(GizmoAxis::Screen, GizmoAxis::None), [0.9, 0.9, 0.9, 0.6]);
    }

    #[test]
    fn default_plane_colour_blends_and_brightens_on_hover() {
        let s = GizmoStyle::default();
        let base = s.plane_colour(GizmoAxis::XY, GizmoAxis::None);
        assert_eq!(base[3], 0.3);
        assert!((base[0] - (0.75 + 0.18) * 0.5).abs() < 1e-6);
        let hov = s.plane_colour(GizmoAxis::XY, GizmoAxis::XY);
        assert_eq!(hov[3], 0.5);
        assert!(hov[0] > base[0], "hover brightens the blend");
    }

    #[test]
    fn default_geometry_tracks_the_hit_test_constants() {
        let s = GizmoStyle::default();
        assert_eq!(s.ring_radius, gizmo::ROTATION_RING_RADIUS);
        assert_eq!(s.plane_offset, gizmo::PLANE_OFFSET);
        assert_eq!(s.plane_size, gizmo::PLANE_SIZE);
        assert_eq!(s.shaft_length, gizmo::SHAFT_LENGTH);
    }

    #[test]
    fn presets_keep_the_pickable_layout() {
        // thin and bold must not move the handle layout, or drawing and picking
        // would disagree.
        for s in [GizmoStyle::thin(), GizmoStyle::bold()] {
            assert_eq!(s.ring_radius, gizmo::ROTATION_RING_RADIUS);
            assert_eq!(s.plane_offset, gizmo::PLANE_OFFSET);
            assert_eq!(s.plane_size, gizmo::PLANE_SIZE);
            assert_eq!(s.shaft_length, gizmo::SHAFT_LENGTH);
        }
    }
}
