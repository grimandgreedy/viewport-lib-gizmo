# viewport-lib-gizmo

A move / rotate / scale transform gizmo for [viewport-lib](https://github.com/grimandgreedy/viewport-lib) scenes. It hit-tests the handles, turns drags and G / R / S keyboard sessions into transforms, and builds overlay items which are submitted to the vpl frame.

<p align="center">
  <img src="https://github.com/grimandgreedy/viewport-lib-gizmo/blob/master/assets/gizmo2.png?raw=true" alt="viewport-lib-gizmo" width="50%" />
</p>

## Examples

See the examples for demo and usage practices. The styling example shows both visually and in code how to take advantage of the various customisable elements of vpl-gizmo.


```sh
cargo run --release --example gizmo_basic
cargo run --release --example gizmo_styling
cargo run --release --example gizmo_multi_viewport
```
