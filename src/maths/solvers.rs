//! Pure math solver functions for manipulation transforms.
//!
//! All functions are stateless. They translate raw input (pointer delta, camera,
//! axis constraint) into world-space transform components that the app can apply directly.

use crate::maths::gizmo::{GizmoAxis, project_drag_onto_axis};
use viewport_lib_types::camera::Camera;

/// Compute the signed rotation angle (radians) produced by the cursor sweeping around the
/// screen-projected gizmo center.
///
/// Returns 0.0 when the cursor is within `MIN_RADIUS` pixels of the center, or when
/// `cursor_viewport` is `None`.
///
/// # Arguments
/// * `cursor_viewport` : current cursor position in viewport-local pixels (Y-down).
/// * `pointer_delta` : mouse movement since last frame in pixels.
/// * `gizmo_center` : world-space rotation pivot.
/// * `axis_world` : world-space rotation axis (unit vector).
/// * `view_proj` : camera view-projection matrix.
/// * `viewport_size` : viewport dimensions in pixels.
/// * `camera_view` : camera view matrix (used to orient the angle sign).
pub fn angular_rotation_from_cursor(
    cursor_viewport: Option<glam::Vec2>,
    pointer_delta: glam::Vec2,
    gizmo_center: glam::Vec3,
    axis_world: glam::Vec3,
    view_proj: glam::Mat4,
    viewport_size: glam::Vec2,
    camera_view: glam::Mat4,
) -> f32 {
    const MIN_RADIUS: f32 = 10.0;

    let cursor = match cursor_viewport {
        Some(c) => c,
        None => return 0.0,
    };

    // Project gizmo center into viewport pixel space (Y-down).
    let ndc = view_proj.project_point3(gizmo_center);
    let center_screen = glam::Vec2::new(
        (ndc.x + 1.0) * 0.5 * viewport_size.x,
        (1.0 - ndc.y) * 0.5 * viewport_size.y,
    );

    let r_curr = cursor - center_screen;
    let r_prev = r_curr - pointer_delta;

    if r_curr.length() < MIN_RADIUS || r_prev.length() < MIN_RADIUS {
        return 0.0;
    }

    // Signed angle from r_prev to r_curr in screen space (Y-down).
    // Positive cross2d = CW visual rotation (because screen Y is down).
    let cross2d = r_prev.x * r_curr.y - r_prev.y * r_curr.x;
    let dot = r_prev.dot(r_curr);
    let screen_angle = cross2d.atan2(dot);

    // Correct for axis orientation relative to camera.
    // In a look_at_rh view matrix, camera +Z points backward (away from scene),
    // so axis_z_cam > 0 means the axis points toward the camera.
    // For a toward-camera axis, RH positive rotation = CCW from camera = CCW visual.
    // In Y-down screen space, CCW visual -> negative cross2d -> negative screen_angle,
    // so we negate to recover the positive world angle.
    let axis_z_cam = (camera_view * axis_world.extend(0.0)).z;
    if axis_z_cam >= 0.0 {
        -screen_angle
    } else {
        screen_angle
    }
}

/// Compute the world-space translation vector from a pointer delta given an axis constraint.
///
/// # Arguments
/// * `pointer_delta` : mouse movement in pixels since last frame.
/// * `axis` : optional axis constraint. `None` = free camera-plane movement.
/// * `exclude_axis` : if `true`, `axis` names the axis to *exclude*; movement is in the perpendicular plane.
/// * `gizmo_center` : world-space pivot (used for axis projection).
/// * `camera` : current camera (provides right, up, distance, fov_y).
/// * `viewport_size` : viewport dimensions in pixels.
pub fn constrained_translation(
    pointer_delta: glam::Vec2,
    axis: Option<GizmoAxis>,
    exclude_axis: bool,
    gizmo_center: glam::Vec3,
    camera: &Camera,
    viewport_size: glam::Vec2,
) -> glam::Vec3 {
    let pan_scale = camera.view_height_at(camera.focus_distance()) / viewport_size.y.max(1.0);
    let camera_right = camera.right();
    let camera_up = camera.up();

    let camera_view = camera.view_matrix();
    let view_proj = camera.proj_matrix() * camera_view;

    match axis {
        None => {
            // Free: move in the camera plane.
            camera_right * pointer_delta.x * pan_scale - camera_up * pointer_delta.y * pan_scale
        }
        Some(ax) => {
            if exclude_axis {
                // Move in camera plane, then zero the excluded axis component.
                let mut world_delta = camera_right * pointer_delta.x * pan_scale
                    - camera_up * pointer_delta.y * pan_scale;
                match ax {
                    GizmoAxis::X => world_delta.x = 0.0,
                    GizmoAxis::Y => world_delta.y = 0.0,
                    GizmoAxis::Z | GizmoAxis::None => world_delta.z = 0.0,
                    _ => world_delta.z = 0.0,
                }
                world_delta
            } else {
                // Constrained to a single axis.
                let axis_world = gizmo_axis_to_vec3(ax);
                let amount = project_drag_onto_axis(
                    pointer_delta,
                    axis_world,
                    view_proj,
                    gizmo_center,
                    viewport_size,
                );
                axis_world * amount
            }
        }
    }
}

/// The world translation that keeps the grabbed point under the cursor as it
/// moves from `from` to `to` (viewport pixels). With `from` the cursor at the
/// start of the drag, this is the drag's total translation.
///
/// The point is held to the same constraint as [`constrained_translation`]: the
/// axis line through `gizmo_center`, the plane through it whose normal is the
/// excluded axis, or (with no axis) the camera-facing plane through it. Where
/// that solver scales a pointer delta, this one intersects the two cursor rays
/// with the line or plane, so the point tracks the cursor exactly in perspective
/// and at any depth.
///
/// Returns `None` when either ray meets the line or plane behind the eye, or
/// runs too close to parallel with it to give a stable answer.
pub(crate) fn cursor_translation(
    from: glam::Vec2,
    to: glam::Vec2,
    axis: Option<GizmoAxis>,
    exclude_axis: bool,
    gizmo_center: glam::Vec3,
    camera: &Camera,
    viewport_size: glam::Vec2,
) -> Option<glam::Vec3> {
    let (o0, d0) = crate::maths::gizmo::cursor_ray(camera, from, viewport_size);
    let (o1, d1) = crate::maths::gizmo::cursor_ray(camera, to, viewport_size);
    match axis {
        Some(ax) if !exclude_axis => {
            let a = gizmo_axis_to_vec3(ax);
            let s0 = ray_line_param(o0, d0, gizmo_center, a)?;
            let s1 = ray_line_param(o1, d1, gizmo_center, a)?;
            Some(a * (s1 - s0))
        }
        _ => {
            let normal = match axis {
                Some(ax) => gizmo_axis_to_vec3(ax),
                None => camera.forward(),
            };
            let p0 = ray_plane_hit(o0, d0, gizmo_center, normal)?;
            let p1 = ray_plane_hit(o1, d1, gizmo_center, normal)?;
            let mut delta = p1 - p0;
            // Drop the rounding left along the normal, so a plane drag stays
            // exactly in its plane.
            if let Some(ax) = axis {
                match ax {
                    GizmoAxis::X => delta.x = 0.0,
                    GizmoAxis::Y => delta.y = 0.0,
                    _ => delta.z = 0.0,
                }
            }
            Some(delta)
        }
    }
}

/// Where the ray `origin + dir * t` (`dir` normalised) crosses the plane through
/// `point` with unit `normal`, if it does so in front of the origin and not
/// within about a degree of grazing.
fn ray_plane_hit(
    origin: glam::Vec3,
    dir: glam::Vec3,
    point: glam::Vec3,
    normal: glam::Vec3,
) -> Option<glam::Vec3> {
    let denom = dir.dot(normal);
    if denom.abs() < 0.02 {
        return None;
    }
    let t = (point - origin).dot(normal) / denom;
    (t > 0.0).then(|| origin + dir * t)
}

/// The parameter `s` of the point on the line `point + axis * s` (`axis` unit)
/// closest to the ray `origin + dir * t` (`dir` normalised), if that closest
/// approach is in front of the origin and the two are not near parallel.
fn ray_line_param(
    origin: glam::Vec3,
    dir: glam::Vec3,
    point: glam::Vec3,
    axis: glam::Vec3,
) -> Option<f32> {
    let w = point - origin;
    let b = axis.dot(dir);
    let denom = 1.0 - b * b;
    if denom < 1e-4 {
        return None;
    }
    let s = (b * dir.dot(w) - axis.dot(w)) / denom;
    let t = dir.dot(w) + b * s;
    (t > 0.0).then_some(s)
}

/// Compute per-axis scale multipliers from a pointer delta given an axis constraint.
///
/// All returned scale factors are clamped to `>= 0.001`.
///
/// # Arguments
/// * `pointer_delta` : mouse movement in pixels since last frame.
/// * `axis` : optional axis constraint. `None` = uniform scale.
/// * `exclude_axis` : if `true`, `axis` names the axis to hold at 1.0; the other two axes scale uniformly.
/// * `position` : world-space object position (used for axis projection onto screen).
/// * `view_proj` : camera view-projection matrix.
/// * `viewport_size` : viewport dimensions in pixels.
pub fn constrained_scale(
    pointer_delta: glam::Vec2,
    axis: Option<GizmoAxis>,
    exclude_axis: bool,
    position: glam::Vec3,
    view_proj: glam::Mat4,
    viewport_size: glam::Vec2,
) -> glam::Vec3 {
    const MIN_SCALE: f32 = 0.001;
    let sensitivity = 8.0 / viewport_size.x.max(1.0);

    match axis {
        None => {
            // Uniform scale from horizontal drag.
            let factor = (1.0 + pointer_delta.x * sensitivity).max(MIN_SCALE);
            glam::Vec3::splat(factor)
        }
        Some(ax) => {
            if exclude_axis {
                // Uniform scale on the two non-excluded axes; excluded axis stays at 1.0.
                let factor = (1.0 + pointer_delta.x * sensitivity).max(MIN_SCALE);
                let mut scale = glam::Vec3::ONE;
                match ax {
                    GizmoAxis::X => {
                        scale.y = factor;
                        scale.z = factor;
                    }
                    GizmoAxis::Y => {
                        scale.x = factor;
                        scale.z = factor;
                    }
                    GizmoAxis::Z | GizmoAxis::None => {
                        scale.x = factor;
                        scale.y = factor;
                    }
                    _ => {
                        scale.x = factor;
                        scale.y = factor;
                    }
                }
                scale
            } else {
                // Project drag onto the screen-space axis direction.
                let axis_world = gizmo_axis_to_vec3(ax);
                let base_ndc = view_proj.project_point3(position);
                let tip_ndc = view_proj.project_point3(position + axis_world);
                let base_screen = glam::Vec2::new(
                    (base_ndc.x + 1.0) * 0.5 * viewport_size.x,
                    (1.0 - base_ndc.y) * 0.5 * viewport_size.y,
                );
                let tip_screen = glam::Vec2::new(
                    (tip_ndc.x + 1.0) * 0.5 * viewport_size.x,
                    (1.0 - tip_ndc.y) * 0.5 * viewport_size.y,
                );
                let axis_screen = tip_screen - base_screen;
                let axis_screen_len = axis_screen.length();
                let amount = if axis_screen_len > 1e-4 {
                    pointer_delta.dot(axis_screen / axis_screen_len) / viewport_size.x.max(1.0)
                        * 8.0
                } else {
                    0.0
                };
                let factor = (1.0 + amount).max(MIN_SCALE);
                let mut scale = glam::Vec3::ONE;
                match ax {
                    GizmoAxis::X => scale.x = factor,
                    GizmoAxis::Y => scale.y = factor,
                    GizmoAxis::Z | GizmoAxis::None => scale.z = factor,
                    _ => scale.z = factor,
                }
                scale
            }
        }
    }
}

/// Map a `GizmoAxis` single-axis variant to its world-space unit vector.
///
/// Plane and screen axes fall back to Z.
pub(crate) fn gizmo_axis_to_vec3(axis: GizmoAxis) -> glam::Vec3 {
    match axis {
        GizmoAxis::X => glam::Vec3::X,
        GizmoAxis::Y => glam::Vec3::Y,
        GizmoAxis::Z | GizmoAxis::None => glam::Vec3::Z,
        _ => glam::Vec3::Z,
    }
}

/// Map a `GizmoAxis` exclude-axis variant to the two perpendicular axes.
pub(crate) fn excluded_axes(axis: GizmoAxis) -> (glam::Vec3, glam::Vec3) {
    match axis {
        GizmoAxis::X => (glam::Vec3::Y, glam::Vec3::Z),
        GizmoAxis::Y => (glam::Vec3::X, glam::Vec3::Z),
        GizmoAxis::Z | GizmoAxis::None => (glam::Vec3::X, glam::Vec3::Y),
        _ => (glam::Vec3::X, glam::Vec3::Y),
    }
}

// Re-export project_drag_onto_rotation for use within the crate.
pub(crate) use crate::maths::gizmo::project_drag_onto_rotation as drag_onto_rotation;

#[cfg(test)]
mod tests {
    use super::*;

    fn make_camera() -> Camera {
        Camera::default()
    }

    // --- gizmo_axis_to_vec3 ---

    #[test]
    fn gizmo_axis_to_vec3_x() {
        assert_eq!(gizmo_axis_to_vec3(GizmoAxis::X), glam::Vec3::X);
    }

    #[test]
    fn gizmo_axis_to_vec3_y() {
        assert_eq!(gizmo_axis_to_vec3(GizmoAxis::Y), glam::Vec3::Y);
    }

    #[test]
    fn gizmo_axis_to_vec3_z_and_none_fallback_to_z() {
        assert_eq!(gizmo_axis_to_vec3(GizmoAxis::Z), glam::Vec3::Z);
        assert_eq!(gizmo_axis_to_vec3(GizmoAxis::None), glam::Vec3::Z);
    }

    // --- excluded_axes ---

    #[test]
    fn excluded_axes_x() {
        let (a, b) = excluded_axes(GizmoAxis::X);
        assert_eq!(a, glam::Vec3::Y);
        assert_eq!(b, glam::Vec3::Z);
    }

    #[test]
    fn excluded_axes_y() {
        let (a, b) = excluded_axes(GizmoAxis::Y);
        assert_eq!(a, glam::Vec3::X);
        assert_eq!(b, glam::Vec3::Z);
    }

    #[test]
    fn excluded_axes_z() {
        let (a, b) = excluded_axes(GizmoAxis::Z);
        assert_eq!(a, glam::Vec3::X);
        assert_eq!(b, glam::Vec3::Y);
    }

    // --- angular_rotation_from_cursor ---

    #[test]
    fn angular_rotation_none_cursor_returns_zero() {
        let angle = angular_rotation_from_cursor(
            None,
            glam::Vec2::new(10.0, 0.0),
            glam::Vec3::ZERO,
            glam::Vec3::Z,
            glam::Mat4::IDENTITY,
            glam::Vec2::new(800.0, 600.0),
            glam::Mat4::IDENTITY,
        );
        assert_eq!(angle, 0.0);
    }

    #[test]
    fn angular_rotation_near_center_returns_zero() {
        // Cursor very close to the projected center (within MIN_RADIUS=10)
        let cam = make_camera();
        let vp = cam.proj_matrix() * cam.view_matrix();
        // Project the origin to find center, then place cursor near it
        let ndc = vp.project_point3(glam::Vec3::ZERO);
        let cx = (ndc.x + 1.0) * 0.5 * 800.0;
        let cy = (1.0 - ndc.y) * 0.5 * 600.0;
        let angle = angular_rotation_from_cursor(
            Some(glam::Vec2::new(cx + 2.0, cy)), // 2px from center < MIN_RADIUS
            glam::Vec2::new(1.0, 0.0),
            glam::Vec3::ZERO,
            glam::Vec3::Z,
            vp,
            glam::Vec2::new(800.0, 600.0),
            cam.view_matrix(),
        );
        assert_eq!(angle, 0.0);
    }

    #[test]
    fn angular_rotation_zero_delta_returns_zero() {
        let cam = make_camera();
        let vp = cam.proj_matrix() * cam.view_matrix();
        let ndc = vp.project_point3(glam::Vec3::ZERO);
        let cx = (ndc.x + 1.0) * 0.5 * 800.0;
        let cy = (1.0 - ndc.y) * 0.5 * 600.0;
        let angle = angular_rotation_from_cursor(
            Some(glam::Vec2::new(cx + 100.0, cy)),
            glam::Vec2::ZERO, // no movement
            glam::Vec3::ZERO,
            glam::Vec3::Z,
            vp,
            glam::Vec2::new(800.0, 600.0),
            cam.view_matrix(),
        );
        assert!(angle.abs() < 1e-6);
    }

    // --- constrained_translation ---

    #[test]
    fn constrained_translation_zero_delta_is_zero() {
        let cam = make_camera();
        let result = constrained_translation(
            glam::Vec2::ZERO,
            None,
            false,
            glam::Vec3::ZERO,
            &cam,
            glam::Vec2::new(800.0, 600.0),
        );
        assert!(result.length() < 1e-6);
    }

    #[test]
    fn constrained_translation_free_is_in_camera_plane() {
        let cam = make_camera();
        let delta = glam::Vec2::new(50.0, 0.0); // drag right
        let result = constrained_translation(
            delta,
            None,
            false,
            glam::Vec3::ZERO,
            &cam,
            glam::Vec2::new(800.0, 600.0),
        );
        // Result should be along camera right direction
        let cam_right = cam.right().normalize();
        let result_dir = result.normalize();
        assert!(
            cam_right.dot(result_dir) > 0.9,
            "free translation should be along camera right"
        );
    }

    #[test]
    fn constrained_translation_x_axis_only_x() {
        let cam = make_camera();
        let result = constrained_translation(
            glam::Vec2::new(50.0, 0.0),
            Some(GizmoAxis::X),
            false,
            glam::Vec3::ZERO,
            &cam,
            glam::Vec2::new(800.0, 600.0),
        );
        // Y and Z should be zero when constrained to X
        assert!(result.y.abs() < 1e-5, "Y should be 0 when constrained to X");
        assert!(result.z.abs() < 1e-5, "Z should be 0 when constrained to X");
    }

    #[test]
    fn constrained_translation_exclude_x_zeros_x() {
        let cam = make_camera();
        let result = constrained_translation(
            glam::Vec2::new(50.0, 30.0),
            Some(GizmoAxis::X),
            true,
            glam::Vec3::ZERO,
            &cam,
            glam::Vec2::new(800.0, 600.0),
        );
        assert!(result.x.abs() < 1e-5, "X should be 0 when excluded");
    }

    // --- constrained_scale ---

    #[test]
    fn constrained_scale_zero_delta_is_one() {
        let cam = make_camera();
        let vp = cam.proj_matrix() * cam.view_matrix();
        let result = constrained_scale(
            glam::Vec2::ZERO,
            None,
            false,
            glam::Vec3::ZERO,
            vp,
            glam::Vec2::new(800.0, 600.0),
        );
        assert!((result.x - 1.0).abs() < 1e-5);
        assert!((result.y - 1.0).abs() < 1e-5);
        assert!((result.z - 1.0).abs() < 1e-5);
    }

    #[test]
    fn constrained_scale_uniform_positive_drag() {
        let cam = make_camera();
        let vp = cam.proj_matrix() * cam.view_matrix();
        let result = constrained_scale(
            glam::Vec2::new(100.0, 0.0), // drag right
            None,
            false,
            glam::Vec3::ZERO,
            vp,
            glam::Vec2::new(800.0, 600.0),
        );
        // All axes should be the same and > 1.0
        assert!(result.x > 1.0);
        assert!((result.x - result.y).abs() < 1e-5);
        assert!((result.x - result.z).abs() < 1e-5);
    }

    #[test]
    fn constrained_scale_min_clamp() {
        let cam = make_camera();
        let vp = cam.proj_matrix() * cam.view_matrix();
        let result = constrained_scale(
            glam::Vec2::new(-10000.0, 0.0), // huge negative drag
            None,
            false,
            glam::Vec3::ZERO,
            vp,
            glam::Vec2::new(800.0, 600.0),
        );
        assert!(result.x >= 0.001);
    }

    #[test]
    fn constrained_scale_exclude_x_keeps_x_at_one() {
        let cam = make_camera();
        let vp = cam.proj_matrix() * cam.view_matrix();
        let result = constrained_scale(
            glam::Vec2::new(100.0, 0.0),
            Some(GizmoAxis::X),
            true,
            glam::Vec3::ZERO,
            vp,
            glam::Vec2::new(800.0, 600.0),
        );
        assert!(
            (result.x - 1.0).abs() < 1e-5,
            "excluded X should stay at 1.0"
        );
        assert!(result.y > 1.0, "non-excluded Y should scale");
        assert!(result.z > 1.0, "non-excluded Z should scale");
    }

    // --- cursor_translation and its ray helpers ---

    #[test]
    fn a_ray_parallel_to_a_plane_does_not_hit_it() {
        let hit = ray_plane_hit(
            glam::Vec3::Z,
            glam::Vec3::X,
            glam::Vec3::ZERO,
            glam::Vec3::Z,
        );
        assert_eq!(hit, None);
    }

    #[test]
    fn a_plane_behind_the_ray_is_not_hit() {
        let hit = ray_plane_hit(
            glam::Vec3::Z,
            glam::Vec3::Z,
            glam::Vec3::ZERO,
            glam::Vec3::Z,
        );
        assert_eq!(hit, None);
    }

    #[test]
    fn a_ray_down_the_line_has_no_closest_point() {
        let s = ray_line_param(
            glam::Vec3::new(0.0, 0.0, 5.0),
            -glam::Vec3::Z,
            glam::Vec3::ZERO,
            glam::Vec3::Z,
        );
        assert_eq!(s, None);
    }

    #[test]
    fn the_closest_point_on_a_line_is_where_the_ray_crosses_over_it() {
        // A ray straight down through (2, 0, 5) passes over the X axis at x = 2.
        let s = ray_line_param(
            glam::Vec3::new(2.0, 0.0, 5.0),
            -glam::Vec3::Z,
            glam::Vec3::ZERO,
            glam::Vec3::X,
        );
        assert!((s.unwrap() - 2.0).abs() < 1e-5);
    }

    #[test]
    fn a_cursor_translation_with_no_move_is_zero() {
        let camera = make_camera();
        let vp = glam::Vec2::new(800.0, 600.0);
        let at = glam::Vec2::new(410.0, 290.0);
        for (axis, exclude) in [
            (None, false),
            (Some(GizmoAxis::X), false),
            (Some(GizmoAxis::Z), true),
        ] {
            let t = cursor_translation(at, at, axis, exclude, glam::Vec3::ZERO, &camera, vp);
            assert_eq!(t, Some(glam::Vec3::ZERO), "{axis:?} exclude {exclude}");
        }
    }
}
