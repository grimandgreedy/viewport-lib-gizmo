//! A transform gizmo in a four-view layout, the way a CAD quad view is built.
//!
//! One scene is seen from four cameras: perspective, top, front and right, the
//! last three orthographic. Each view has its own camera, orbit controller and
//! input resolver, and is rendered with viewport-lib's multi-viewport API into
//! its own offscreen target, which the window's paint hook blits into a quarter
//! of the window.
//!
//! One [`GizmoDriver`] manipulates the selection for all four views. The view
//! under the cursor drives it, and [`ActiveView`] keeps a drag or a G / R / S
//! session on the view it started in, even when the cursor crosses into another.
//! Every view draws the gizmo sized for its own camera with
//! [`GizmoDriver::build_view_overlays`]. Only the driving view shows the hovered
//! handle; while a session runs, every view shows the axis being changed.
//!
//! Drag a handle to move the cube; navigation orbits the view under the cursor
//! when no handle is grabbed. Tab or Q cycles the mode, backtick (`) toggles
//! relative / objective values, and G / R / S start a keyboard session.
//!
//! Run with `cargo run --example gizmo_multi_viewport`.

use std::cell::RefCell;
use std::rc::Rc;

use glam::{Mat4, Quat, Vec2, Vec3};
use viewport_lib as vpl;
use vpl::input::adapters::winit as vpl_winit;
use vpl::scene::Scene;
use vpl::{
    Action, AppConfig, BindingPreset, BlitTexture, ButtonState, Camera, CameraFrame, Colour,
    FrameData, KeyCode, LabelItem, Material, ModifiersMatch, NodeId, OffscreenViewportTarget,
    OrbitCameraController, SceneFrame, Selection, ViewportBinding, ViewportContext, ViewportEvent,
    ViewportGesture, ViewportId, ViewportInput, WindowConfig, primitives,
    viewport_default_bindings,
};

use viewport_lib_gizmo::{
    ActiveView, FrameInput, GizmoAxis, GizmoDriver, GizmoStyle, ManipResult, TransformDelta,
    ValueMode, build_numeric_label,
};

const LABELS: [&str; 4] = ["Perspective", "Top", "Front", "Right"];

/// One quadrant: its camera and input, the renderer viewport it draws through,
/// and the offscreen target the window blits from.
struct View {
    camera: Camera,
    orbit: OrbitCameraController,
    input: ViewportInput,
    id: ViewportId,
    target: OffscreenViewportTarget,
    blit: Option<BlitTexture>,
}

#[derive(Default)]
struct State {
    scene: Scene,
    selection: Selection,
    node: Option<NodeId>,
    /// The cube's committed transform (before the current drag).
    base: Mat4,
    /// The cube's live transform (base plus the in-progress delta).
    current: Mat4,
    driver: GizmoDriver,
    style: GizmoStyle,
    active: ActiveView,
    /// The view that drove last frame, to tell it the pointer left when it changes.
    last_active: Option<usize>,
    /// True while any pointer button is down: a drag stays on its view.
    button_held: bool,
    /// True while a handle is hovered or a session runs: the left button and
    /// pointer moves are the gizmo's, not the camera's.
    grabbing: bool,
    views: Vec<View>,
    /// Each view's rect in the window, in physical pixels: `[x, y, w, h]`.
    rects: [[u32; 4]; 4],
}

/// The four cameras: a perspective view and orthographic top, front and right.
fn cameras() -> [Camera; 4] {
    let half_pi = std::f32::consts::FRAC_PI_2;
    let camera = |orientation: Quat, orthographic: bool, distance: f32| {
        let camera = Camera::orbit(glam::DVec3::ZERO, distance, orientation);
        if orthographic {
            camera.orthographic()
        } else {
            camera
        }
    };
    [
        camera(
            Quat::from_rotation_z(0.6) * Quat::from_rotation_x(1.1),
            false,
            12.0,
        ),
        camera(Quat::IDENTITY, true, 10.0),
        camera(Quat::from_rotation_x(half_pi), true, 10.0),
        camera(
            Quat::from_rotation_z(half_pi) * Quat::from_rotation_x(half_pi),
            true,
            10.0,
        ),
    ]
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
                ValueMode::Objective => *v,
                _ => base_pos[i] + *v,
            };
        }
    }
    m
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

fn main() {
    let state = Rc::new(RefCell::new(State::default()));
    let setup_state = state.clone();
    let paint_state = state.clone();

    vpl::ViewportApp::new(AppConfig::default())
        .window_with_paint(
            WindowConfig::default()
                .with_title("viewport-lib-gizmo : four views")
                .with_window_size(1280, 800),
            // The window's own scene stays empty: it only clears the backdrop the
            // four views are blitted onto. The shared scene lives in `State`.
            move |instance, device| {
                let cube = instance
                    .resources_mut()
                    .upload_mesh_data(device, &primitives::cube(1.0))
                    .unwrap();
                let gold = instance
                    .resources_mut()
                    .upload_material(Material::from_colour(Colour::linear_rgb(0.8, 0.55, 0.15)))
                    .unwrap();
                let grey = instance
                    .resources_mut()
                    .upload_material(Material::from_colour(Colour::linear_rgb(0.35, 0.35, 0.4)))
                    .unwrap();
                // The window's resolver drives the gizmo, so its bindings carry the
                // gizmo's shortcuts.
                instance.set_bindings(gizmo_bindings());
                let mut s = setup_state.borrow_mut();
                let node = s.scene.add(Some(cube), Mat4::IDENTITY, gold);
                // Fixed markers so the views have something to orient against.
                for pos in [
                    Vec3::new(3.0, 0.0, 0.0),
                    Vec3::new(0.0, 3.0, 0.0),
                    Vec3::new(0.0, 0.0, 3.0),
                ] {
                    let marker = Mat4::from_scale_rotation_translation(
                        Vec3::splat(0.4),
                        Quat::IDENTITY,
                        pos,
                    );
                    s.scene.add(Some(cube), marker, grey);
                }
                s.scene.update_transforms();
                s.selection.select_one(node);
                s.node = Some(node);
            },
            move |ctx| {
                let device = ctx.device().clone();
                let queue = ctx.queue().clone();
                let format = ctx.surface_format();
                let [pw, ph] = ctx.surface_size();
                let [wl, _] = ctx.viewport_size();
                let ppp = pw as f32 / wl.max(1.0);
                let dt = ctx.dt();
                let action = ctx.action_frame().clone();
                let cursor = action.pointer.cursor;
                let events: Vec<ViewportEvent> = ctx.events().to_vec();

                let mut guard = state.borrow_mut();
                let s = &mut *guard;
                let Some(node) = s.node else {
                    return;
                };

                // Quadrant rects in physical pixels, and the same in logical points.
                let (left_w, top_h) = (pw / 2, ph / 2);
                s.rects = [
                    [0, 0, left_w, top_h],
                    [left_w, 0, pw - left_w, top_h],
                    [0, top_h, left_w, ph - top_h],
                    [left_w, top_h, pw - left_w, ph - top_h],
                ];
                let logical = s.rects.map(|[x, y, w, h]| {
                    (
                        Vec2::new(x as f32, y as f32) / ppp,
                        Vec2::new(w as f32, h as f32) / ppp,
                    )
                });

                // Build the views once, then keep each target the size of its quarter.
                if s.views.is_empty() {
                    for (camera, rect) in cameras().into_iter().zip(&s.rects) {
                        s.views.push(View {
                            camera,
                            orbit: OrbitCameraController::new_stateless(),
                            input: ViewportInput::from_preset(BindingPreset::Default),
                            id: ctx.renderer_mut().create_viewport(&device),
                            target: OffscreenViewportTarget::new(
                                &device,
                                format,
                                [rect[2], rect[3]],
                            ),
                            blit: None,
                        });
                    }
                }
                for (view, rect) in s.views.iter_mut().zip(&s.rects) {
                    if view.target.resize(&device, [rect[2], rect[3]]) {
                        view.blit = None;
                    }
                    if view.blit.is_none() {
                        view.blit = Some(
                            ctx.renderer_mut()
                                .create_blit(&device, view.target.render_view()),
                        );
                    }
                }

                // Which view drives the gizmo and takes the input this frame.
                let hovered = cursor.and_then(|c| {
                    logical.iter().position(|(o, sz)| {
                        c.x >= o.x && c.y >= o.y && c.x < o.x + sz.x && c.y < o.y + sz.y
                    })
                });
                let active = s.active.current(hovered);
                if s.last_active != active {
                    if let Some(prev) = s.last_active {
                        s.views[prev].input.push_event(ViewportEvent::PointerLeft);
                    }
                    s.last_active = active;
                }

                for ev in &events {
                    match ev {
                        ViewportEvent::MouseButton { state, .. } => {
                            s.button_held = *state == ButtonState::Pressed;
                        }
                        ViewportEvent::FocusLost => s.button_held = false,
                        // Backtick toggles relative / objective values.
                        ViewportEvent::Key {
                            key: KeyCode::Backtick,
                            state: ButtonState::Pressed,
                            repeat: false,
                            ..
                        } if active.is_some() => s.driver.toggle_value_mode(),
                        _ => {}
                    }
                }
                // Tab or Q cycles the mode. cycle_mode is ignored during a session,
                // where Tab advances the numeric axis instead.
                if active.is_some() && action.is_active(Action::CycleGizmoMode) {
                    s.driver.cycle_mode();
                }

                // Navigation. Only the driving view owns the input; every view is
                // given every event and its resolver drops what is not its own. While
                // a handle is hovered or a session runs, the gizmo claims the pointer
                // in the driving view: a left press or pointer move is the gizmo's
                // and does not also orbit, while the wheel, modifier changes and the
                // other buttons still reach the camera, so zooming and panning keep
                // working over a handle.
                for (i, view) in s.views.iter_mut().enumerate() {
                    let (origin, size) = logical[i];
                    let driving = active == Some(i);
                    // The runner only delivers events while the window has them.
                    let ownership =
                        vpl_winit::ownership(true, driving, false, s.grabbing, !driving);
                    view.input
                        .begin_frame(ViewportContext::new(ownership, size.into()));
                    for ev in &events {
                        view.input.push_event(ev.offset_by(origin));
                    }
                    view.orbit.set_viewport_size(size.into());
                    view.orbit
                        .apply(&mut view.camera, &view.input.resolve(), dt);
                    view.camera.set_aspect_ratio(size.x, size.y);
                }

                // Drive the gizmo from one view, with the window's unfiltered input
                // and the cursor moved into that view's pixels. With the cursor
                // outside the window, view 0 keeps it sized and placed, with no
                // cursor.
                let driver_view = active.unwrap_or(0);
                let was_grabbing = s.grabbing;
                let actions = &action;
                let pointer = actions.pointer;
                let centre = s.current.col(3).truncate();
                let result = s.driver.update(FrameInput {
                    action_frame: actions,
                    camera: &s.views[driver_view].camera,
                    viewport_size: logical[driver_view].1,
                    cursor_viewport: if active.is_some() {
                        pointer.cursor.map(|c| c - logical[driver_view].0)
                    } else {
                        None
                    },
                    pointer_delta: pointer.delta,
                    selection_center: Some(centre),
                    selection_orientation: glam::Quat::IDENTITY,
                    drag_started: pointer.pressed,
                    dragging: pointer.dragging,
                    clicked: pointer.clicked,
                });
                match result {
                    ManipResult::Update(delta) => {
                        // The delta is the total since the drag began, so it composes
                        // onto the drag-start snapshot around the drag-start pivot.
                        let pivot = s.base.col(3).truncate();
                        s.current = apply_delta(&delta, pivot, s.base, s.driver.value_mode());
                        s.scene.set_local_transform(node, s.current);
                    }
                    ManipResult::Commit => s.base = s.current,
                    ManipResult::Cancel | ManipResult::ConstraintChanged => {
                        s.current = s.base;
                        s.scene.set_local_transform(node, s.current);
                    }
                    ManipResult::None if !s.driver.is_active() => s.base = s.current,
                    _ => {}
                }
                s.scene.update_transforms();
                s.grabbing = s.driver.is_active() || s.driver.hovered_axis() != GizmoAxis::None;
                // The views' ownership for this frame was decided last frame, so a
                // press the gizmo takes with no hover frame before it (a touch, or a
                // move and press in one frame) has already armed that view's drag.
                if pointer.pressed && (was_grabbing || s.grabbing) {
                    s.views[driver_view].input.cancel_gesture();
                }
                s.active
                    .end_frame(active, s.button_held || s.driver.is_active());

                // One frame per view: the shared scene, that view's camera, and the
                // gizmo sized for it.
                let items = s.scene.collect_render_items(&s.selection);
                // Offset from the window's own (empty) scene, which renders through
                // the same renderer, so the two never share a generation.
                let generation = s.scene.version() + (1 << 32);
                let centre = s.current.col(3).truncate();
                let readout = s.driver.readout();
                let frame_data: Vec<FrameData> = s
                    .views
                    .iter()
                    .enumerate()
                    .map(|(i, view)| {
                        let size = logical[i].1;
                        let camera = CameraFrame::from_camera(&view.camera, size.into())
                            .with_viewport_id(view.id)
                            .with_pixels_per_point(ppp);
                        let mut fd =
                            FrameData::new(camera, SceneFrame::from_surface_items(items.clone()));
                        fd.scene.generation = generation;
                        fd.interaction.selection_generation = s.selection.version();
                        fd.interaction.outline_selected = true;
                        fd.viewport.show_grid = true;
                        fd.viewport.show_axes_indicator = true;
                        let driving = active == Some(i);
                        s.driver.build_view_overlays(
                            &s.style,
                            &view.camera,
                            size,
                            driving,
                            &mut fd.overlays.shapes,
                            &mut fd.overlays.polylines,
                        );
                        if let (true, Some(text)) = (driving, &readout) {
                            fd.overlays
                                .labels
                                .push(build_numeric_label(&s.style, text, centre));
                        }
                        fd.overlays.labels.push(
                            LabelItem::new(LABELS[i])
                                .with_position([8.0, 8.0])
                                .with_colour(Colour::linear_rgb(0.8, 0.8, 0.8)),
                        );
                        fd
                    })
                    .collect();

                // Prepare every view before rendering any, then render each into its
                // own target.
                let renderer = ctx.renderer_mut();
                let (scene_fx, _) = frame_data[0].effects.split();
                let token =
                    renderer
                        .owned()
                        .prepare_scene(&device, &queue, &frame_data[0], &scene_fx);
                for (view, fd) in s.views.iter().zip(&frame_data) {
                    renderer
                        .owned()
                        .prepare_viewport(&device, &queue, &token, view.id, fd);
                }
                for (view, fd) in s.views.iter().zip(&frame_data) {
                    let cmd = renderer.owned().render_viewport(
                        &device,
                        &queue,
                        view.target.render_view(),
                        view.id,
                        fd,
                    );
                    queue.submit(std::iter::once(cmd));
                }
                ctx.request_redraw();
            },
            move |pctx| {
                let s = paint_state.borrow();
                for (view, [x, y, w, h]) in s.views.iter().zip(s.rects) {
                    if let Some(blit) = &view.blit {
                        pctx.blit_rect(blit, x, y, w, h);
                    }
                }
            },
        )
        .run();
}
