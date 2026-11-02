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
   `gizmo_scale_for_camera` (and the perspective-only `compute_gizmo_scale`),
   `gizmo_centre_for_pivot`, `cursor_ray`, the `Gizmo` hit-tester, and the
   `snap_*` helpers. Enough to build a gizmo of your own.
2. **Style and overlays.** `build_gizmo_overlays` turns gizmo state into overlay
   shapes and polylines in the look set by a `GizmoStyle`. `GizmoStyle::default()`,
   `thin()` and `bold()` are the built-in looks; every colour and dimension is a
   public field.
3. **Controller.** `ManipulationController` runs the drag and keyboard sessions
   (G / R / S, X / Y / Z constraints, typed numeric values, confirm and cancel)
   and reports a `TransformDelta`. `GizmoDriver` wraps it with the per-frame work:
   sizing the gizmo for the screen, hit-testing the cursor, and handing the
   controller the gizmo's state. Most apps start here. Both read the same
   `FrameInput`.

## Usage

Each frame, pass the driver the resolved input, the camera, and the world-space
centre of the selection, then act on the result. `FrameInput::from_action` reads
the cursor and the primary button from the resolved input's pointer frame:

```rust,ignore
use viewport_lib_gizmo::{
    FrameInput, GizmoDriver, GizmoStyle, ManipResult, build_gizmo_overlays,
};

let mut driver = GizmoDriver::new();
let style = GizmoStyle::default();

// Each frame:
let input = FrameInput::from_action(&action_frame, &camera, viewport_size, Some(centre))
    .with_selection_orientation(rotation); // used in local space; leave it out for world only
let result = driver.update(input);

match result {
    // Total change since the session began: apply it to the start pose.
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

### Keys

The controller reads the actions in `viewport_lib_gizmo::actions`. They are `Action::Custom` values, so the resolver reports them only once it has bindings for them: add `manipulation_bindings()` to the resolver the gizmo reads from.

```rust,ignore
let input = ViewportInput::from_preset(BindingPreset::Default)
    .with_bindings(viewport_lib_gizmo::manipulation_bindings());
```

That binds G, R and S to begin a session, X / Y / Z to constrain (with Shift to exclude), Backspace and Tab for typed values, Tab to cycle the mode outside a session, and `[` / `]` to cycle the pivot. `actions::TOGGLE_SPACE` is left unbound for the host to place. The crate keeps its values in `0x1000_0000..0x1000_0100`; give your own `Custom` actions values below `0x1000_0000`.

`TransformDelta` is cumulative. Each `Update` holds the whole change since the
drag began, so you recompute the pose from the drag-start snapshot every frame.
Nothing drifts, and cancelling just restores the snapshot.

While a handle is hovered or a session is running (`driver.hovered_axis()` is
not `GizmoAxis::None`, or `driver.is_active()`), hold back the camera's left-drag
so orbit and the gizmo do not both act on the same drag.

### Local space

`driver.set_space(GizmoSpace::Local)` lines the handles up with the selection's
own axes, from the `selection_orientation` passed in each frame. The handles are
drawn, picked and dragged along those axes, and the X / Y / Z constraint keys
use them too. A session keeps the axes it started with.
`driver.space_orientation()` is the orientation to hand to
`build_gizmo_overlays`: identity in world space.

### Pivot

`driver.pivot_mode()` is where the transform centre sits for a multi-selection
(the centroid, each object's own origin, the world origin, a 3D cursor), cycled
with `cycle_pivot_forward()` and `cycle_pivot_backward()` (call them when the
`actions::CYCLE_PIVOT_FORWARD` and `actions::CYCLE_PIVOT_BACKWARD` actions fire;
`manipulation_bindings()` binds them to `[` and `]`). The driver
does not resolve it: pass it to `gizmo_centre_for_pivot` with your selection to
get the `selection_centre`. A session keeps the pivot it started about.

G, R and S switch the handles to the session's kind while it runs; the mode set
before comes back when it ends.

### Value mode

`ValueMode` is `Relative` (typed values are offsets from the start pose) or
`Objective` (typed values are absolute world values). The driver tracks the mode
and shows it in the readout; your app decides how to apply the numeric overrides
in `TransformDelta`.

### Snapping

```rust,ignore
let snap = SnapConfig::new()
    .with_translation(0.25)
    .with_rotation(std::f32::consts::PI / 12.0);
let driver = GizmoDriver::new().with_snap(snap);
```

### Several views

For a split layout (a CAD quad view, say) that shows one selection from several
cameras, use one `GizmoDriver` for the selection, not one per view: the driver
holds the session, and separate drivers would each start their own.

Each frame, drive it from one view and draw it in all of them:

```rust,ignore
let mut active = ActiveView::new();

// Each frame:
let view = active.current(hovered_view);         // the view under the cursor, held during a drag
let i = view.unwrap_or(0);
let mut input = FrameInput::from_action(&action_frame, &cameras[i], sizes[i], centre);
input.cursor_viewport = view.and(cursor_in_view_i);   // in that view's own pixels
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

`ActiveView` keeps a drag or a G / R / S session on the view it started in: the
drag is measured in that view's pixels and through its camera, so switching
views part way would make the object jump. `build_view_overlays` sizes the gizmo
for each view's own camera and height. Only the driving view shows the hovered
handle; while a session runs, every view shows the axis being changed.

`cursor_ray` handles orthographic cameras, so the handles can be grabbed in
orthographic top, front and side views as well as in perspective, and
`gizmo_scale_for_camera` keeps the gizmo the same size on screen in both.

## Examples

```sh
cargo run --example gizmo_basic                 # ViewportApp, one cube, default style
cargo run --example gizmo_styling               # eframe, sidebar restyles the gizmo live
cargo run --example gizmo_multi_viewport        # four views, one gizmo, three of them orthographic
```

In all of them, Tab or Q cycles the mode and backtick toggles relative /
objective values. G, R and S start a keyboard session.

## Building

The crate uses path dependencies, so it expects `viewport-lib` checked out next
to it:

```text
viewport-lib/
viewport-lib-gizmo/
```

## License

GPL-3.0-only.
