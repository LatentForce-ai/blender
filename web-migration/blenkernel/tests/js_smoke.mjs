// Smoke test of the built WASM package from JavaScript (no browser needed).
// Run after `wasm-pack build --target web`:  node tests/js_smoke.mjs

import { readFileSync } from "node:fs";
import assert from "node:assert/strict";
import init, { BlenderMain } from "../pkg/blenkernel.js";

await init({ module_or_path: readFileSync(new URL("../pkg/blenkernel_bg.wasm", import.meta.url)) });

const NONE = 0xffffffff;
const near = (a, b, msg) => assert.ok(Math.abs(a - b) < 1e-5, `${msg}: ${a} != ${b}`);

const bmain = new BlenderMain();
const scene = bmain.scene_add("Scene");
const cubeMesh = bmain.mesh_add_cube(2.0);
const sphereMesh = bmain.mesh_add_uv_sphere(1.0, 32, 16);
const cube = bmain.object_add_mesh("Cube", cubeMesh);
const moon = bmain.object_add_mesh("Moon", sphereMesh);
const empty = bmain.object_add_empty("Empty");
bmain.scene_link_object(scene, cube);
bmain.scene_link_object(scene, moon);

// Typed arrays come back with the right types and sizes.
assert.deepEqual([...bmain.mesh_counts(cubeMesh)], [8, 12, 6, 24]);
assert.deepEqual([...bmain.mesh_counts(sphereMesh)], [482, 992, 512, 1984]);
assert.ok(bmain.mesh_positions(cubeMesh) instanceof Float32Array);
assert.equal(bmain.mesh_positions(cubeMesh).length, 24);
assert.ok(bmain.mesh_triangles(cubeMesh) instanceof Uint32Array);
assert.equal(bmain.mesh_triangles(cubeMesh).length, 36);
assert.equal(bmain.mesh_edges(cubeMesh).length, 24);
assert.equal(bmain.mesh_vert_normals(sphereMesh).length, 482 * 3);
assert.deepEqual([...bmain.mesh_bounds(cubeMesh)], [-1, -1, -1, 1, 1, 1]);

// Scene contents and handles.
assert.deepEqual([...bmain.scene_objects(scene)], [cube, moon]);
assert.equal(bmain.object_mesh(cube), cubeMesh);
assert.equal(bmain.object_mesh(empty), NONE);
assert.equal(bmain.object_name(cube), "Cube");

// New data-blocks are reported, then cleared.
assert.ok(bmain.take_changes().length > 0);
assert.equal(bmain.take_changes().length, 0);

// Transforms: column-major, translation in 12..14.
bmain.object_set_location(cube, 1, 2, 3);
let m = bmain.object_world_matrix(cube);
assert.equal(m.length, 16);
assert.deepEqual([...m.slice(12)], [1, 2, 3, 1]);
assert.deepEqual([...bmain.take_changes()], [1, cube, 1]); // object, handle, TRANSFORM

// Parenting: moon at (3,0,0) under a cube rotated 90° about Z at (1,2,3) -> (1,5,3).
bmain.object_set_rotation_euler(cube, 0, 0, Math.PI / 2);
bmain.object_set_parent(moon, cube);
bmain.object_set_location(moon, 3, 0, 0);
m = bmain.object_world_matrix(moon);
near(m[12], 1, "x"); near(m[13], 5, "y"); near(m[14], 3, "z");
assert.equal(bmain.object_parent(moon), cube);
assert.throws(() => bmain.object_set_parent(cube, moon), /cycle/);

// Geometry edits are reported as GEOMETRY on the mesh.
bmain.take_changes();
const p = bmain.mesh_positions(cubeMesh).map((v) => v * 2);
bmain.mesh_set_positions(cubeMesh, p);
assert.deepEqual([...bmain.take_changes()], [2, cubeMesh, 2]);
assert.deepEqual([...bmain.mesh_bounds(cubeMesh)], [-2, -2, -2, 2, 2, 2]);
assert.throws(() => bmain.mesh_set_positions(cubeMesh, new Float32Array(3)), /expected length 8/);

// Custom mesh from raw data (one triangle).
const tri = bmain.mesh_add_from_data("Tri", new Float32Array([0, 0, 0, 1, 0, 0, 0, 1, 0]),
  new Int32Array([0, 3]), new Int32Array([0, 1, 2]));
assert.deepEqual([...bmain.mesh_counts(tri)], [3, 3, 1, 3]);
assert.deepEqual([...bmain.mesh_face_normals(tri)], [0, 0, 1]);

// Errors become JS exceptions.
assert.throws(() => bmain.object_name(999), /invalid Object handle 999/);
assert.throws(() => bmain.mesh_remove(cubeMesh), /still has 1 user/);
bmain.object_remove(cube);
assert.deepEqual([...bmain.scene_objects(scene)], [moon]);
assert.equal(bmain.object_parent(moon), NONE);
bmain.mesh_remove(cubeMesh);

console.log("js_smoke: all checks passed");
