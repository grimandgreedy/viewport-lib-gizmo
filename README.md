# viewport-lib-gizmo

A move / rotate / scale transform gizmo for [viewport-lib](https://github.com/grimandgreedy/viewport-lib) scenes. It hit-tests the handles, turns screen drags into world-space transforms, and builds overlay items for you to submit to your renderer. Its only dependencies are `viewport-lib-types` and `glam`: it never touches wgpu.

<img src="https://github.com/grimandgreedy/viewport-lib-gizmo/blob/master/assets/gizmo2.png?raw=true" alt="viewport-lib-gizmo" />

## Features

- Drag handles for axes, planes and the screen plane, with hover highlighting.
- Blender-style keyboard sessions: G / R / S to start, X / Y / Z to constrain, typed numeric values, confirm and cancel.
- World or local space, and a choice of pivot for multi-selections.
- Translation, rotation and scale snapping.
- Perspective and orthographic cameras, and one gizmo shared across several views.
- Every colour and dimension is a field on `GizmoStyle`.

The world up axis is `+Z`, matching viewport-lib.

## Layers

Use as much of the crate as you need.

1. **Maths.** Stateless functions over a `Camera`, a cursor and `glam`: the `project_drag_onto_*` functions, `cursor_ray`, `gizmo_scale_for_camera`, `gizmo_centre_for_pivot`, the `Gizmo` hit-tester and the `snap_*` helpers. Enough to build your own gizmo.
2. **Overlays.** `build_gizmo_overlays` turns gizmo state into overlay shapes and polylines in the look set by a `GizmoStyle` (`default()`, `thin()` or `bold()`, or your own).
3. **Controller.** `ManipulationController` runs drag and keyboard sessions and reports a `TransformDelta`. `GizmoDriver` wraps it with the per-frame work: sizing the gizmo, hit-testing the cursor and tracking the hovered handle. Most apps start here.

## Usage

Each frame, give the driver the resolved input, the camera and the selection centre, then act on the result:

```rust,ignore
use viewport_lib_gizmo::{
    FrameInput, GizmoDriver, GizmoStyle, ManipResult, build_gizmo_overlays,
};

let mut driver = GizmoDriver::new();
let style = GizmoStyle::default();

// Each frame:
let input = FrameInput::from_action(&action_frame, &camera, viewport_size, Some(centre))
    .with_selection_orientation(rotation); // only needed for local space
match driver.update(input) {
    ManipResult::Update(delta) => current = apply(&delta, start),
    ManipResult::Commit => start = current,
    ManipResult::Cancel | ManipResult::ConstraintChanged => current = start,
    _ => {}
}

if let Some(centre) = driver.gizmo_centre() {
    build_gizmo_overlays(
        &style,
        driver.mode(),
        camera.proj_matrix() * camera.view_matrix(),
        camera.forward(),
        [viewport_size.x, viewport_size.y],
        centre,
        driver.gizmo_scale(),
        driver.hovered_axis(),
        driver.space_orientation(),
        &mut overlays.shapes,
        &mut overlays.polylines,
    );
}
```

`TransformDelta` is cumulative: each `Update` holds the whole change since the session began, so apply it to the pose you saved at the start. Nothing drifts, and cancelling just restores that pose.

While a handle is hovered or a session is running (`driver.hovered_axis()` is not `GizmoAxis::None`, or `driver.is_active()`), keep the left drag away from your camera controller so the gizmo and the orbit do not both act on it.

### Key bindings

The controller reads the `Action::Custom` values in `viewport_lib_gizmo::actions`. Add their default bindings to the resolver the gizmo reads from:

```rust,ignore
let input = ViewportInput::from_preset(BindingPreset::Default)
    .with_bindings(viewport_lib_gizmo::manipulation_bindings());
```

| Keys | Action |
| --- | --- |
| G / R / S | Start a move / rotate / scale session |
| X / Y / Z | Constrain to an axis (Shift+X / Y / Z: the plane excluding it) |
| Digits, Backspace, Tab | Type a value, and move to the next axis |
| Tab (no session) | Cycle the gizmo mode |
| `[` / `]` | Cycle the pivot |

`actions::TOGGLE_SPACE` is left unbound for you to place. The crate's actions use `0x1000_0000..0x1000_0100`, so keep your own `Custom` values below `0x1000_0000`.

### Options

- **Local space.** `driver.set_space(GizmoSpace::Local)` aligns the handles and the X / Y / Z constraints with the selection's own axes, taken from `with_selection_orientation`. Pass `driver.space_orientation()` to `build_gizmo_overlays`.
- **Pivot.** `driver.pivot_mode()` says where the transform centre sits for a multi-selection: the centroid, each object's origin, the world origin or a 3D cursor. The driver does not resolve it: pass it to `gizmo_centre_for_pivot` with your selection to get the centre.
- **Value mode.** `ValueMode::Relative` treats typed values as offsets from the start pose; `ValueMode::Objective` treats them as absolute world values. The driver tracks and displays the mode, and your app applies the numeric overrides in `TransformDelta`.
- **Snapping.**

  ```rust,ignore
  let snap = SnapConfig::new()
      .with_translation(0.25)
      .with_rotation(std::f32::consts::PI / 12.0);
  let driver = GizmoDriver::new().with_snap(snap);
  ```

A running session keeps the space, pivot and mode it started with.

### Several views

To show one selection in several views (a CAD quad view, say), use one `GizmoDriver`, drive it from the view under the cursor and draw it in all of them:

```rust,ignore
let mut active = ActiveView::new();

// Each frame:
let view = active.current(hovered_view); // held for the length of a drag
let i = view.unwrap_or(0);
let mut input = FrameInput::from_action(&action_frame, &cameras[i], sizes[i], centre);
input.cursor_viewport = view.and(cursor_in_view_i); // in that view's own pixels
driver.update(input);
active.end_frame(view, button_held || driver.is_active());

for (j, frame) in frames.iter_mut().enumerate() {
    driver.build_view_overlays(
        &style,
        &cameras[j],
        sizes[j],
        view == Some(j),
        &mut frame.overlays.shapes,
        &mut frame.overlays.polylines,
    );
}
```

`ActiveView` keeps a drag or session on the view it started in, so the object does not jump when the cursor crosses into another view. `build_view_overlays` sizes the gizmo for each view's camera.

## Examples

```sh
cargo run --example gizmo_basic            # one cube, default style
cargo run --example gizmo_styling          # sidebar edits the GizmoStyle live
cargo run --example gizmo_multi_viewport   # one gizmo across four views
```

Tab or Q cycles the mode, backtick toggles relative / objective values, and G / R / S start a keyboard session.

## Building from source

The repo uses path dependencies, so it expects `viewport-lib` checked out next to it:

```text
viewport-lib/
viewport-lib-gizmo/
```

## License

GPL-3.0-only.
