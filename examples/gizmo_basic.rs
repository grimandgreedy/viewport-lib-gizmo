//! A transform gizmo drawn over one object, driven end to end.
//!
//! Uses the `ViewportApp` runner from `viewport-lib` (a dev-dependency, for the
//! renderer only). Each frame it drives a [`GizmoDriver`], applies the returned
//! delta to the object, and draws the handles with the default [`GizmoStyle`].
//! Drag a handle to move the object; navigation orbits when no handle is grabbed.
//! Press Tab or Q to cycle the gizmo mode (translate / rotate / scale), and
//! backtick (`) to toggle relative / objective values.
//!
//! Run with `cargo run --example gizmo_basic`.

use std::cell::RefCell;
use std::rc::Rc;

use glam::{Mat4, Vec2, Vec3};
use viewport_lib as vpl;
use vpl::{
    AppConfig, ButtonState, KeyCode, Material, NodeId, OrbitCameraController, ViewportApp,
    ViewportContext, ViewportEvent, primitives,
};

use viewport_lib_gizmo::{
    FrameInput, GizmoDriver, GizmoStyle, ManipResult, TransformDelta, build_gizmo_overlays,
    build_numeric_label,
};

/// Shared gizmo state, written in the run callback and read by the input handler.
struct State {
    node: Option<NodeId>,
    /// The object's committed transform (before the current drag).
    base: Mat4,
    /// The object's live transform (base plus the in-progress delta).
    current: Mat4,
    driver: GizmoDriver,
    style: GizmoStyle,
    /// True while a handle is hovered or a session is active: suppress orbit.
    grabbing: bool,
}

/// Compose a transform delta around a world pivot onto a base transform.
fn apply_delta(delta: &TransformDelta, pivot: Vec3, base: Mat4) -> Mat4 {
    let rot = Mat4::from_quat(delta.rotation);
    let scale = Mat4::from_scale(Vec3::new(
        delta.scale_override[0].unwrap_or(delta.scale.x),
        delta.scale_override[1].unwrap_or(delta.scale.y),
        delta.scale_override[2].unwrap_or(delta.scale.z),
    ));
    let translate = Mat4::from_translation(delta.translation);
    let to_pivot = Mat4::from_translation(-pivot);
    let from_pivot = Mat4::from_translation(pivot);
    let mut m = translate * from_pivot * rot * scale * to_pivot * base;
    for (i, ov) in delta.position_override.iter().enumerate() {
        if let Some(v) = ov {
            m.col_mut(3)[i] = *v;
        }
    }
    m
}

fn main() {
    let state = Rc::new(RefCell::new(State {
        node: None,
        base: Mat4::IDENTITY,
        current: Mat4::IDENTITY,
        driver: GizmoDriver::new(),
        style: GizmoStyle::default(),
        grabbing: false,
    }));

    ViewportApp::new(
        AppConfig::default()
            .with_title("viewport-lib-gizmo : basic")
            .with_window_size(1280, 720),
    )
    .setup({
        let state = state.clone();
        move |session, device| {
            let cube = session
                .resources_mut()
                .upload_mesh_data(device, &primitives::cube(1.0))
                .unwrap();
            let transform = Mat4::from_translation(Vec3::ZERO);
            let node = session
                .scene_mut()
                .add(Some(cube), transform, Material::from_colour([0.4, 0.6, 0.9]));
            session.camera_mut().distance = 8.0;
            let mut s = state.borrow_mut();
            s.node = Some(node);
            s.base = transform;
            s.current = transform;
        }
    })
    // Own navigation so orbit can be suppressed while a handle is grabbed.
    .with_input({
        let state = state.clone();
        let mut orbit = OrbitCameraController::viewport_all();
        move |ictx| {
            let size = ictx.viewport_size();
            orbit.begin_frame(ViewportContext {
                hovered: true,
                focused: true,
                viewport_size: size,
            });
            let grabbing = state.borrow().grabbing;
            for ev in ictx.events().to_vec() {
                // Tab or Q cycles the gizmo mode (translate -> rotate -> scale);
                // backtick toggles relative / objective values. cycle_mode is
                // ignored while a session is active, so during a G/R/S session Tab
                // still advances the numeric axis instead.
                if let ViewportEvent::Key {
                    key,
                    state: ButtonState::Pressed,
                    repeat: false,
                } = ev
                {
                    match key {
                        KeyCode::Tab | KeyCode::Q => state.borrow_mut().driver.cycle_mode(),
                        KeyCode::Backtick => state.borrow_mut().driver.toggle_value_mode(),
                        _ => {}
                    }
                }
                ictx.forward(ev.clone());
                if !grabbing {
                    orbit.push_event(ev);
                }
            }
            orbit.apply_to_camera(ictx.camera_mut());
        }
    })
    .run({
        let state = state.clone();
        move |ctx| {
            let mut s = state.borrow_mut();
            let Some(node) = s.node else {
                return;
            };

            let pointer = ctx.action_frame().pointer;
            let viewport = ctx.viewport_size();
            let viewport_size = Vec2::new(viewport[0], viewport[1]);
            let camera = ctx.camera().clone();

            // The gizmo sits at the object's current position.
            let center = s.current.col(3).truncate();

            let result = s.driver.update(FrameInput {
                action_frame: ctx.action_frame(),
                camera: &camera,
                viewport_size,
                cursor_viewport: pointer.cursor,
                pointer_delta: pointer.delta,
                selection_center: Some(center),
                drag_started: pointer.drag_started,
                dragging: pointer.dragging,
                clicked: pointer.clicked,
            });

            match result {
                ManipResult::Update(delta) => {
                    // The delta is the total transform since the drag began, so it
                    // always composes onto the drag-start snapshot around the
                    // drag-start pivot.
                    let pivot = s.base.col(3).truncate();
                    s.current = apply_delta(&delta, pivot, s.base);
                    ctx.scene_mut().set_local_transform(node, s.current);
                }
                ManipResult::Commit => {
                    s.base = s.current;
                }
                ManipResult::Cancel | ManipResult::ConstraintChanged => {
                    s.current = s.base;
                    ctx.scene_mut().set_local_transform(node, s.current);
                }
                ManipResult::None => {
                    // Keep the snapshot current while idle so the next drag starts
                    // from the committed pose.
                    if !s.driver.is_active() {
                        s.base = s.current;
                    }
                }
                _ => {}
            }

            // Suppress orbit next frame while a handle is hovered or a drag is on.
            s.grabbing = s.driver.is_active()
                || s.driver.hovered_axis() != viewport_lib_gizmo::GizmoAxis::None;

            // Draw the gizmo.
            if let Some(gizmo_center) = s.driver.gizmo_center() {
                let scale = s.driver.gizmo_scale();
                let view_proj = camera.proj_matrix() * camera.view_matrix();
                let forward = (camera.center - camera.eye_position()).normalize_or(Vec3::NEG_Z);
                let mode = s.driver.gizmo().mode;
                let hovered = s.driver.hovered_axis();
                let style = s.style.clone();
                let overlays = ctx.overlays_mut();
                build_gizmo_overlays(
                    &style,
                    mode,
                    view_proj,
                    forward,
                    viewport,
                    gizmo_center,
                    scale,
                    hovered,
                    glam::Quat::IDENTITY,
                    &mut overlays.shapes,
                    &mut overlays.polylines,
                );
                // Numeric readout (e.g. while typing "S 22 Tab 2").
                if let Some(text) = s.driver.readout() {
                    overlays
                        .labels
                        .push(build_numeric_label(&style, &text, gizmo_center));
                }
            }
        }
    });
}
