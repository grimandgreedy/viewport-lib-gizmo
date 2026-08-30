//! A transform gizmo in an eframe app, with a sidebar that restyles it live.
//!
//! Mirrors viewport-lib's `eframe_minimal` embedding (a `ViewportInstance`
//! rendering into an offscreen texture that egui displays), and adds a right
//! sidebar: preset buttons (default / thin / bold) and per-axis colour pickers
//! that edit the [`GizmoStyle`]. Drag a handle to move the cube; drag the
//! background to orbit.
//!
//! Run with `cargo run --example gizmo_styling`.

use eframe::{egui, wgpu};
use glam::{Mat4, Quat, Vec2, Vec3};
use viewport_lib as vpl;
use vpl::input::adapters::from_egui;
use vpl::{
    Material, Modifiers, NodeId, OrbitCameraController, ViewportContext, ViewportEvent,
    ViewportInstance, primitives,
};

use viewport_lib_gizmo::{
    AxisColour, FrameInput, GizmoAxis, GizmoDriver, GizmoStyle, ManipResult, TransformDelta,
    build_gizmo_overlays,
};

/// Compose a transform delta around a world pivot onto a base transform.
fn apply_delta(delta: &TransformDelta, pivot: Vec3, base: Mat4) -> Mat4 {
    let rot = Mat4::from_quat(delta.rotation);
    let scale = Mat4::from_scale(delta.scale);
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

/// A colour picker for one axis: edits the base colour and derives the hover
/// colour from it.
fn axis_colour_picker(ui: &mut egui::Ui, label: &str, col: &mut AxisColour) {
    let mut rgb = [col.base[0], col.base[1], col.base[2]];
    ui.horizontal(|ui| {
        ui.label(label);
        if ui.color_edit_button_rgb(&mut rgb).changed() {
            col.base = [rgb[0], rgb[1], rgb[2], col.base[3]];
            col.hover = [
                (rgb[0] * 1.3).min(1.0),
                (rgb[1] * 1.3).min(1.0),
                (rgb[2] * 1.3).min(1.0),
                col.base[3],
            ];
        }
    });
}

fn main() -> eframe::Result {
    eframe::run_native(
        "viewport-lib-gizmo : styling",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default().with_inner_size([1280.0, 720.0]),
            ..Default::default()
        },
        Box::new(|cc| {
            let rs = cc
                .wgpu_render_state
                .as_ref()
                .expect("wgpu backend required");
            let mut session = ViewportInstance::new(&rs.device, rs.target_format);
            let cube = session
                .resources_mut()
                .upload_mesh_data(&rs.device, &primitives::cube(1.0))
                .unwrap();
            let node = session.scene_mut().add(
                Some(cube),
                Mat4::IDENTITY,
                Material::from_colour([0.4, 0.6, 0.9]),
            );
            session.camera_mut().distance = 8.0;

            Ok(Box::new(App {
                session,
                orbit: OrbitCameraController::viewport_all(),
                node,
                base: Mat4::IDENTITY,
                current: Mat4::IDENTITY,
                driver: GizmoDriver::new(),
                style: GizmoStyle::default(),
                target: None,
            }))
        }),
    )
}

/// The offscreen render target and its egui texture registration.
struct Target {
    _texture: wgpu::Texture,
    view: wgpu::TextureView,
    id: egui::TextureId,
    size: [u32; 2],
}

struct App {
    session: ViewportInstance,
    orbit: OrbitCameraController,
    node: NodeId,
    base: Mat4,
    current: Mat4,
    driver: GizmoDriver,
    style: GizmoStyle,
    target: Option<Target>,
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        let rs = frame.wgpu_render_state().expect("wgpu backend required");

        egui::SidePanel::right("style_panel")
            .resizable(false)
            .show(ctx, |ui| {
                ui.heading("Gizmo style");
                ui.separator();
                ui.label("Presets");
                ui.horizontal(|ui| {
                    if ui.button("Default").clicked() {
                        self.style = GizmoStyle::default();
                    }
                    if ui.button("Thin").clicked() {
                        self.style = GizmoStyle::thin();
                    }
                    if ui.button("Bold").clicked() {
                        self.style = GizmoStyle::bold();
                    }
                });
                ui.separator();
                ui.label("Axis colours");
                axis_colour_picker(ui, "X", &mut self.style.x);
                axis_colour_picker(ui, "Y", &mut self.style.y);
                axis_colour_picker(ui, "Z", &mut self.style.z);
                ui.separator();
                ui.label("Drag a handle to move the cube.");
                ui.label("Drag the background to orbit.");
            });

        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ctx, |ui| {
                let (rect, response) =
                    ui.allocate_exact_size(ui.available_size(), egui::Sense::click_and_drag());
                let ppp = ui.ctx().pixels_per_point();
                let size = [
                    (rect.width() * ppp).round().max(1.0) as u32,
                    (rect.height() * ppp).round().max(1.0) as u32,
                ];

                // (Re)create the offscreen target when the panel size changes.
                if self.target.as_ref().map_or(true, |t| t.size != size) {
                    let texture = rs.device.create_texture(&wgpu::TextureDescriptor {
                        label: Some("viewport_offscreen"),
                        size: wgpu::Extent3d {
                            width: size[0],
                            height: size[1],
                            depth_or_array_layers: 1,
                        },
                        mip_level_count: 1,
                        sample_count: 1,
                        dimension: wgpu::TextureDimension::D2,
                        format: rs.target_format,
                        usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                            | wgpu::TextureUsages::TEXTURE_BINDING,
                        view_formats: &[],
                    });
                    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
                    let id = rs.renderer.write().register_native_texture(
                        &rs.device,
                        &view,
                        wgpu::FilterMode::Linear,
                    );
                    self.target = Some(Target {
                        _texture: texture,
                        view,
                        id,
                        size,
                    });
                }

                let vp_ctx = ViewportContext {
                    hovered: response.hovered(),
                    focused: response.has_focus(),
                    viewport_size: [rect.width(), rect.height()],
                };
                self.session.begin_frame(vp_ctx);
                self.session.set_pixels_per_point(ppp);
                let origin = Vec2::new(rect.left(), rect.top());
                ui.input(|i| {
                    self.session
                        .handle_event(ViewportEvent::ModifiersChanged(Modifiers {
                            alt: i.modifiers.alt,
                            shift: i.modifiers.shift,
                            ctrl: i.modifiers.command,
                        }));
                    for event in &i.events {
                        if let Some(ev) = from_egui(event, origin) {
                            self.session.handle_event(ev);
                        }
                    }
                });

                // Resolve input, then drive the gizmo.
                let af = self.session.resolve().clone();
                let camera = self.session.camera().clone();
                let viewport = [rect.width(), rect.height()];
                let center = self.current.col(3).truncate();

                let result = self.driver.update(FrameInput {
                    action_frame: &af,
                    camera: &camera,
                    viewport_size: Vec2::new(viewport[0], viewport[1]),
                    cursor_viewport: af.pointer.cursor,
                    pointer_delta: af.pointer.delta,
                    selection_center: Some(center),
                    drag_started: af.pointer.drag_started,
                    dragging: af.pointer.dragging,
                    clicked: af.pointer.clicked,
                });
                match result {
                    ManipResult::Update(delta) => {
                        let pivot = self.base.col(3).truncate();
                        self.current = apply_delta(&delta, pivot, self.base);
                        self.session
                            .scene_mut()
                            .set_local_transform(self.node, self.current);
                    }
                    ManipResult::Commit => self.base = self.current,
                    ManipResult::Cancel | ManipResult::ConstraintChanged => {
                        self.current = self.base;
                        self.session
                            .scene_mut()
                            .set_local_transform(self.node, self.current);
                    }
                    ManipResult::None => {
                        if !self.driver.is_active() {
                            self.base = self.current;
                        }
                    }
                    _ => {}
                }

                // Assemble the frame every tick (this rebuilds the scene sub-frame
                // and clears the overlay list). Orbit only when a handle is not
                // grabbed; while grabbing, assemble without moving the camera.
                let grabbing =
                    self.driver.is_active() || self.driver.hovered_axis() != GizmoAxis::None;
                if grabbing {
                    self.session.frame(vp_ctx);
                } else {
                    self.session.update_orbit(&mut self.orbit);
                }

                // Draw the gizmo in the current style, after assembly so it is not
                // cleared.
                if let Some(gizmo_center) = self.driver.gizmo_center() {
                    let scale = self.driver.gizmo_scale();
                    let view_proj = camera.proj_matrix() * camera.view_matrix();
                    let forward =
                        (camera.center - camera.eye_position()).normalize_or(Vec3::NEG_Z);
                    let mode = self.driver.gizmo().mode;
                    let hovered = self.driver.hovered_axis();
                    let fd = self.session.frame_data_mut();
                    build_gizmo_overlays(
                        &self.style,
                        mode,
                        view_proj,
                        forward,
                        viewport,
                        gizmo_center,
                        scale,
                        hovered,
                        Quat::IDENTITY,
                        &mut fd.overlays.shapes,
                        &mut fd.overlays.polylines,
                    );
                }

                let target = self.target.as_ref().unwrap();
                let cmd = self.session.render(&rs.device, &rs.queue, &target.view);
                rs.queue.submit(std::iter::once(cmd));
                ui.painter().image(
                    target.id,
                    rect,
                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    egui::Color32::WHITE,
                );
            });

        ctx.request_repaint();
    }
}
