//! The gizmo as viewport-lib-ui widgets across a four-view layout.
//!
//! The `ui` counterpart of `gizmo_multi_viewport`: perspective, top, front and
//! right views of one scene, each rendered through viewport-lib's multi-viewport
//! API into its own offscreen target and blitted into a quarter of the window.
//!
//! One `GizmoTool` is registered once, and each view has its own toolkit screen
//! holding one pane of it, with an id of its own. Each view's frames carry that
//! view's camera in a `GizmoFrame` on their host map. The panes share the tool's
//! one driver. The toolkit works out which pane the pointer is on, and that pane
//! drives; a drag or a G / R / S session stays with the pane it started in. The
//! others paint the gizmo through their own view's camera, and show the axis being
//! changed while a session runs. A transform therefore moves the cube once,
//! however many views show it.
//!
//! Drag a handle to move the cube; drag empty space to orbit the view under the
//! cursor. Tab or Q cycles the mode, backtick (`) toggles relative / objective,
//! and G / R / S start a keyboard session in the view under the cursor.
//!
//! Run with `cargo run -p viewport-lib-gizmo-ui --example gizmo_ui_multi_viewport`.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use glam::{Mat4, Quat, Vec2, Vec3};
use viewport_lib as vpl;
use vpl::input::adapters::winit as vpl_winit;
use vpl::scene::Scene;
use vpl::{
    Action, AppConfig, BindingPreset, BlitTexture, ButtonState, Camera, CameraFrame, FrameData,
    KeyCode, LabelItem, Material, ModifiersMatch, NodeId, OffscreenViewportTarget,
    OrbitCameraController, SceneFrame, Selection, ViewportBinding, ViewportContext, ViewportEvent,
    ViewportGesture, ViewportId, ViewportInput, WindowConfig, primitives,
    viewport_default_bindings,
};

use viewport_lib_ui::{
    Alignment, Frame, HostMap, HostSources, InputTranslator, Layout, Screen, ShapedTextMeasurer,
    UiState, Widget, WidgetKind, WidgetRegistry, custom_output, extend_frame, map_cursor,
    serde_json,
};

use viewport_lib_gizmo::ActiveView;
use viewport_lib_gizmo::{GizmoMode, ValueMode};
use viewport_lib_gizmo_ui::{GIZMO_KIND, GizmoDelta, GizmoFrame, GizmoResult, GizmoTool};

const LABELS: [&str; 4] = ["Perspective", "Top", "Front", "Right"];
const ROOT: u32 = 0;
/// Each view's gizmo pane. The tool tells the pane that drives its shared driver
/// by its id, so the four differ even though each sits on a screen of its own.
const PANES: [u32; 4] = [1, 2, 3, 4];

fn no_textures(_key: &str) -> Option<vpl::OverlayTextureId> {
    None
}
fn no_fonts(_key: &str) -> Option<vpl::FontHandle> {
    None
}

/// One quadrant: its camera and input, its toolkit screen and gizmo pane, the
/// renderer viewport it draws through, and the offscreen target the window blits
/// from.
struct View {
    camera: Camera,
    orbit: OrbitCameraController,
    input: ViewportInput,
    pane: u32,
    screen: Screen,
    ui: UiState,
    /// True when the toolkit took the pointer last frame: the left button and
    /// pointer moves are the toolkit's, not the camera's.
    grabbing: bool,
    id: ViewportId,
    target: OffscreenViewportTarget,
    blit: Option<BlitTexture>,
}

#[derive(Default)]
struct State {
    scene: Scene,
    selection: Selection,
    node: Option<NodeId>,
    base: Mat4,
    current: Mat4,
    views: Vec<View>,
    /// The one tool behind every view's pane.
    tool: Arc<GizmoTool>,
    registry: WidgetRegistry,
    translator: InputTranslator,
    active: ActiveView,
    last_active: Option<usize>,
    button_held: bool,
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

/// A screen holding one gizmo pane, covering the view.
fn pane_screen(pane: u32) -> Screen {
    Screen::new(
        Widget::new(ROOT, WidgetKind::Panel)
            .with_layout(Layout::Free)
            .with_child(
                Widget::custom(pane, GIZMO_KIND, serde_json::Value::Null).placed(
                    Alignment::TOP_LEFT,
                    [0.0, 0.0],
                    [640.0, 400.0],
                ),
            ),
    )
}

/// Compose a gizmo delta around a world pivot onto a base transform, with typed
/// values relative to the start pose or absolute, per `values`.
fn apply(delta: &GizmoDelta, pivot: Vec3, base: Mat4, values: ValueMode) -> Mat4 {
    let translate = Mat4::from_translation(Vec3::from(delta.translation));
    let rotate = Mat4::from_quat(Quat::from_array(delta.rotation));
    let scale = Mat4::from_scale(Vec3::from(delta.scale_values()));
    let to_pivot = Mat4::from_translation(-pivot);
    let from_pivot = Mat4::from_translation(pivot);
    let mut m = translate * from_pivot * rotate * scale * to_pivot * base;
    let base_pos = base.col(3);
    for (i, typed) in delta.position_override.iter().enumerate() {
        if let Some(value) = typed {
            m.col_mut(3)[i] = match values {
                ValueMode::Objective => *value,
                _ => base_pos[i] + *value,
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
                .with_title("viewport-lib-gizmo : ui, four views")
                .with_window_size(1280, 800),
            // The window's own scene stays empty; the shared scene lives in `State`.
            move |instance, device| {
                let cube = instance
                    .resources_mut()
                    .upload_mesh_data(device, &primitives::cube(1.0))
                    .unwrap();
                let blue = instance
                    .resources_mut()
                    .upload_material(Material::from_colour(vpl::Colour::linear_rgb(
                        0.4, 0.6, 0.9,
                    )))
                    .unwrap();
                let grey = instance
                    .resources_mut()
                    .upload_material(Material::from_colour(vpl::Colour::linear_rgb(
                        0.35, 0.35, 0.4,
                    )))
                    .unwrap();
                // The window's resolver feeds the tool, so its bindings carry the
                // tool's shortcuts.
                instance.set_bindings(gizmo_bindings());
                let mut s = setup_state.borrow_mut();
                let node = s.scene.add(Some(cube), Mat4::IDENTITY, blue);
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

                // Build the views once, each with a pane of the one tool.
                if s.views.is_empty() {
                    s.registry
                        .register(GIZMO_KIND, s.tool.clone())
                        .expect("the registry is empty");
                    for (i, (camera, rect)) in cameras().into_iter().zip(&s.rects).enumerate() {
                        s.views.push(View {
                            camera,
                            orbit: OrbitCameraController::new_stateless(),
                            input: ViewportInput::from_preset(BindingPreset::Default),
                            pane: PANES[i],
                            screen: pane_screen(PANES[i]),
                            ui: UiState::new(),
                            grabbing: false,
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

                // The view that takes the camera input: the hovered one, held for the
                // length of a drag or a gizmo session.
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

                // Mode, space and value mode belong to the tool, so they hold in
                // every view.
                let tool = s.tool.clone();
                for ev in &events {
                    match ev {
                        ViewportEvent::MouseButton { state, .. } => {
                            s.button_held = *state == ButtonState::Pressed;
                        }
                        ViewportEvent::FocusLost => s.button_held = false,
                        ViewportEvent::Key {
                            key: KeyCode::Backtick,
                            state: ButtonState::Pressed,
                            repeat: false,
                            ..
                        } => tool.set_value_mode(tool.value_mode().toggled()),
                        _ => {}
                    }
                }
                // Tab or Q cycles the mode, except during a session, where Tab
                // advances the numeric axis instead.
                if !tool.is_active() && action.is_active(Action::CycleGizmoMode) {
                    tool.set_mode(match tool.mode() {
                        GizmoMode::Translate => GizmoMode::Rotate,
                        GizmoMode::Rotate => GizmoMode::Scale,
                        _ => GizmoMode::Translate,
                    });
                }

                // Navigation. Only the active view owns the input; every view is
                // given every event and its resolver drops what is not its own. While
                // a view's toolkit held the pointer last frame, a left press or
                // pointer move is the toolkit's and does not also orbit; the wheel,
                // modifier changes and the other buttons still reach the camera, so
                // zooming and panning keep working over a handle.
                for (i, view) in s.views.iter_mut().enumerate() {
                    let (origin, size) = logical[i];
                    let driving = active == Some(i);
                    // The runner only delivers events while the window has them.
                    let ownership =
                        vpl_winit::ownership(true, driving, false, view.grabbing, !driving);
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

                // Run each view's toolkit screen with the window input moved into the
                // view's own pixels. The toolkit hovers only the pane under the
                // cursor, which is how the tool picks the pane that drives.
                let ui_events: Vec<ViewportEvent> = events
                    .iter()
                    .filter(|ev| !matches!(ev, ViewportEvent::Wheel { .. }))
                    .cloned()
                    .collect();
                let input = s.translator.frame(&ui_events);
                let centre = s.current.col(3).truncate();
                // The sources every view reads. This example has none, so each view's
                // map holds only its own frame: its camera, the selection, and the
                // window's unfiltered input, which carries the G / R / S keys even
                // while the camera has given up the pointer.
                let shared = HostMap::new();
                let mut ui_cursor = None;
                {
                    let measure = ShapedTextMeasurer::new(ctx.resources(), no_fonts);
                    for (i, view) in s.views.iter_mut().enumerate() {
                        let (origin, size) = logical[i];
                        let map = HostMap::over(&shared).with_context(
                            GizmoFrame::new(view.camera.clone())
                                .with_selection_centre(Some(centre))
                                .with_action_frame(action.clone()),
                        );
                        if let Some(w) = view.screen.root.find_mut(view.pane) {
                            w.size = size.into();
                        }
                        let mut local = input.clone();
                        local.pointer.cursor[0] -= origin.x;
                        local.pointer.cursor[1] -= origin.y;
                        let frame = Frame::new(size.into(), &measure, &s.registry)
                            .with_host(HostSources::default().with_map(&map));
                        let result = view.ui.update_frame(&mut view.screen, &local, &frame);
                        view.grabbing = result.capture.pointer;
                        // This view's ownership was decided last frame, so a press the
                        // toolkit takes with no hover frame before it (a touch, or a move
                        // and press in one frame) has already armed its camera's drag.
                        if action.pointer.pressed && view.grabbing {
                            view.input.cancel_gesture();
                        }
                        if active == Some(i) {
                            ui_cursor = Some(view.ui.cursor());
                        }
                        // Only the driving pane reports, so each change arrives once.
                        match custom_output::<GizmoResult>(&result.events, view.pane) {
                            Ok(Some(GizmoResult::Update(delta))) => {
                                let pivot = s.base.col(3).truncate();
                                s.current = apply(&delta, pivot, s.base, tool.value_mode());
                            }
                            Ok(Some(GizmoResult::Commit)) => s.base = s.current,
                            Ok(Some(GizmoResult::Cancel))
                            | Ok(Some(GizmoResult::ConstraintChanged)) => s.current = s.base,
                            _ => {}
                        }
                    }
                }
                if !tool.is_active() {
                    s.base = s.current;
                }
                s.scene.set_local_transform(node, s.current);
                s.scene.update_transforms();
                s.active
                    .end_frame(active, s.button_held || tool.is_active());
                if let Some(shape) = ui_cursor.as_ref().and_then(map_cursor) {
                    ctx.set_cursor(shape);
                }

                // One frame per view: the shared scene and that view's toolkit
                // overlays, which carry its gizmo.
                let items = s.scene.collect_render_items(&s.selection);
                // Offset from the window's own (empty) scene, which renders through
                // the same renderer, so the two never share a generation.
                let generation = s.scene.version() + (1 << 32);
                let frame_data: Vec<FrameData> = {
                    let measure = ShapedTextMeasurer::new(ctx.resources(), no_fonts);
                    s.views
                        .iter()
                        .enumerate()
                        .map(|(i, view)| {
                            let size = logical[i].1;
                            let camera = CameraFrame::from_camera(&view.camera, size.into())
                                .with_viewport_id(view.id)
                                .with_pixels_per_point(ppp);
                            let mut fd = FrameData::new(
                                camera,
                                SceneFrame::from_surface_items(items.clone()),
                            );
                            fd.scene.generation = generation;
                            fd.interaction.selection_generation = s.selection.version();
                            fd.interaction.outline_selected = true;
                            fd.viewport.show_grid = true;
                            fd.viewport.show_axes_indicator = true;
                            // The gizmo draws where the cube is now, through the view's
                            // own camera.
                            let map = HostMap::over(&shared).with_context(
                                GizmoFrame::new(view.camera.clone())
                                    .with_selection_centre(Some(s.current.col(3).truncate())),
                            );
                            let frame = Frame::new(size.into(), &measure, &s.registry)
                                .with_host(HostSources::default().with_map(&map));
                            let list = view.ui.render(&view.screen, &frame);
                            extend_frame(&list, &mut fd.overlays, &no_textures, &no_fonts);
                            fd.overlays.labels.push(
                                LabelItem::new(LABELS[i])
                                    .with_position([8.0, 8.0])
                                    .with_colour(vpl::Colour::linear_rgb(0.8, 0.8, 0.8)),
                            );
                            fd
                        })
                        .collect()
                };

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
