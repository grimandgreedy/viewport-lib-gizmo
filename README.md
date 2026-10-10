# viewport-lib-gizmo

A transform gizmo (move, rotate, scale) for [viewport-lib](https://github.com/grimandgreedy/viewport-lib)
scenes. It hit-tests the handles, projects screen drags into world-space
transforms, and builds overlay items that you submit to your own renderer. It
depends on `viewport-lib-types` and `glam` only, and never touches the renderer
or wgpu.

The world up axis is `+Z`, matching viewport-lib.

## Three layers

Use as much of the crate as you need.

1. **Maths.** Stateless functions over a `Camera`, a cursor, and `glam`:
   `project_drag_onto_axis`, `project_drag_onto_plane`,
   `project_drag_onto_rotation`, `project_drag_onto_screen_plane`,
   `compute_gizmo_scale`, `gizmo_center_for_pivot`, `cursor_ray`, the `Gizmo`
   hit-tester, and the `snap_*` helpers. Enough to build a gizmo of your own.
2. **Style and overlays.** `build_gizmo_overlays` turns gizmo state into overlay
   shapes and polylines in the look set by a `GizmoStyle`. `GizmoStyle::default()`,
   `thin()` and `bold()` are the built-in looks; every colour and dimension is a
   public field.
3. **Controller.** `ManipulationController` runs the drag and keyboard sessions
   (G / R / S, X / Y / Z constraints, typed numeric values, confirm and cancel)
   and reports a `TransformDelta`. `GizmoDriver` wraps it with the per-frame work:
   sizing the gizmo for the screen, hit-testing the cursor, and assembling the
   context. Most apps start here.

## Usage

Each frame, pass the driver the resolved input, the camera, and the world-space
centre of the selection, then act on the result:

```rust,ignore
use viewport_lib_gizmo::{
    FrameInput, GizmoDriver, GizmoStyle, ManipResult, build_gizmo_overlays,
};

let mut driver = GizmoDriver::new();
let style = GizmoStyle::default();

// Each frame:
let result = driver.update(FrameInput {
    action_frame: &action_frame,
    camera: &camera,
    viewport_size,
    cursor_viewport: pointer.cursor,
    pointer_delta: pointer.delta,
    selection_center: Some(centre),
    drag_started: pointer.drag_started,
    dragging: pointer.dragging,
    clicked: pointer.clicked,
});

match result {
    // Total change since the session began: apply it to the start pose.
    ManipResult::Update(delta) => current = apply(&delta, start),
    ManipResult::Commit => start = current,
    ManipResult::Cancel | ManipResult::ConstraintChanged => current = start,
    _ => {}
}

if let Some(centre) = driver.gizmo_center() {
    build_gizmo_overlays(
        &style,
        driver.mode(),
        camera.proj_matrix() * camera.view_matrix(),
        (camera.center - camera.eye_position()).normalize_or(glam::Vec3::NEG_Z),
        [viewport_size.x, viewport_size.y],
        centre,
        driver.gizmo_scale(),
        driver.hovered_axis(),
        glam::Quat::IDENTITY,
        &mut overlays.shapes,
        &mut overlays.polylines,
    );
}
```

`TransformDelta` is cumulative. Each `Update` holds the whole change since the
drag began, so you recompute the pose from the drag-start snapshot every frame.
Nothing drifts, and cancelling just restores the snapshot.

While a handle is hovered or a session is running (`driver.hovered_axis()` is
not `GizmoAxis::None`, or `driver.is_active()`), hold back the camera's left-drag
so orbit and the gizmo do not both act on the same drag.

### Value mode

`ValueMode` is `Relative` (typed values are offsets from the start pose) or
`Objective` (typed values are absolute world values). The driver tracks the mode
and shows it in the readout; your app decides how to apply the numeric overrides
in `TransformDelta`.

### Snapping

```rust,ignore
let mut snap = SnapConfig::default();
snap.translation = Some(0.25);
snap.rotation = Some(std::f32::consts::PI / 12.0);
let driver = GizmoDriver::new().with_snap(snap);
```

## The `ui` feature

`ui` adds the `ui` module, which hosts the gizmo as a
[viewport-lib-ui](https://github.com/grimandgreedy/viewport-lib-ui) `Custom`
widget. `GizmoTool` paints the handles in the same pass as the rest of the UI and
shares one pointer capture with the panels, so a click on a panel never reaches
the gizmo or the camera. Results come back as a typed `GizmoResult`. The module
also has a readout panel: an x / y / z vector field with a relative / objective
toggle and a badge for the current mode.

The feature pulls only `viewport-lib-ui-core` and `viewport-lib-ui-document`,
which are renderer-free, so it still does not depend on viewport-lib or wgpu.

```toml
viewport-lib-gizmo = { path = "...", features = ["ui"] }
```

## Examples

```sh
cargo run --example gizmo_basic                 # ViewportApp, one cube, default style
cargo run --example gizmo_styling               # eframe, sidebar restyles the gizmo live
cargo run --example gizmo_ui --features ui      # the gizmo as a viewport-lib-ui widget
```

In all of them, Tab or Q cycles the mode and backtick toggles relative /
objective values. G, R and S start a keyboard session.

## Building

The crate uses path dependencies, so it expects `viewport-lib` and
`viewport-lib-ui` checked out next to it:

```text
viewport-lib/
viewport-lib-ui/
viewport-lib-gizmo/
```

## License

GPL-3.0-only.
