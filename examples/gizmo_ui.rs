//! The gizmo hosted as a viewport-lib-ui `Custom` widget.
//!
//! Unlike `gizmo_basic`, which submits the gizmo's overlay items straight to the
//! renderer, this runs the gizmo through the viewport-lib-ui toolkit: the handles
//! are painted by a `GizmoTool` custom widget in the same pass as the rest of the
//! UI, the pointer is arbitrated once (a panel over the viewport and the camera
//! yield to each other through one capture signal), and the numeric readout is a
//! vector-editor field with a relative/objective segmented toggle beside it.
//!
//! The 3D maths, the camera, and the controller stay in the gizmo crate; the tool
//! only bridges them to the toolkit. Drag a handle to move the cube; drag empty
//! space to orbit; a click on the panel neither orbits nor moves the cube. The
//! pointer reads as an open hand over a handle and a closed one while dragging it.
//! The readout counts the drag in relative mode and reads the cube's own world
//! position, rotation, or scale in objective mode.
//! Press G, R, or S to start a keyboard move / rotate / scale session; Tab or Q
//! cycles the gizmo mode, backtick toggles relative / objective, H cycles the readout
//! between always shown, shown only while a session is running, and hidden.
//!
//! The readout carries a G / R / S badge for the mode it is reading, matching the key
//! that starts that session.
//!
//! Run with `cargo run --example gizmo_ui --features ui`.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;
use std::sync::Arc;

use glam::{Mat4, Quat, Vec3};
use viewport_lib as vpl;
use vpl::{
    AppConfig, ButtonState, KeyCode, Material, NodeId, OrbitCameraController, PointerOwnership,
    ViewportApp, ViewportContext, ViewportEvent, WindowConfig, forward_to_viewport, primitives,
};

use viewport_lib_ui::{
    Anchor, Colour, Cursor, Frame, HostMap, HostSources, InputTranslator, Layout, Screen,
    ShapedTextMeasurer, Theme, ThemeMode, UiEvent, UiState, Value, Visibility, Widget, WidgetKind,
    WidgetRegistry, custom_output, extend_frame, map_cursor, roles, serde_json,
};

use viewport_lib_gizmo::ui::{
    GIZMO_KIND, GizmoDelta, GizmoFrame, GizmoResult, GizmoTool, ObjectPose, ReadoutIds,
    ReadoutWhen, readout_panel, readout_values_in, set_mode_badge, set_readout,
    value_mode_from_index, value_mode_index,
};
use viewport_lib_gizmo::{GizmoMode, ValueMode};

// Widget ids in the screen.
const ROOT: u32 = 0;
const GIZMO_PANE: u32 = 1;
const INFO_PANEL: u32 = 10;
const READOUT_ROW: u32 = 11;
const READOUT: u32 = 12;
const MODE_SEG: u32 = 13;
const MODE_BADGE: u32 = 14;

/// No textures and no custom fonts: the example draws no images and uses the house font.
fn no_textures(_key: &str) -> Option<vpl::OverlayTextureId> {
    None
}
fn no_fonts(_key: &str) -> Option<vpl::FontHandle> {
    None
}

/// A compact dark palette covering the toolkit's role set, so the readout field and
/// the segmented control resolve to real colours.
fn dark_theme() -> Theme {
    let mut roles_map: BTreeMap<String, Colour> = BTreeMap::new();
    let mut set = |role: &str, colour: Colour| {
        roles_map.insert(role.to_string(), colour);
    };
    set(roles::SURFACE, [0.12, 0.13, 0.16, 1.0]);
    set(roles::SURFACE_VARIANT, [0.18, 0.19, 0.23, 1.0]);
    set(roles::ON_SURFACE, [0.86, 0.88, 0.92, 1.0]);
    set(roles::TEXT, [0.90, 0.92, 0.96, 1.0]);
    set(roles::TEXT_MUTED, [0.60, 0.63, 0.70, 1.0]);
    set(roles::ACCENT, [0.30, 0.62, 0.95, 1.0]);
    set(roles::ON_ACCENT, [0.02, 0.05, 0.08, 1.0]);
    set(roles::OUTLINE, [0.30, 0.33, 0.40, 1.0]);
    set(roles::SHADOW, [0.0, 0.0, 0.0, 0.5]);
    set(roles::SUCCESS, [0.35, 0.78, 0.45, 1.0]);
    set(roles::WARNING, [0.95, 0.72, 0.25, 1.0]);
    set(roles::ERROR, [0.92, 0.35, 0.38, 1.0]);
    set(roles::INFO, [0.45, 0.65, 0.95, 1.0]);
    Theme {
        name: "gizmo dark".to_string(),
        mode: ThemeMode::Dark,
        roles: roles_map,
        metrics: BTreeMap::new(),
        base: None,
        density: 1.0,
    }
}

/// Build the screen: a full-viewport gizmo pane under a small readout panel.
fn build_screen() -> Screen {
    let gizmo = Widget::custom(GIZMO_PANE, GIZMO_KIND, serde_json::Value::Null).placed(
        Anchor::TopLeft,
        [0.0, 0.0],
        [1280.0, 720.0],
    );

    // The readout panel is defined by the gizmo crate (styling and padding are its
    // defaults); the host only places it.
    let panel = readout_panel(
        ReadoutIds {
            panel: INFO_PANEL,
            row: READOUT_ROW,
            badge: MODE_BADGE,
            field: READOUT,
            mode: MODE_SEG,
        },
        GizmoMode::Translate,
        ValueMode::Relative,
    )
    .placed(Anchor::BottomCentre, [0.0, -16.0], [0.0, 0.0]);

    // The gizmo pane is first so the panel paints over it and wins the pointer where
    // they overlap.
    Screen::new(
        Widget::new(ROOT, WidgetKind::Panel)
            .with_layout(Layout::Free)
            .with_children([gizmo, panel]),
    )
}

/// Compose a gizmo delta around a world pivot onto a base transform. A number typed
/// during a session arrives as an override for its axis and replaces the dragged
/// value there: relative to the start pose, or as an absolute world coordinate in
/// objective mode.
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

/// The whole app, shared between the input and frame callbacks.
struct App {
    screen: Screen,
    ui: UiState,
    tool: Arc<GizmoTool>,
    registry: WidgetRegistry,
    theme: Theme,
    node: Option<NodeId>,
    base: Mat4,
    current: Mat4,
    last_delta: GizmoDelta,
    grabbing: bool,
    /// The shape the UI asked for last frame, applied in the render callback (the
    /// input callback has no window to set it on).
    cursor: Cursor,
}

fn main() {
    // The registry holds the handler shared; keep a clone to set its frame and read it.
    let tool = Arc::new(GizmoTool::new());
    let mut registry = WidgetRegistry::new();
    registry
        .register(GIZMO_KIND, tool.clone())
        .expect("the registry is empty");

    let app = Rc::new(RefCell::new(App {
        screen: build_screen(),
        ui: UiState::new(),
        tool,
        registry,
        theme: dark_theme(),
        node: None,
        base: Mat4::IDENTITY,
        current: Mat4::IDENTITY,
        last_delta: GizmoDelta::default(),
        grabbing: false,
        cursor: Cursor::Default,
    }));

    ViewportApp::new(AppConfig::default())
        .window_with_input(
            WindowConfig::default()
                .with_title("viewport-lib-gizmo : ui frontend")
                .with_window_size(1280, 720),
            {
                let app = app.clone();
                move |session, device| {
                    let cube = session
                        .resources_mut()
                        .upload_mesh_data(device, &primitives::cube(1.0))
                        .unwrap();
                    let blue = session
                        .resources_mut()
                        .upload_material(Material::from_colour(vpl::Colour::linear_rgb(
                            0.4, 0.6, 0.9,
                        )))
                        .unwrap();
                    let transform = Mat4::from_translation(Vec3::ZERO);
                    let node = session.scene_mut().add(Some(cube), transform, blue);
                    session.camera_mut().set_orbit_distance(8.0);
                    let mut a = app.borrow_mut();
                    a.node = Some(node);
                    a.base = transform;
                    a.current = transform;
                }
            },
            {
                let app = app.clone();
                let mut orbit = OrbitCameraController::viewport_all();
                let mut translator = InputTranslator::new();
                move |ictx| {
                    let vp_px = ictx.viewport_size();
                    // While vpl-ui held the pointer last frame, a left press or pointer
                    // move is its own and does not also orbit.
                    let ownership = if app.borrow().grabbing {
                        PointerOwnership::Inside
                    } else {
                        PointerOwnership::Owned
                    };
                    orbit.begin_frame(ViewportContext::with_ownership(ownership, vp_px));

                    let raw = ictx.events().to_vec();

                    // Input router. The host owns the camera; vpl-ui owns the panels and the
                    // gizmo. The wheel is always the camera's zoom (nothing in this UI scrolls),
                    // so it goes straight to the camera and is withheld from vpl-ui, whose coarse
                    // pointer capture would otherwise swallow it while a handle is hovered.
                    // Pointer and key events go to vpl-ui; the camera's orbit and pan get them
                    // only when vpl-ui did not capture the pointer last frame.
                    let mut ui_events = Vec::with_capacity(raw.len());
                    for ev in &raw {
                        if let ViewportEvent::Key {
                            key,
                            state: ButtonState::Pressed,
                            repeat: false,
                            ..
                        } = ev
                        {
                            let a = app.borrow();
                            match key {
                                KeyCode::Tab | KeyCode::Q => {
                                    let next = match a.tool.mode() {
                                        GizmoMode::Translate => GizmoMode::Rotate,
                                        GizmoMode::Rotate => GizmoMode::Scale,
                                        _ => GizmoMode::Translate,
                                    };
                                    a.tool.set_mode(next);
                                }
                                KeyCode::Backtick => {
                                    a.tool.set_value_mode(a.tool.value_mode().toggled())
                                }
                                KeyCode::H => {
                                    // Cycle the readout: always shown, shown only while a drag or
                                    // a keyboard session is running, hidden.
                                    let tool = &a.tool;
                                    match (tool.show_readout(), tool.readout_when()) {
                                        (true, ReadoutWhen::Always) => {
                                            tool.set_readout_when(ReadoutWhen::WhileActive)
                                        }
                                        (true, _) => tool.set_show_readout(false),
                                        _ => {
                                            tool.set_show_readout(true);
                                            tool.set_readout_when(ReadoutWhen::Always);
                                        }
                                    }
                                }
                                _ => {}
                            }
                        }
                        ictx.forward(ev.clone());

                        // The only thing the gizmo (or a panel) consumes is the left-button
                        // drag, so that is the only thing withheld from the camera while the UI
                        // holds the pointer. The wheel, modifier changes, and other buttons
                        // always reach the camera, so zoom, cmd+scroll orbit, and shift+scroll
                        // pan keep working even with the cursor over a handle.
                        if forward_to_viewport(ev, ownership) {
                            orbit.push_event(ev.clone());
                        }

                        // vpl-ui sees pointer, buttons, keys, and modifier changes so its panels
                        // hover and click and the gizmo drag runs. The wheel is the camera's
                        // alone in this app (nothing in the UI scrolls).
                        if !matches!(ev, ViewportEvent::Wheel { .. }) {
                            ui_events.push(ev.clone());
                        }
                    }
                    orbit.apply_to_camera(ictx.camera_mut());
                    let camera = ictx.camera_mut().clone();
                    // Resolve this frame's input now that every event has been forwarded, so
                    // the gizmo sees the keyboard g/r/s, confirm/cancel, and numeric entry.
                    let action = ictx.resolve().clone();
                    let input = translator.frame(&ui_events);
                    let measure = ShapedTextMeasurer::new(ictx.resources(), no_fonts);

                    let mut a = app.borrow_mut();
                    let App {
                        screen,
                        ui,
                        tool,
                        registry,
                        theme,
                        node,
                        base,
                        current,
                        last_delta,
                        grabbing,
                        cursor,
                        ..
                    } = &mut *a;

                    let centre = current.col(3).truncate();
                    let map = HostMap::new().with_context(
                        GizmoFrame::new(camera)
                            .with_selection_centre(Some(centre))
                            .with_action_frame(action),
                    );
                    // Keep the pane covering the viewport as the window resizes.
                    if let Some(w) = screen.root.find_mut(GIZMO_PANE) {
                        w.size = vp_px;
                    }

                    let frame = Frame::new(vp_px, &measure, registry)
                        .with_theme(theme)
                        .with_host(HostSources::default().with_map(&map));
                    let result = ui.update_frame(screen, &input, &frame);

                    if node.is_some() {
                        match custom_output::<GizmoResult>(&result.events, GIZMO_PANE) {
                            Ok(Some(GizmoResult::Update(delta))) => {
                                *last_delta = delta;
                                let pivot = base.col(3).truncate();
                                *current = apply(&delta, pivot, *base, tool.value_mode());
                            }
                            Ok(Some(GizmoResult::Commit)) => {
                                *base = *current;
                                *last_delta = GizmoDelta::default();
                            }
                            Ok(Some(GizmoResult::Cancel))
                            | Ok(Some(GizmoResult::ConstraintChanged)) => {
                                *current = *base;
                            }
                            _ => {
                                if !tool.is_active() {
                                    *base = *current;
                                }
                            }
                        }
                    }

                    // A pick on the value-mode segmented toggles the tool's value mode.
                    for event in &result.events {
                        if let UiEvent::ValueChanged { id, value } = event {
                            if *id == MODE_SEG {
                                if let Value::Index(index) = value {
                                    tool.set_value_mode(value_mode_from_index(*index));
                                }
                            }
                        }
                    }

                    // Refresh the readout and keep the segmented in sync with the tool. Relative
                    // mode counts the drag; objective reads the cube's own world values, so the
                    // field needs the pose the host is holding.
                    let pose = ObjectPose::from_matrix(*current);
                    set_mode_badge(&mut screen.root, MODE_BADGE, tool.mode());
                    set_readout(
                        &mut screen.root,
                        READOUT,
                        readout_values_in(tool.mode(), last_delta, tool.value_mode(), &pose),
                    );
                    if let Some(w) = screen.root.find_mut(MODE_SEG) {
                        if let WidgetKind::Segmented { selected, .. } = &mut w.kind {
                            *selected = value_mode_index(tool.value_mode());
                        }
                    }

                    // The show_readout flag on the tool gates the panel's visibility.
                    if let Some(w) = screen.root.find_mut(INFO_PANEL) {
                        w.visibility = if tool.readout_visible() {
                            Visibility::Shown
                        } else {
                            Visibility::Hidden
                        };
                    }

                    // Suppress orbit next frame whenever the UI took the pointer (a hovered
                    // handle, a live drag, or a click on the panel).
                    *grabbing = result.capture.pointer;
                    // The shape the UI wants for what the pointer is over: the gizmo's open hand
                    // over a handle, the closed one while dragging, whatever a control under it
                    // asks for otherwise. Applied in the render callback, which has the window.
                    *cursor = ui.cursor();
                }
            },
            {
                let app = app.clone();
                move |ctx| {
                    let vp_px = ctx.viewport_size();

                    // Push the object's current transform to the scene first, before the text
                    // measurer borrows the frame's resources.
                    {
                        let a = app.borrow();
                        if let Some(node) = a.node {
                            ctx.scene_mut().set_local_transform(node, a.current);
                        }
                    }

                    let measure = ShapedTextMeasurer::new(ctx.resources(), no_fonts);
                    let a = app.borrow();
                    if let Some(shape) = map_cursor(&a.cursor) {
                        ctx.set_cursor(shape);
                    }
                    // The gizmo draws where the cube is now, through the camera this frame
                    // renders with.
                    let map = HostMap::new().with_context(
                        GizmoFrame::new(ctx.camera().clone())
                            .with_selection_centre(Some(a.current.col(3).truncate())),
                    );
                    let frame = Frame::new(vp_px, &measure, &a.registry)
                        .with_theme(&a.theme)
                        .with_host(HostSources::default().with_map(&map));
                    let list = a.ui.render(&a.screen, &frame);
                    extend_frame(&list, ctx.overlays_mut(), &no_textures, &no_fonts);
                }
            },
        )
        .run();
}
