//! A headless rig that drives a `GizmoDriver` the way an app does: a camera, a
//! cursor in viewport pixels, and the per-frame pointer flags.

#![allow(dead_code)]

use glam::{DVec3, Quat, Vec2, Vec3};
use viewport_lib_gizmo::{FrameInput, GizmoDriver, ManipResult, TransformDelta};
use viewport_lib_types::camera::Camera;
use viewport_lib_types::input::{Action, ActionFrame, ResolvedActionState};

pub const VP: Vec2 = Vec2::new(1200.0, 800.0);

/// Looking down at the origin from the -Y, +X side, about 30 degrees up.
pub fn three_quarter() -> Camera {
    Camera::look_at(DVec3::new(6.0, -8.0, 5.0), DVec3::ZERO, Vec3::Z).with_aspect(VP.x / VP.y)
}

/// Straight down the -Z axis with +Y up the screen, so world X is screen right.
pub fn top() -> Camera {
    Camera::look_at(DVec3::new(0.0, 0.0, 10.0), DVec3::ZERO, Vec3::Y).with_aspect(VP.x / VP.y)
}

/// Nearly level with the XY plane, looking along +Y.
pub fn low() -> Camera {
    Camera::look_at(DVec3::new(0.0, -10.0, 1.0), DVec3::ZERO, Vec3::Z).with_aspect(VP.x / VP.y)
}

/// Project a world point to a viewport pixel (top-left origin).
pub fn to_px(camera: &Camera, world: Vec3) -> Vec2 {
    let clip = (camera.proj_matrix() * camera.view_matrix()) * world.extend(1.0);
    let ndc = clip.truncate() / clip.w;
    Vec2::new((ndc.x * 0.5 + 0.5) * VP.x, (0.5 - ndc.y * 0.5) * VP.y)
}

/// An action frame with `actions` pressed.
pub fn pressed(actions: &[Action]) -> ActionFrame {
    let mut frame = ActionFrame::default();
    for &a in actions {
        frame.actions.insert(a, ResolvedActionState::Pressed);
    }
    frame
}

/// An action frame carrying typed characters.
pub fn typed(chars: &str) -> ActionFrame {
    let mut frame = ActionFrame::default();
    frame.typed_chars.extend(chars.chars());
    frame
}

/// The delta of an `Update`, or a panic naming what came back instead.
pub fn update(result: ManipResult) -> TransformDelta {
    match result {
        ManipResult::Update(delta) => delta,
        other => panic!("expected an update, got {other:?}"),
    }
}

pub fn assert_near(a: Vec3, b: Vec3, tol: f32, what: &str) {
    assert!(
        (a - b).length() <= tol,
        "{what}: got {a:?}, expected {b:?} (tolerance {tol})"
    );
}

pub fn is_finite(d: &TransformDelta) -> bool {
    d.translation.is_finite() && d.rotation.is_finite() && d.scale.is_finite()
}

/// One driver, one camera, one selection, and the cursor carried between frames.
pub struct Rig {
    pub driver: GizmoDriver,
    pub camera: Camera,
    pub centre: Option<Vec3>,
    pub orientation: Quat,
    pub cursor: Option<Vec2>,
    pub button_down: bool,
}

impl Rig {
    /// A rig with the selection at `centre`, sized by one idle frame.
    pub fn new(camera: Camera, centre: Vec3) -> Self {
        Self::with_driver(GizmoDriver::new(), camera, centre)
    }

    pub fn with_driver(driver: GizmoDriver, camera: Camera, centre: Vec3) -> Self {
        let mut rig = Self {
            driver,
            camera,
            centre: Some(centre),
            orientation: Quat::IDENTITY,
            cursor: None,
            button_down: false,
        };
        rig.idle();
        rig
    }

    /// The gizmo's world scale, as last resolved.
    pub fn scale(&self) -> f32 {
        self.driver.gizmo_scale()
    }

    pub fn px(&self, world: Vec3) -> Vec2 {
        to_px(&self.camera, world)
    }

    pub fn centre_px(&self) -> Vec2 {
        self.px(self.centre.expect("a selection"))
    }

    /// Run one frame. The pointer delta is the cursor's move since the last
    /// frame, or `fallback_delta` when either end is outside the viewport.
    pub fn frame_with(
        &mut self,
        frame: &ActionFrame,
        cursor: Option<Vec2>,
        fallback_delta: Vec2,
        drag_started: bool,
        clicked: bool,
    ) -> ManipResult {
        let pointer_delta = match (self.cursor, cursor) {
            (Some(prev), Some(now)) => now - prev,
            _ => fallback_delta,
        };
        self.cursor = cursor;
        self.driver.update(FrameInput {
            action_frame: frame,
            camera: &self.camera,
            viewport_size: VP,
            cursor_viewport: cursor,
            pointer_delta,
            selection_center: self.centre,
            selection_orientation: self.orientation,
            drag_started,
            dragging: self.button_down,
            clicked,
        })
    }

    /// A frame with no input at the current cursor.
    pub fn idle(&mut self) -> ManipResult {
        self.frame_with(
            &ActionFrame::default(),
            self.cursor,
            Vec2::ZERO,
            false,
            false,
        )
    }

    /// Move the cursor without a button held.
    pub fn hover(&mut self, at: Vec2) -> ManipResult {
        self.frame_with(&ActionFrame::default(), Some(at), Vec2::ZERO, false, false)
    }

    /// Put the cursor at `at` (one hover frame), then press and start a drag
    /// there.
    pub fn press(&mut self, at: Vec2) -> ManipResult {
        self.hover(at);
        self.button_down = true;
        self.frame_with(&ActionFrame::default(), Some(at), Vec2::ZERO, true, false)
    }

    /// Move the cursor with the button held.
    pub fn drag_to(&mut self, at: Vec2) -> ManipResult {
        self.frame_with(&ActionFrame::default(), Some(at), Vec2::ZERO, false, false)
    }

    /// Move to `to` in `steps` even frames, returning the last result.
    pub fn drag_path(&mut self, to: Vec2, steps: usize) -> ManipResult {
        let from = self.cursor.expect("a cursor to drag from");
        let mut last = ManipResult::None;
        for i in 1..=steps {
            last = self.drag_to(from.lerp(to, i as f32 / steps as f32));
        }
        last
    }

    /// Let go of the button where the cursor is.
    pub fn release(&mut self) -> ManipResult {
        self.button_down = false;
        self.idle()
    }

    /// A frame with `actions` pressed at the current cursor.
    pub fn keys(&mut self, actions: &[Action]) -> ManipResult {
        self.frame_with(&pressed(actions), self.cursor, Vec2::ZERO, false, false)
    }

    /// A frame with `chars` typed at the current cursor.
    pub fn type_chars(&mut self, chars: &str) -> ManipResult {
        self.frame_with(&typed(chars), self.cursor, Vec2::ZERO, false, false)
    }

    /// A click (press and release without a drag) at the current cursor.
    pub fn click(&mut self) -> ManipResult {
        self.frame_with(
            &ActionFrame::default(),
            self.cursor,
            Vec2::ZERO,
            false,
            true,
        )
    }
}
