# blenkernel (Rust → WASM)

A thin Rust port of Blender's `source/blender/blenkernel` data model (Main, ID, Mesh, Object,
Collection, Scene, Material with shader node trees) compiled to WebAssembly for the browser. Hackathon ticket HACK-1; the scope
and the mapping to the C++ sources are in [PLAN.md](PLAN.md).

## Setup (once)

```sh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh    # or: brew install rustup && rustup-init
source "$HOME/.cargo/env"
rustup target add wasm32-unknown-unknown
cargo install wasm-pack           # builds from crates.io
```

## Build, test, run

From this folder (`web-migration/blenkernel/`):

```sh
cargo test                       # Rust tests (tests/*.rs)
wasm-pack build --target web     # writes pkg/blenkernel.js + pkg/blenkernel_bg.wasm
node tests/js_smoke.mjs          # JS API checks against the built package
python3 -m http.server 8080      # then open http://localhost:8080/www/
```

The demo builds a cube, a UV sphere parented to it and a grid floor, spins the cube, can wave the
grid's vertices, and shows what `take_changes()` reports every frame.

## Using it from JavaScript

```js
import init, { BlenderMain } from "./pkg/blenkernel.js";
await init();

const bmain = new BlenderMain();
const scene = bmain.scene_add("Scene");
const cube = bmain.object_add_mesh("Cube", bmain.mesh_add_cube(2.0));
bmain.scene_link_object(scene, cube);
bmain.object_set_rotation_euler(cube, 0, 0, Math.PI / 4);

for (const ob of bmain.scene_objects(scene)) {
  const matrix = bmain.object_world_matrix(ob);   // Float32Array(16), column-major
  const mesh = bmain.object_mesh(ob);             // 0xFFFFFFFF for empties
  const positions = bmain.mesh_positions(mesh);   // Float32Array(verts * 3)
  const normals = bmain.mesh_vert_normals(mesh);  // Float32Array(verts * 3)
  const indices = bmain.mesh_triangles(mesh);     // Uint32Array(tris * 3)
}

const changes = bmain.take_changes();             // [type, handle, flags, ...], then cleared
```

Conventions:
- Every data-block is a `u32` handle; `0xFFFFFFFF` means none. Bad handles throw an `Error`.
- Buffers are copies, safe to keep after WASM memory grows.
- Matrices: column-major, translation at elements 12–14, ready for WebGPU uniform buffers.
- Triangles are counter-clockwise seen from outside (Blender's winding).
- `take_changes()` types: 0 scene, 1 object, 2 mesh, 3 collection. Flags: `1` transform,
  `2` geometry, `0x80000000` relations (links, parenting).

### Materials and shader node trees (for shading and render)

```js
const mat = bmain.material_add("Red", true);          // true = default Principled BSDF → Material Output
bmain.material_set_viewport(mat, 1, 0, 0, 1, 0, 0.4); // r, g, b, a, metallic, roughness (Solid mode)
bmain.mesh_set_materials(mesh, new Uint32Array([mat]));            // slots; 0xFFFFFFFF = empty slot
bmain.mesh_set_material_indices(mesh, new Int32Array(faceCount));  // slot per face
bmain.mesh_tri_material_indices(mesh);                // slot per triangle of mesh_triangles()
bmain.object_material(ob, 0);                         // material of an object's slot 0

const tree = JSON.parse(bmain.material_node_tree_json(mat)); // {output_node, nodes, links}
const rgb = bmain.material_node_add(mat, "ShaderNodeRGB");
bmain.material_node_set_output(mat, rgb, "Color", new Float32Array([1, 0, 0, 1]));
bmain.material_link_add(mat, rgb, "Color", tree.nodes[0].id, "Base Color");
bmain.material_node_order(mat);                       // evaluation order (inputs first)
```

Supported node types (`src/node_types.rs`, socket identifiers as in Blender):
`ShaderNodeOutputMaterial`, `ShaderNodeBsdfPrincipled` (main inputs), `ShaderNodeBsdfDiffuse`,
`ShaderNodeEmission`, `ShaderNodeMixShader`, `ShaderNodeMix` (color mode), `ShaderNodeMath`,
`ShaderNodeRGB`, `ShaderNodeValue`, `ShaderNodeTexImage` (`image` is a name/path property).
blenkernel stores the graph; evaluating it is the shading module's job. Material edits report the
`SHADING` flag (`0x80`) on type `4` in `take_changes()`.

From Rust: `bmain.material_add`, `bmain.material(id)?.node_tree`, `NodeTree::nodes()`,
`input_link()`, `output_node()`, `topological_order()`, `Mesh::tri_material_indices()`.

Full method list: `src/wasm.rs` (each method has a doc comment), or `pkg/blenkernel.d.ts` after a
build.

## Layout

| File | Ported from (`source/blender/…`) |
|---|---|
| `src/id.rs` | `makesdna/DNA_ID.h`, `blenkernel/intern/lib_id.cc` |
| `src/main_db.rs` | `blenkernel/BKE_main.hh`, `intern/main.cc`, `intern/main_namemap.cc` |
| `src/mesh.rs`, `src/attribute.rs` | `makesdna/DNA_mesh_types.h`, `intern/mesh.cc`, `intern/attribute_storage.cc` |
| `src/mesh_calc_edges.rs` | `intern/mesh_calc_edges.cc` |
| `src/mesh_normals.rs` | `intern/mesh_normals.cc` |
| `src/mesh_tessellate.rs` | `intern/mesh_tessellate.cc` |
| `src/primitives.rs` | `geometry/intern/mesh_primitive_cuboid.cc`, `_grid.cc`, `_uv_sphere.cc` |
| `src/object.rs` | `makesdna/DNA_object_types.h`, `intern/object.cc` |
| `src/material.rs` | `makesdna/DNA_material_types.h`, `intern/material.cc` (`material_default_surface_init`) |
| `src/node.rs`, `src/node_types.rs` | `makesdna/DNA_node_types.h`, `intern/node.cc`, `nodes/shader/nodes/node_shader_*.cc` |
| `src/collection.rs`, `src/scene.rs` | `DNA_collection_types.h`, `DNA_scene_types.h`, `intern/collection.cc`, `intern/scene.cc` |
| `src/wasm.rs` | new: the JavaScript API |

## Known differences from Blender

- Edge order and the cube's vertex order differ (counts, bounds and winding match).
- n-gons are fan-triangulated; Blender uses polyfill, which differs for concave n-gons.
- No parent-inverse matrix: a child keeps its local transform when parented.
- Axis-angle rotation mode is not supported.
- Materials are assigned on mesh slots only (no object-level slot override), no Image data-block
  (Image Texture stores a name/path), the Mix node supports color mode only, and Principled BSDF
  declares its main inputs only.
