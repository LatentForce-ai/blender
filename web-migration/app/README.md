# app: the Blender web build

The single WebAssembly module of the Blender web migration. It links every ported module so they
share one scene in one memory, and it is the only crate built with `wasm-pack`.

| Crate | Ticket | Status |
|---|---|---|
| `../blenkernel` | HACK-1, HACK-9 | Linked: scenes, objects, meshes, materials |
| `../render` | HACK-3 | Linked: Solid-mode viewport, WebGPU with WebGL2 fallback |
| `../depsgraph` | HACK-2 | Linked: evaluates transforms and geometry each frame |
| node shading | HACK-7 | Not linked yet (Material Preview / Rendered modes) |

## Build and run

Once: install Rust with `rustup`, then `rustup target add wasm32-unknown-unknown` and `wasm-pack`.

```sh
wasm-pack build --target web          # release: about 2.3 MB, 0.9 MB gzipped
wasm-pack build --target web --dev    # faster builds, about 18 MB
python3 -m http.server 8091           # then open http://localhost:8091/www/
```

`www/` shows Blender's startup scene. `www/?scene=demo` exercises every update path: a spinning
cube with two material slots, a mirrored metallic sphere parented to it, and a floor whose
vertices wave every frame. Drag to orbit, scroll to zoom.

## JavaScript API

```js
import init, { BlenderMain, Depsgraph, Viewport, tick } from "./pkg/app.js";

await init();
const viewport = await Viewport.create(canvas); // WebGPU, or WebGL2 where WebGPU is missing
const bmain = new BlenderMain();                // blenkernel's API: ../blenkernel/README.md
const scene = bmain.scene_add("Scene");
// ... add objects, meshes and materials through bmain ...
const depsgraph = new Depsgraph(scene);         // one per scene, kept across frames

const frame = () => {
  tick(bmain, depsgraph, viewport);
  requestAnimationFrame(frame);
};
requestAnimationFrame(frame);
```

`Viewport` also has `backend()` (`"WebGPU"` or `"WebGL2"`), `resize(width, height)` in canvas
pixels, `orbit(dx, dy)` in CSS pixels, and `zoom(steps)` in wheel notches (positive moves away).

## Adding a module

Depend on it in `Cargo.toml` and call it from `tick` in `src/lib.rs`. Each frame `tick`:

1. evaluates the depsgraph (`Depsgraph::evaluate`), which consumes the scene's pending changes
   and returns what it recomputed;
2. hands those updates to the renderer (`Viewport::apply_updates`), then draws the evaluated
   scene (`Viewport::draw`).

The depsgraph drains `Main::take_changes`, so a new module reacts to its `Updates` or checks its
own inputs rather than reading the change list. Modules work on blenkernel's `Main` directly.
