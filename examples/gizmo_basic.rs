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
    Action, AppConfig, BindingPreset, ButtonState, Colour, InputOwnership, KeyCode,
    KeyboardOwnership, Material, ModifiersMatch, NodeId, OrbitCameraController, PointerOwnership,
    ViewportApp, ViewportBinding, ViewportContext, ViewportEvent, ViewportGesture, ViewportInput,
    WindowConfig, primitives, viewport_default_bindings,
};

use viewport_lib_gizmo::{
    FrameInput, GizmoDriver, GizmoStyle, ManipResult, TransformDelta, ValueMode,
    build_gizmo_overlays, build_numeric_label,
};

/// Shared gizmo state, written in the frame callback and read by the input handler.
struct State {
    node: Option<NodeId>,
    /// The object's committed transform (before the current drag).
    base: Mat4,
    /// The object's live transform (base plus the in-progress delta).
    current: Mat4,
    driver: GizmoDriver,
    style: GizmoStyle,
    /// True while a handle is hovered or a session is active: the left button and
    /// pointer moves are the gizmo's, not the camera's.
    grabbing: bool,
    /// The camera's own resolver. The gizmo reads the runner's frame, which sees
    /// every press; this one is kept off the pointer while the gizmo wants it.
    camera_input: ViewportInput,
}

/// The default bindings, with Q also cycling the gizmo mode. Tab already does,
/// and Q is bound by the character it types so it is the key labelled Q on any
/// layout.
fn gizmo_bindings() -> Vec<ViewportBinding> {
    let mut bindings = viewport_default_bindings();
    bindings.push(ViewportBinding::new(
        Action::CycleGizmoMode,
        ViewportGesture::CharPress {
            ch: 'q',
            modifiers: ModifiersMatch::Exact(vpl::input::Modifiers::NONE),
        },
    ));
    bindings
}

/// Compose a transform delta around a world pivot onto a base transform. Numeric
/// overrides are applied relative to the start pose or as absolute world values,
/// per `values`.
fn apply_delta(delta: &TransformDelta, pivot: Vec3, base: Mat4, values: ValueMode) -> Mat4 {
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
    let base_pos = base.col(3);
    for (i, ov) in delta.position_override.iter().enumerate() {
        if let Some(v) = ov {
            m.col_mut(3)[i] = match values {
                ValueMode::Objective => *v, // absolute world coordinate
                _ => base_pos[i] + *v,      // relative: offset from start
            };
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
        camera_input: ViewportInput::from_preset(BindingPreset::Default),
    }));

    ViewportApp::new(AppConfig::default())
        .window_with_input(
            WindowConfig::default()
                .with_title("viewport-lib-gizmo : basic")
                .with_window_size(1280, 720),
            {
                let state = state.clone();
                move |session, device| {
                    let cube = session
                        .resources_mut()
                        .upload_mesh_data(device, &primitives::cube(1.0))
                        .unwrap();
                    let blue = session
                        .resources_mut()
                        .upload_material(Material::from_colour(Colour::linear_rgb(0.4, 0.6, 0.9)))
                        .unwrap();
                    let transform = Mat4::from_translation(Vec3::ZERO);
                    let node = session.scene_mut().add(Some(cube), transform, blue);
                    session.camera_mut().set_orbit_distance(8.0);
                    session.set_bindings(gizmo_bindings());
                    let mut s = state.borrow_mut();
                    s.node = Some(node);
                    s.base = transform;
                    s.current = transform;
                }
            },
            // Own navigation so the gizmo can keep the left button while a handle is
            // grabbed.
            {
                let state = state.clone();
                let mut orbit = OrbitCameraController::new_stateless();
                move |ictx| {
                    let size = ictx.viewport_size();
                    // The size stays on the controller, which needs it to turn a pan in pixels
                    // into one in world units; the events go to the input, which owns the scheme
                    // that reads them.
                    orbit.set_viewport_size(size);
                    let mut s = state.borrow_mut();
                    // While a handle is hovered or a session runs, the pointer is only
                    // inside as far as the camera is concerned: a left press or pointer
                    // move is the gizmo's and does not also orbit, while the wheel,
                    // modifier changes and the other buttons still reach the camera,
                    // so zooming and panning keep working over a handle.
                    let pointer = if s.grabbing {
                        PointerOwnership::Inside
                    } else {
                        PointerOwnership::Owned
                    };
                    let ownership = InputOwnership {
                        pointer,
                        keyboard: KeyboardOwnership::Owned,
                    };
                    s.camera_input
                        .begin_frame(ViewportContext::new(ownership, size));
                    for ev in ictx.events().to_vec() {
                        // Backtick toggles relative / objective values.
                        if let ViewportEvent::Key {
                            key: KeyCode::Backtick,
                            state: ButtonState::Pressed,
                            repeat: false,
                            ..
                        } = ev
                        {
                            s.driver.toggle_value_mode();
                        }
                        ictx.forward(ev.clone());
                        s.camera_input.push_event(ev);
                    }
                    let actions = s.camera_input.resolve();
                    let dt = ictx.dt();
                    orbit.apply(ictx.camera_mut(), &actions, dt);
                }
            },
            {
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
                    let was_busy = s.grabbing;

                    // Tab or Q cycles the gizmo mode (translate -> rotate -> scale).
                    // cycle_mode is ignored while a session is active, so during a
                    // G/R/S session Tab still advances the numeric axis instead.
                    if ctx.action_frame().is_active(Action::CycleGizmoMode) {
                        s.driver.cycle_mode();
                    }

                    let result = s.driver.update(FrameInput {
                        action_frame: ctx.action_frame(),
                        camera: &camera,
                        viewport_size,
                        cursor_viewport: pointer.cursor,
                        pointer_delta: pointer.delta,
                        selection_center: Some(center),
                        selection_orientation: glam::Quat::IDENTITY,
                        drag_started: pointer.pressed,
                        dragging: pointer.dragging,
                        clicked: pointer.clicked,
                    });

                    match result {
                        ManipResult::Update(delta) => {
                            // The delta is the total transform since the drag began, so it
                            // always composes onto the drag-start snapshot around the
                            // drag-start pivot.
                            let pivot = s.base.col(3).truncate();
                            let values = s.driver.value_mode();
                            s.current = apply_delta(&delta, pivot, s.base, values);
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

                    // Keep the left button from the camera next frame while a handle is
                    // hovered or a drag is on.
                    s.grabbing = s.driver.is_active()
                        || s.driver.hovered_axis() != viewport_lib_gizmo::GizmoAxis::None;
                    // The camera's ownership for this frame was decided last frame, so a
                    // press the gizmo takes with no hover frame before it (a touch, or a
                    // move and press in one frame) has already armed the camera's drag.
                    if pointer.pressed && (was_busy || s.grabbing) {
                        s.camera_input.cancel_gesture();
                    }

                    // Draw the gizmo.
                    if let Some(gizmo_center) = s.driver.gizmo_center() {
                        let scale = s.driver.gizmo_scale();
                        let view_proj = camera.proj_matrix() * camera.view_matrix();
                        let forward = camera.forward();
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
            },
        )
        .run();
}
