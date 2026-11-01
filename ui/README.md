# viewport-lib-gizmo-ui

`viewport-lib-gizmo-ui` hosts the `viewport-lib-gizmo` transform gizmo as a
[viewport-lib-ui](https://github.com/grimandgreedy/viewport-lib-ui) `Custom`
widget. `GizmoTool` paints the handles in the same pass as the rest of the UI and
shares one pointer capture with the panels, so a click on a panel never reaches
the gizmo or the camera. Results come back as a typed `GizmoResult`. The crate
also has a readout panel: an x / y / z vector field with a relative / objective
toggle and a badge for the current mode.

Register one `GizmoTool` under `GIZMO_KIND` (`"vpl.gizmo"`) and it serves
every gizmo node on the screen. Each frame, put a `GizmoFrame` (the camera, the
selection centre and the resolved input) on the frame's host map:

```rust,ignore
let map = HostMap::new().with_context(
    GizmoFrame::new(camera)
        .with_selection_centre(Some(centre))
        .with_action_frame(action),
);
let frame = Frame::new(size, &measure, &registry).with_host(HostSources::default().with_map(&map));
let result = ui.update_frame(&mut screen, &input, &frame);
```

For several views of one selection, put a pane of that kind in each view, give
each its own widget id, and build each view's frames with that view's camera in
its `GizmoFrame`. The panes share one driver: the pane the
pointer is on drives it, a drag or a G / R / S session stays with the pane it
started in, and the others paint the gizmo for their own camera. What a pane
keeps between frames lives in the toolkit's per-node state, so the tool holds
nothing per pane.

The crate depends only on `viewport-lib-gizmo`, `viewport-lib-ui-extend` and
`viewport-lib-ui-document`, which are renderer-free, so it does not depend on
viewport-lib or wgpu.

```toml
viewport-lib-gizmo = { path = "..." }
viewport-lib-gizmo-ui = { path = ".../ui" }
```

## Examples

```sh
cargo run -p viewport-lib-gizmo-ui --example gizmo_ui                  # the gizmo as a viewport-lib-ui widget
cargo run -p viewport-lib-gizmo-ui --example gizmo_ui_multi_viewport   # four views through the widget
```

Tab or Q cycles the mode and backtick toggles relative / objective values. G, R
and S start a keyboard session.

## License

GPL-3.0-only.
