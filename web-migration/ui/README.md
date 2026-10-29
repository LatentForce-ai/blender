# ui: Blender Web interface

React + TypeScript shell (HACK-4) around the `app` crate's WebAssembly build. It has a Blender-like
layout:

- **Toolbar**: add Cube / UV Sphere / Plane / Grid / Empty, and Delete (or press X / Delete).
  Undo, Redo, Save and Open are shown disabled until HACK-8 lands.
- **3D Viewport**: the render crate's Solid-mode viewport (WebGPU, WebGL2 fallback). Drag to
  orbit, scroll to zoom.
- **Outliner**: the scene's objects nested by parent. Click to select, double-click to rename.
- **Properties**: Object (name, location, rotation in degrees, scale, parent), Material (slots,
  viewport color / metallic / roughness, new and remove slot), Mesh Data (counts).
- **Status bar**: object, vertex, face and triangle counts, backend, fps.

All scene data lives in Rust (`BlenderMain`). `src/engine.ts` wraps it: every UI edit goes
through an `Engine` method, which bumps a revision so React re-reads the scene. Each animation
frame calls `tick()`, which runs the depsgraph and draws.

## Run

```sh
npm install
npm run wasm     # builds ../app/pkg with wasm-pack (needs Rust + wasm-pack, see ../app/README.md)
npm run dev      # http://localhost:5173
npm run build    # type-checks and writes dist/
```

## Not yet

- Selecting by clicking in the viewport (needs picking in render) and a selection outline.
- Timeline (there's no animation system yet), Material Preview / Rendered modes (needs shading
  linked into `app`), editing quaternion rotations.
