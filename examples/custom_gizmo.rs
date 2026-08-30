//! A custom gizmo built from the maths helpers alone: no `ManipulationController`
//! and no renderer.
//!
//! This shows the layer-1 "build your own gizmo" surface. Each frame a real app
//! would: size the gizmo for the screen, turn the cursor into a ray, hit-test the
//! handles, and project the drag once a handle is grabbed. Here we run one such
//! step with fixed inputs and print the results.
//!
//! Run with `cargo run --example custom_gizmo`.

use glam::{Vec2, Vec3};
use viewport_lib_gizmo::{
    Gizmo, GizmoAxis, compute_gizmo_scale, cursor_ray, project_drag_onto_axis, snap_value,
};
use viewport_lib_types::camera::Camera;

fn main() {
    // A camera looking at the origin from a fixed distance (Z-up world).
    let mut camera = Camera::default();
    camera.center = Vec3::ZERO;
    camera.distance = 6.0;
    let viewport = Vec2::new(1280.0, 720.0);
    let view_proj = camera.proj_matrix() * camera.view_matrix();

    // The gizmo sits at the origin. Size it so it is a constant ~100px on screen.
    let gizmo_center = Vec3::ZERO;
    let scale = compute_gizmo_scale(
        gizmo_center,
        camera.eye_position(),
        camera.fov_y,
        viewport.y,
    );
    println!("gizmo world scale at distance {}: {scale:.4}", camera.distance);

    // Hit-test: aim a ray from the eye at the +X arm and see which axis it grabs.
    let gizmo = Gizmo::new();
    let x_handle = gizmo_center + Vec3::X * scale * 0.5;
    let ray_origin = camera.eye_position();
    let ray_dir = (x_handle - ray_origin).normalize();
    let hit = gizmo.hit_test(ray_origin, ray_dir, gizmo_center, scale);
    println!("ray aimed at the +X arm hit: {hit:?}");
    assert_eq!(hit, GizmoAxis::X, "a ray down the X arm should grab X");

    // Turn a cursor at the viewport centre into a world ray (what a real app feeds
    // the hit-test each frame).
    let (origin, dir) = cursor_ray(&camera, viewport * 0.5, viewport);
    println!("centre-cursor ray: origin {origin:?}, dir {dir:?}");

    // Grab X and project a 40px horizontal drag onto the axis, then snap it to a
    // 0.25-unit grid.
    let drag = Vec2::new(40.0, 0.0);
    let along_x = project_drag_onto_axis(drag, Vec3::X, view_proj, gizmo_center, viewport);
    let snapped = snap_value(along_x, 0.25);
    println!("drag {drag:?} on X: {along_x:.4} world units, snapped to {snapped:.4}");

    // Build the transform a real app would apply to the selected object.
    let translation = Vec3::X * snapped;
    println!("resulting translation: {translation:?}");
}
