# HACK-1: blenkernel → Rust → WASM

**Owner:** Bimal · **Deadline:** today, 2026-10-29 · **Ticket:** "Port blenkernel to Rust and expose it via WASM"

Everything for this ticket lives in this folder (`web-migration/blenkernel/`).

> **Status (2026-10-29):** Phases 0–5 done. `cargo test`: 25 passing · `node tests/js_smoke.mjs`: passing ·
> `wasm-pack build --target web`: 104 KB `.wasm` · demo verified in Chrome (spinning cube + parented
> sphere + waving grid, `take_changes()` reporting TRANSFORM / GEOMETRY). Open: Phase 6 integration
> with depsgraph/render, §8 stretch goal (`.blend` loading).

---

## 1. What we are building

Blender's data model rewritten in **Rust**, compiled to **WebAssembly**, and callable from
JavaScript in the browser. It is the bottom layer of the hackathon project:

```
  ┌────────────────────────────────────────┐
  │ gpu/render  (Rishik)  WebGPU drawing   │  reads meshes + object matrices
  ├────────────────────────────────────────┤
  │ depsgraph   (Sandeep) update engine    │  reads "what changed" flags, recomputes
  ├────────────────────────────────────────┤
  │ blenkernel  (Bimal)   THIS FOLDER      │  stores Scene / Collection / Object / Mesh
  └────────────────────────────────────────┘
```

### Scope: a thin, working slice, not all of blenkernel

The real `source/blender/blenkernel` is ~383k lines of C++ in ~590 files and links against
almost all of Blender (depsgraph, gpu, draw, nodes, python, physics libs). It cannot be
ported in a day. We port only the core data model needed for a 3D scene in the browser.

| In scope (today) | Out of scope |
|---|---|
| `Main` (the database of all data-blocks) | Particles, physics, fluid, dynamic paint |
| `ID` (name, type, user count, recalc flags) | Constraints, drivers, animation |
| `Mesh` (positions, edges, faces, corners, attributes) | Modifiers, geometry nodes, node trees |
| Mesh normals, triangulation, bounds | Materials, images, textures |
| Primitives: cube, grid, UV sphere | Grease pencil, curves, volumes |
| `Object` (loc / rot / scale → 4×4 matrix, parent) | Library linking, overrides, undo |
| `Collection`, `Scene` | Python API, UI, editors |
| WASM API for JavaScript | Full `.blend` reading (stretch goal, §8) |

**Done today means:** in a browser page, JS creates a scene, adds a cube and a sphere, moves them,
and gets back GPU-ready buffers (positions, normals, triangle indices, world matrices) plus a
list of changed data-blocks. Rishik can draw it and Sandeep can react to the changes.

---

## 2. Where it comes from in the C++ (read these first)

| Rust module | Ported from | What to take |
|---|---|---|
| `id.rs` | `makesdna/DNA_ID.h` (`struct ID`, line ~415), `DNA_ID_enums.h`, `blenkernel/intern/lib_id.cc` | `name`, `us` (user count), `recalc` flags, `session_uid`; `id_us_plus/min`, `BKE_id_new` |
| `main_db.rs` | `blenkernel/BKE_main.hh` (`struct Main`: `scenes`, `objects`, `meshes`, `collections` lists), `intern/main.cc`, `intern/main_namemap.cc` | One list per ID type; unique names ("Cube", "Cube.001", …) |
| `mesh.rs` | `makesdna/DNA_mesh_types.h` (`struct Mesh`), `intern/mesh.cc` | Counts + struct-of-arrays layout (see §3.2) |
| `attribute.rs` | `intern/attribute_storage.cc`, `mesh.cc:641` | Named arrays per domain (Point/Edge/Face/Corner) |
| `mesh_calc_edges.rs` | `intern/mesh_calc_edges.cc` | Build unique edges from faces |
| `mesh_normals.rs` | `intern/mesh_normals.cc` (`face_normal_calc` :143, `normals_calc_verts` :194) | Newell face normals; vertex normals = angle-weighted face normals |
| `mesh_tessellate.rs` | `intern/mesh_tessellate.cc` | Faces → triangles (fan triangulation is enough for quads/convex n-gons) |
| `primitives.rs` | `geometry/intern/mesh_primitive_cuboid.cc` (:372), `_grid.cc` (:224), `_uv_sphere.cc` (:302) | Grid + UV sphere: same vertex/face order as Blender; cube: own vertex order, same counts/winding |
| `object.rs` | `makesdna/DNA_object_types.h` (`struct Object`), `intern/object.cc` (`BKE_object_to_mat3` :3109, `BKE_object_to_mat4` :3122) | `loc`, `rot` (Euler), `quat`, `scale`, `rotmode`, `parent`, `data` |
| `collection.rs` | `makesdna/DNA_collection_types.h`, `intern/collection.cc` (`BKE_collection_object_add`) | Objects list + child collections |
| `scene.rs` | `makesdna/DNA_scene_types.h` (`struct Scene`), `intern/scene.cc` (`BKE_scene_add`) | Master collection, active camera (optional) |
| `wasm.rs` | (new) | `wasm-bindgen` API for JS |

---

## 3. Design

### 3.1 Pointers become handles

C++ Blender links everything with raw pointers (`Object::data` is an `ID*`, `ListBase` are linked
lists). In Rust, and across the WASM boundary, we use **index handles** into arenas owned by `Main`:

```rust
pub struct MeshId(u32);   pub struct ObjectId(u32);
pub struct CollectionId(u32);   pub struct SceneId(u32);

pub struct Main {
    meshes: Arena<Mesh>, objects: Arena<Object>,
    collections: Arena<Collection>, scenes: Arena<Scene>,
    names: NameMap, next_session_uid: u32,
}
```

A generational arena (`slotmap` crate, or a small hand-written one) catches stale handles after
deletion. JS only ever sees plain `u32` handles.

### 3.2 Mesh layout: same as Blender's

Blender ≥ 4.0 stores meshes as flat arrays, which is also what a GPU wants:

| Field | Type | Blender attribute name | Meaning |
|---|---|---|---|
| `positions` | `Vec<[f32;3]>` | `"position"` (Point) | vertex coordinates |
| `edge_verts` | `Vec<[i32;2]>` | `".edge_verts"` (Edge) | 2 vertex indices per edge |
| `face_offsets` | `Vec<i32>` (len = faces+1) | `face_offset_indices` | face *i* = corners `[off[i], off[i+1])` |
| `corner_verts` | `Vec<i32>` | `".corner_vert"` (Corner) | vertex used by each face corner |
| `corner_edges` | `Vec<i32>` | `".corner_edge"` (Corner) | edge after each corner |
| `attributes` | `AttributeStorage` | any other name | e.g. `"UVMap"` (Corner, float2) |

Derived data is computed lazily and cached, then cleared when the mesh changes (C++ does the same
with `runtime` caches): `vert_normals`, `face_normals`, `corner_tris`, `bounds`.

### 3.3 Object transform: match `BKE_object_to_mat4`

```
mat3  = rot_mat(rot ⊕ drot, rotmode) × size_mat(scale × dscale)
mat4  = mat3 with translation (loc + dloc)
world = parent.world × local            (if parent set; parentinv ignored today)
```

- Matrices are **column-major `[f32;16]`**, translation in elements 12..14. This is the same as
  Blender's `float[4][4]` (`r_mat[3]` is translation) and is what WebGPU/WGSL expect, so Rishik
  uploads them directly.
- `rotmode`: support Euler XYZ (default `ROT_MODE_EUL`) and quaternion. Other Euler orders are
  cheap to add with `glam::EulerRot`.

### 3.4 Change tracking (the contract with depsgraph)

Every ID carries `recalc: u32` like C++ `ID::recalc`. Any mutating API call sets a flag:

| Flag | Set when |
|---|---|
| `RECALC_TRANSFORM` | object loc/rot/scale/parent changes |
| `RECALC_GEOMETRY` | mesh positions/topology change |
| `RECALC_SELECT`, `RECALC_COPY_ON_EVAL` | reserved, not used today |

`Main::take_changes() -> Vec<(IdType, u32 handle, u32 flags)>` returns and clears them.
Sandeep's depsgraph calls this each frame, then re-evaluates world matrices and mesh caches.

### 3.5 Unique naming

As in C++ (`BKE_main_namemap_get_unique_name`): names are unique per ID type; collisions get
`.001`, `.002`, … Max 255 bytes (`MAX_ID_NAME` 258 minus prefix and terminator). Internally store the bare name; the C++ 2-char type prefix
(`"OBCube"`) is only needed if we read `.blend` files (§8).

### 3.6 Dependencies

- `glam`: vectors/matrices/quaternions, column-major, works on wasm32
- `wasm-bindgen`: JS bindings · `js-sys`: `Float32Array` / `Uint32Array`
- `slotmap` (optional): generational handles
- No other crates. Keep the `.wasm` small.

---

## 4. Folder layout (target)

```
web-migration/blenkernel/
├── PLAN.md                 ← this file
├── README.md               build + usage instructions (written in Phase 5)
├── Cargo.toml              crate "blenkernel", crate-type = ["cdylib", "rlib"]
├── src/
│   ├── lib.rs              module wiring, public Rust API
│   ├── error.rs            BkeError (every failure is a value → JS exception)
│   ├── id.rs               IdType, IdHeader (name, users, recalc, session_uid)
│   ├── main_db.rs          Main + arenas + NameMap + take_changes()
│   ├── attribute.rs        AttrDomain, AttrType, AttributeStorage
│   ├── mesh.rs             Mesh struct + accessors + cache invalidation
│   ├── mesh_calc_edges.rs
│   ├── mesh_normals.rs
│   ├── mesh_tessellate.rs
│   ├── primitives.rs       cube / grid / uv_sphere
│   ├── object.rs           Object, transforms, parenting
│   ├── collection.rs
│   ├── scene.rs
│   └── wasm.rs             #[wasm_bindgen] API (only compiled for wasm32)
├── tests/
│   ├── mesh_tests.rs       counts, normals, edges vs Blender
│   ├── object_tests.rs     matrices vs Blender
│   └── main_tests.rs       names, user counts, collections, take_changes
├── pkg/                    wasm-pack output (git-ignored)
└── www/
    ├── index.html          demo page
    └── demo.js             creates scene, prints buffers, draws wireframe on <canvas>
```

---

## 5. JavaScript API (share this with Sandeep and Rishik first)

```js
import init, { BlenderMain } from "./pkg/blenkernel.js";
await init();

const bmain = new BlenderMain();
const scene = bmain.scene_add("Scene");                    // -> u32
const cubeMesh = bmain.mesh_add_cube(2.0);                 // size 2 = Blender's default cube
const cube = bmain.object_add_mesh("Cube", cubeMesh);      // -> u32
bmain.scene_link_object(scene, cube);

bmain.object_set_location(cube, 1, 0, 0);
bmain.object_set_rotation_euler(cube, 0, 0, Math.PI / 4);
bmain.object_set_scale(cube, 1, 1, 2);
bmain.object_set_parent(child, cube);                      // optional

// for Rishik (render)
bmain.scene_objects(scene);              // Uint32Array of object handles
bmain.object_world_matrix(cube);         // Float32Array(16), column-major
bmain.object_mesh(cube);                 // mesh handle, or 0xFFFFFFFF if not a mesh
bmain.mesh_positions(cubeMesh);          // Float32Array(verts*3)
bmain.mesh_vert_normals(cubeMesh);       // Float32Array(verts*3)
bmain.mesh_triangles(cubeMesh);          // Uint32Array(tris*3), indices into positions
bmain.mesh_edges(cubeMesh);              // Uint32Array(edges*2), for wireframe
bmain.mesh_bounds(cubeMesh);             // Float32Array(6): min xyz, max xyz

// for Sandeep (depsgraph)
bmain.take_changes();                    // Uint32Array [type, handle, flags, type, handle, flags, …]
bmain.object_local_matrix(cube);         // local matrix; depsgraph composes the parent chain
bmain.object_parent(cube);               // parent handle or 0xFFFFFFFF
bmain.mesh_set_positions(cubeMesh, f32); // edit geometry, sets RECALC_GEOMETRY
```

**Rule:** buffer getters return **copies** (`Float32Array::from(&slice[..])`). Zero-copy views
into WASM memory become invalid when memory grows, which is a common hard-to-trace bug. Switch to
views later only if profiling shows it matters.

---

## 6. Step-by-step plan for today

Rough timings. Phases 1 and 2 unblock the other two tickets, so do them first.

### Phase 0: Toolchain (≈20 min)
Nothing Rust-related is installed on this machine yet (node v26 is).
```sh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh   # installs rustup + cargo
source "$HOME/.cargo/env"
rustup target add wasm32-unknown-unknown
cargo install wasm-pack
```
- [x] `cargo --version`, `wasm-pack --version` work

### Phase 1: Skeleton + API stubs (≈40 min) → **send to Sandeep & Rishik**
- [x] `cargo init --lib` in this folder, add deps, `crate-type = ["cdylib","rlib"]`
- [x] Create every file in §4 with types and **stub** functions matching §5 (`todo!()` / dummy data)
- [x] `mesh_add_cube` returns a hard-coded 8-vertex cube so render can start immediately
- [x] `wasm-pack build --target web` succeeds; `www/index.html` logs the cube's positions
- [ ] Post the §5 API in the Hackathon topic; commit

### Phase 2: ID + Main (≈45 min)
- [x] `IdType` enum (`Scene`, `Object`, `Mesh`, `Collection`), `IdHeader { name, users, recalc, session_uid }`
- [x] Arenas, `*_add`, `*_remove` (removing an object unlinks it from collections; removing a mesh in use is refused while `users > 0`)
- [x] `NameMap` unique naming (`Cube`, `Cube.001`)
- [x] User counting: object → mesh = +1 user; unlink = −1
- [x] `take_changes()`
- [x] Tests: unique names, user counts, stale handle rejected

### Phase 3: Mesh (≈1 h 30)
- [x] `Mesh` with the §3.2 arrays + `AttributeStorage`
- [x] `mesh_calc_edges`: unique undirected edges from face corners; fill `corner_edges`
- [x] Face normals (Newell's method, as in `face_normal_calc`); vertex normals (angle-weighted)
- [x] Triangulation (fan) → `corner_tris`
- [x] Bounds
- [x] Lazy caches, cleared on any write
- [x] Primitives:
  - cube: `create_cuboid_mesh(size, 2,2,2)` → **8 verts, 12 edges, 6 faces, 24 corners**, bounds ±size/2
  - grid: `create_grid_mesh(x, y, size_x, size_y)` → x·y verts, (x−1)(y−1) quads
  - UV sphere: `create_uv_sphere_mesh(radius, segments=32, rings=16)` → 482 verts, 512 faces (Blender default)
- [x] Tests: the counts above; cube face normals are ±X/±Y/±Z unit vectors; every triangle index < verts

### Phase 4: Object, Collection, Scene (≈1 h)
- [x] `Object { kind: Empty | Mesh(MeshId), loc, rot, quat, scale, dloc, drot, dscale, rotmode, parent }`
- [x] `local_matrix()` per §3.3; `world_matrix()` walks the parent chain (reject parent cycles)
- [x] `Collection { objects, children }`; `Scene { master_collection }`; `scene_objects()` flattens recursively without duplicates
- [x] Setters mark `RECALC_TRANSFORM`
- [x] Tests: identity default; loc (1,2,3) → m[12..15]; 90° Z rotation maps +X → +Y; parent × child

### Phase 5: WASM API + demo + hand-off (≈1 h)
- [x] Fill every `wasm.rs` function from §5 with the real implementation
- [x] `www/demo.js`: build scene (cube + sphere + grid), animate the cube's rotation with
      `requestAnimationFrame`, draw a wireframe on a 2D `<canvas>` from `mesh_edges` + `object_world_matrix`
      (simple perspective). This proves the data model end-to-end even if WebGPU isn't ready.
- [x] Serve: `npx http-server .` (or `python3 -m http.server`) and open `www/index.html`
- [x] `README.md`: build, run, API
- [x] `cargo test` green; progress logged on the ticket
- [ ] Commit; tell Sandeep & Rishik the real API is live

### Phase 6: Integration (rest of the day)
- [ ] Help Rishik upload `mesh_positions` / `mesh_triangles` / `object_world_matrix` into WebGPU buffers
- [ ] Agree with Sandeep on who computes `world_matrix` (blenkernel helper vs depsgraph); keep both available
- [ ] Fix whatever the integration turns up

---

## 7. How we check it's right

| Check | Expected (from Blender C++) |
|---|---|
| Cube size 2 | 8 / 12 / 6 / 24 verts / edges / faces / corners, bounds (−1,−1,−1)..(1,1,1) |
| UV sphere 32×16 | 482 verts, 512 faces |
| Grid 10×10 | 100 verts, 81 faces |
| Default object | world matrix = identity |
| loc (1,2,3) | matrix elements 12,13,14 = 1,2,3 |
| rot Z 90° | matrix × (1,0,0) = (0,1,0) |
| Names | second "Cube" is "Cube.001" |

To cross-check against real Blender (optional): `blender -b --python-expr "import bpy; m=bpy.data.objects['Cube'].data; print(len(m.vertices), len(m.edges), len(m.polygons), len(m.loops))"`

---

## 8. Stretch goal: open a real `.blend` file

Only if Phases 0–5 are done. Read `release/datafiles/startup.blend` (the default cube scene):
1. Header: `BLENDER` + pointer size (`_`/`-`) + endianness (`v`/`V`) + version
2. Blocks: 4-byte code (`ME`, `OB`, `SC`, `DATA`, `DNA1`, `ENDB`), size, old pointer, SDNA index, count
3. Parse the `DNA1` block (struct/field names) to find field offsets for `Mesh`, `Object`, `Scene`
4. Resolve old pointers → handles; read the `position`, `.corner_vert`, `face_offset_indices` arrays
Reference: `source/blender/blenloader/intern/readfile.cc`, `makesdna/intern/dna_genfile.cc`.

---

## 9. Risks and how to handle them

| Risk | Mitigation |
|---|---|
| Not enough time | Phases are ordered so each one leaves something that works; Phase 1 stubs already unblock teammates |
| WASM memory views invalidated | Return copies (§5 rule) |
| Matrix layout mismatch with render | Column-major everywhere; one shared test vector (loc 1,2,3 → m[12..14]) |
| Teammates expect a different API | Post §5 in Phase 1 and agree on it before deep work |
| Rust toolchain install blocked (network/permissions) | Fall back to `brew install rustup` |
| Scope creep | Anything not in §1 "In scope" waits until after the deadline |

---

## 10. Hand-off checklist (end of day)
- [ ] `cargo test` passes
- [ ] `wasm-pack build --target web` produces `pkg/`
- [ ] Demo page shows animated objects
- [ ] README + API posted in the Hackathon topic
- [ ] Work committed on a branch; progress logged on the ticket

---

## HACK-9: Material and shader node-tree data-blocks (for shading, HACK-7)

**Done (2026-10-29):**
- `material.rs`: `Material` (viewport color, metallic, roughness, embedded node tree), defaults from
  `DNA_material_defaults.h`; default tree from `material_default_surface_init`.
- `node.rs`: `NodeTree` with nodes, typed sockets + default values, links (one per input, replaced
  on relink; shader→non-shader and cycles refused), active output, topological order, JSON export.
- `node_types.rs`: 10 shader node types with Blender's socket identifiers and defaults.
- Mesh material slots (user-counted), per-face `material_index`, per-triangle slots for render;
  `object_material()` clamps like `BKE_object_material_get`.
- `recalc::SHADING` (= `ID_RECALC_SHADING`) reported by `take_changes()`; IdType 4 = Material.
- JS API for all of the above; `tests/material_tests.rs` (12) + JS smoke checks; demo colors
  objects by material.

**Decisions:** node tree embedded in the material (as in Blender, not a separate data-block);
blenkernel only stores the graph, the shading module evaluates it.
