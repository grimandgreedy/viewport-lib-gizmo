//! Convert the gizmo's projected overlay items into viewport-lib-ui draw commands.
//!
//! [`build_gizmo_overlays`](crate::build_gizmo_overlays) produces `viewport-lib-types`
//! overlay items (shapes and closed polylines). The UI toolkit draws from its own
//! `DrawCommand` vocabulary instead, so this module maps one to the other, field by
//! field, and preserves each item's projected draw order through the toolkit's
//! within-layer depth (`DrawList::push_at_depth`), so the near handle still draws
//! over the far one.
//!
//! The mapping is exact for what the gizmo emits: capsule shafts and cone heads
//! become `Shape` commands with the same rotation and gradient, and the rings,
//! plane quads, and cube faces become closed `Polyline` commands with the same
//! solid fill. It is renderer-free and pure, so it is unit-tested on its own.

use viewport_lib_types::colour::Colour as OverlayColour;
use viewport_lib_types::overlay::{
    LineJoin as OverlayJoin, OverlayFill, OverlayPolylineItem, OverlayShape, OverlayShapeItem,
    PolylineCap, TriangleDirection,
};

use viewport_lib_ui_core::{DrawBorder, DrawCommand, DrawFill, DrawList, DrawStop, Rect};
use viewport_lib_ui_document::{Colour, LineCap, LineJoin, ShapeDir, WidgetShape};

/// Convert one gizmo shape (an arrow shaft or a cone head) into a draw command and
/// its within-layer depth. The depth is the item's `z_order`, so ordering matches
/// the direct-submission path: a higher value draws on top.
pub fn shape_command(item: &OverlayShapeItem) -> (DrawCommand, i32) {
    let command = DrawCommand::Shape {
        rect: Rect::new(item.transform.translate, item.size),
        shape: widget_shape(&item.shape),
        fill: draw_fill(&item.style.fill),
        border: DrawBorder::default(),
        shadows: Vec::new(),
        inner_shadows: Vec::new(),
        // Overlay rotation is radians about the box centre; the toolkit takes
        // degrees, and a centre pivot is `None`.
        rotation_deg: item.transform.rotation.to_degrees(),
        rotation_pivot: None,
        backdrop_blur: 0.0,
        animations: None,
    };
    (command, item.z_order)
}

/// Convert one gizmo polyline (a ring, plane quad, cube face, or centre handle)
/// into a draw command and its within-layer depth.
pub fn polyline_command(item: &OverlayPolylineItem) -> (DrawCommand, i32) {
    // A polyline's stroke is one field now, and `None` is a fill-only path.
    let stroke = item.stroke.as_ref();
    let command = DrawCommand::Polyline {
        points: item.points.clone(),
        width: stroke.map_or(0.0, |s| s.width),
        colour: stroke.map_or([0.0; 4], |s| arr(s.colour)),
        join: line_join(stroke.map_or(OverlayJoin::Mitre, |s| s.join)),
        cap: line_cap(stroke.map_or(PolylineCap::Butt, |s| s.cap)),
        closed: item.closed,
        // The toolkit fills a closed polyline with a solid colour. The gizmo only
        // solid-fills its polylines; a gradient fill would fall back to its first
        // colour, which the gizmo never hits.
        fill: fill_colour_of(&item.style.fill),
    };
    (command, item.z_order)
}

/// Push a whole gizmo's overlay items into a `DrawList`, each at its projected
/// depth. Call this from a custom widget's `paint` after running
/// [`build_gizmo_overlays`](crate::build_gizmo_overlays).
pub fn push_gizmo_overlays(
    list: &mut DrawList,
    shapes: &[OverlayShapeItem],
    polylines: &[OverlayPolylineItem],
) {
    for item in shapes {
        let (command, depth) = shape_command(item);
        list.push_at_depth(command, depth);
    }
    for item in polylines {
        let (command, depth) = polyline_command(item);
        list.push_at_depth(command, depth);
    }
}

/// A `viewport-lib-types` colour as the toolkit's `[r, g, b, a]`.
///
/// The gizmo's overlay colours are linear (viewport-lib takes bare arrays as linear
/// and the renderer encodes them on write). The toolkit's `DrawCommand` colours are
/// sRGB display values that its lowering decodes back to linear. So encode to sRGB
/// here, and the round trip lands on the same linear colour the direct overlay path
/// submits, keeping this frontend's colours identical to the renderer-direct one.
fn arr(colour: OverlayColour) -> Colour {
    let [r, g, b, a]: [f32; 4] = colour.into();
    [linear_to_srgb(r), linear_to_srgb(g), linear_to_srgb(b), a]
}

/// Encode one linear channel to sRGB. Alpha is left alone.
fn linear_to_srgb(c: f32) -> f32 {
    if c <= 0.003_130_8 {
        12.92 * c
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

fn gradient_stop(t: f32, colour: OverlayColour) -> DrawStop {
    DrawStop {
        t,
        colour: arr(colour),
    }
}

fn widget_shape(shape: &OverlayShape) -> WidgetShape {
    match shape {
        OverlayShape::Capsule => WidgetShape::Capsule,
        OverlayShape::Ellipse => WidgetShape::Ellipse,
        OverlayShape::Circle => WidgetShape::Circle,
        OverlayShape::Triangle { direction } => WidgetShape::Triangle {
            direction: shape_dir(*direction),
        },
        OverlayShape::Rect { corner_radius } => {
            if *corner_radius <= 0.0 {
                WidgetShape::Rect
            } else {
                WidgetShape::RoundedRect {
                    top_left: *corner_radius,
                    top_right: *corner_radius,
                    bottom_right: *corner_radius,
                    bottom_left: *corner_radius,
                }
            }
        }
        OverlayShape::Ring { inner_radius_frac } => WidgetShape::Ring {
            inner_frac: *inner_radius_frac,
        },
        // The gizmo emits only the shapes above. Anything else falls back to a
        // plain rectangle rather than silently dropping the item.
        _ => WidgetShape::Rect,
    }
}

fn shape_dir(direction: TriangleDirection) -> ShapeDir {
    match direction {
        TriangleDirection::Up => ShapeDir::Up,
        TriangleDirection::Down => ShapeDir::Down,
        TriangleDirection::Left => ShapeDir::Left,
        TriangleDirection::Right => ShapeDir::Right,
    }
}

fn draw_fill(fill: &OverlayFill) -> DrawFill {
    match fill {
        OverlayFill::Solid(colour) => DrawFill::Solid(arr(*colour)),
        OverlayFill::LinearGradient {
            start_colour,
            end_colour,
            angle,
        } => DrawFill::LinearGradient {
            stops: vec![
                gradient_stop(0.0, *start_colour),
                gradient_stop(1.0, *end_colour),
            ],
            angle_deg: angle.to_degrees(),
        },
        OverlayFill::RadialGradient {
            centre_colour,
            edge_colour,
        } => DrawFill::RadialGradient {
            stops: vec![
                gradient_stop(0.0, *centre_colour),
                gradient_stop(1.0, *edge_colour),
            ],
        },
        OverlayFill::ConicalGradient {
            start_colour,
            end_colour,
            offset_angle,
        } => DrawFill::ConicalGradient {
            stops: vec![
                gradient_stop(0.0, *start_colour),
                gradient_stop(1.0, *end_colour),
            ],
            offset_angle_deg: offset_angle.to_degrees(),
        },
        // Multi-stop and any future fills are not emitted by the gizmo; fall back
        // to opaque white so a stray item is visible rather than invisible.
        _ => DrawFill::Solid([1.0, 1.0, 1.0, 1.0]),
    }
}

/// The solid interior colour for a closed polyline's fill.
/// The solid colour a closed gizmo polyline is filled with, or `None` when it has no fill.
/// A fill is no longer optional on the item: an unfilled one carries a fully transparent
/// solid, which `is_set` is how you ask about.
fn fill_colour_of(fill: &OverlayFill) -> Option<Colour> {
    fill.is_set().then(|| fill_colour(fill))
}

fn fill_colour(fill: &OverlayFill) -> Colour {
    match fill {
        OverlayFill::Solid(colour) => arr(*colour),
        OverlayFill::LinearGradient { start_colour, .. }
        | OverlayFill::ConicalGradient { start_colour, .. } => arr(*start_colour),
        OverlayFill::RadialGradient { centre_colour, .. } => arr(*centre_colour),
        _ => [1.0, 1.0, 1.0, 1.0],
    }
}

fn line_join(join: OverlayJoin) -> LineJoin {
    match join {
        OverlayJoin::Mitre => LineJoin::Mitre,
        OverlayJoin::Bevel => LineJoin::Bevel,
    }
}

fn line_cap(cap: PolylineCap) -> LineCap {
    match cap {
        PolylineCap::Round => LineCap::Round,
        // The toolkit has no butt cap; a flat cut maps to square. Closed gizmo
        // polylines ignore caps anyway.
        PolylineCap::Butt | PolylineCap::Square => LineCap::Square,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Colours compare with a tolerance: the sRGB encode is not bit-exact even for
    /// 1.0 (it rounds to 0.99999994).
    fn close(a: [f32; 4], b: [f32; 4]) -> bool {
        a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-3)
    }

    #[test]
    fn capsule_shaft_becomes_a_rotated_gradient_shape_at_its_depth() {
        let item = OverlayShapeItem::new(OverlayShape::Capsule, [10.0, 20.0], [40.0, 6.0])
            .with_rotation(std::f32::consts::FRAC_PI_2)
            .with_fill(OverlayFill::LinearGradient {
                // 0.0 and 1.0 are invariant under the sRGB encode, so the assert stays exact.
                start_colour: [1.0, 0.0, 0.0, 1.0].into(),
                end_colour: [0.0, 0.0, 0.0, 1.0].into(),
                angle: 0.0,
            })
            .with_z_order(1234);

        let (command, depth) = shape_command(&item);
        assert_eq!(depth, 1234, "depth carries the item's z_order");

        match command {
            DrawCommand::Shape {
                shape,
                fill,
                rotation_deg,
                rect,
                ..
            } => {
                assert!(matches!(shape, WidgetShape::Capsule));
                assert!((rect.width() - 40.0).abs() < 1e-3);
                assert!((rect.height() - 6.0).abs() < 1e-3);
                assert!((rotation_deg - 90.0).abs() < 1e-3, "radians become degrees");
                match fill {
                    DrawFill::LinearGradient { stops, angle_deg } => {
                        assert_eq!(stops.len(), 2);
                        assert!(close(stops[0].colour, [1.0, 0.0, 0.0, 1.0]));
                        assert!(close(stops[1].colour, [0.0, 0.0, 0.0, 1.0]));
                        assert_eq!(angle_deg, 0.0);
                    }
                    other => panic!("expected a linear gradient, got {other:?}"),
                }
            }
            other => panic!("expected a shape, got {other:?}"),
        }
    }

    #[test]
    fn ellipse_cone_base_keeps_its_radial_gradient() {
        let item = OverlayShapeItem::new(OverlayShape::Ellipse, [0.0, 0.0], [12.0, 20.0])
            .with_fill(OverlayFill::RadialGradient {
                centre_colour: [1.0, 1.0, 1.0, 1.0].into(),
                edge_colour: [0.0, 0.0, 0.0, 1.0].into(),
            });

        let (command, _) = shape_command(&item);
        match command {
            DrawCommand::Shape {
                shape: WidgetShape::Ellipse,
                fill: DrawFill::RadialGradient { stops },
                ..
            } => {
                assert_eq!(stops.len(), 2);
                assert!(close(stops[0].colour, [1.0, 1.0, 1.0, 1.0]));
                assert!(close(stops[1].colour, [0.0, 0.0, 0.0, 1.0]));
            }
            other => panic!("expected an ellipse with a radial gradient, got {other:?}"),
        }
    }

    #[test]
    fn triangle_direction_maps_across() {
        let item = OverlayShapeItem::new(
            OverlayShape::Triangle {
                direction: TriangleDirection::Up,
            },
            [0.0, 0.0],
            [10.0, 10.0],
        )
        .with_fill(OverlayFill::Solid([1.0, 1.0, 1.0, 1.0].into()));

        match shape_command(&item).0 {
            DrawCommand::Shape {
                shape: WidgetShape::Triangle { direction },
                ..
            } => assert_eq!(direction, ShapeDir::Up),
            other => panic!("expected a triangle, got {other:?}"),
        }
    }

    #[test]
    fn closed_solid_polyline_becomes_a_filled_polyline_at_its_depth() {
        let item = OverlayPolylineItem::new(vec![[0.0, 0.0], [10.0, 0.0], [10.0, 10.0]])
            .with_closed(true)
            .with_fill(OverlayFill::Solid([1.0, 0.0, 0.0, 1.0].into()))
            .with_colour([1.0, 1.0, 1.0, 1.0])
            .with_thickness(2.0)
            .with_z_order(42);

        let (command, depth) = polyline_command(&item);
        assert_eq!(depth, 42);
        match command {
            DrawCommand::Polyline {
                points,
                width,
                colour,
                closed,
                fill,
                ..
            } => {
                assert_eq!(points.len(), 3);
                assert_eq!(width, 2.0);
                assert!(close(colour, [1.0, 1.0, 1.0, 1.0]), "the stroke colour");
                assert!(closed);
                assert!(
                    fill.is_some_and(|f| close(f, [1.0, 0.0, 0.0, 1.0])),
                    "the solid interior"
                );
            }
            other => panic!("expected a polyline, got {other:?}"),
        }
    }

    #[test]
    fn an_open_unfilled_polyline_has_no_fill() {
        let item = OverlayPolylineItem::new(vec![[0.0, 0.0], [5.0, 5.0]]);
        match polyline_command(&item).0 {
            DrawCommand::Polyline { closed, fill, .. } => {
                assert!(!closed);
                assert_eq!(fill, None);
            }
            other => panic!("expected a polyline, got {other:?}"),
        }
    }

    #[test]
    fn linear_colours_are_encoded_to_srgb() {
        // A mid grey the gizmo submits as linear must reach the toolkit as its sRGB
        // value, so the facade's decode lands back on the original linear colour.
        let item = OverlayShapeItem::new(OverlayShape::Ellipse, [0.0, 0.0], [4.0, 4.0])
            .with_fill(OverlayFill::Solid([0.5, 0.5, 0.5, 0.5].into()));
        match shape_command(&item).0 {
            DrawCommand::Shape {
                fill: DrawFill::Solid([r, g, b, a]),
                ..
            } => {
                assert!(
                    (r - 0.735).abs() < 0.01,
                    "0.5 linear encodes to ~0.735 sRGB, got {r}"
                );
                assert_eq!([g, b], [r, r], "channels encode identically");
                assert_eq!(a, 0.5, "alpha is not encoded");
            }
            other => panic!("expected a solid shape, got {other:?}"),
        }
    }

    #[test]
    fn depth_order_follows_z_order() {
        // A far item and a near item, near given the higher z_order. The near one
        // must convert to the higher depth so it draws on top.
        let far = OverlayShapeItem::new(OverlayShape::Capsule, [0.0, 0.0], [10.0, 4.0])
            .with_fill(OverlayFill::Solid([1.0, 0.0, 0.0, 1.0].into()))
            .with_z_order(100);
        let near = OverlayShapeItem::new(OverlayShape::Capsule, [0.0, 0.0], [10.0, 4.0])
            .with_fill(OverlayFill::Solid([0.0, 1.0, 0.0, 1.0].into()))
            .with_z_order(200);

        assert!(shape_command(&near).1 > shape_command(&far).1);
    }
}
