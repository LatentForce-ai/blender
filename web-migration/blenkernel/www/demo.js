// End-to-end check of the blenkernel WASM API: build a scene in Rust, animate it, and draw a
// wireframe on a 2D canvas from the buffers it returns. Rendering here is deliberately simple;
// the real WebGPU renderer is the gpu/render ticket.

import init, { BlenderMain } from "../pkg/blenkernel.js";

const TYPE_NAMES = ["Scene", "Object", "Mesh", "Collection"];
const FLAG_NAMES = [[1, "TRANSFORM"], [2, "GEOMETRY"], [0x80000000, "RELATIONS"]];
const NONE = 0xffffffff;

const errorBox = document.getElementById("error");

try {
  await init();
  run();
} catch (err) {
  errorBox.textContent = `Failed to start: ${err}\nBuild first: wasm-pack build --target web`;
  throw err;
}

function run() {
  const bmain = new BlenderMain();

  // Scene setup, the same calls the depsgraph/render tickets will make.
  const scene = bmain.scene_add("Scene");
  const cubeMesh = bmain.mesh_add_cube(2.0);
  const sphereMesh = bmain.mesh_add_uv_sphere(1.0, 32, 16);
  const gridMesh = bmain.mesh_add_grid(16, 16, 10.0, 10.0);

  const cube = bmain.object_add_mesh("Cube", cubeMesh);
  const moon = bmain.object_add_mesh("Moon", sphereMesh);
  const floor = bmain.object_add_mesh("Floor", gridMesh);
  for (const ob of [cube, moon, floor]) bmain.scene_link_object(scene, ob);

  // Moon is parented to the cube, so it orbits when the cube spins.
  bmain.object_set_parent(moon, cube);
  bmain.object_set_location(moon, 3.0, 0.0, 0.0);
  bmain.object_set_scale(moon, 0.5, 0.5, 0.5);
  bmain.object_set_location(floor, 0.0, 0.0, -1.5);

  const gridRest = bmain.mesh_positions(gridMesh);

  // Per-mesh GPU-style buffers, refreshed only when take_changes() reports GEOMETRY.
  const meshCache = new Map();
  const meshBuffers = (mesh) => {
    if (!meshCache.has(mesh)) {
      meshCache.set(mesh, { positions: bmain.mesh_positions(mesh), edges: bmain.mesh_edges(mesh) });
    }
    return meshCache.get(mesh);
  };

  const canvas = document.getElementById("viewport");
  const ctx = canvas.getContext("2d");
  const spin = document.getElementById("spin");
  const wave = document.getElementById("wave");

  document.getElementById("blocks").textContent = [cubeMesh, sphereMesh, gridMesh]
    .map((m) => {
      const [v, e, f, c] = bmain.mesh_counts(m);
      return `${bmain.mesh_name(m).padEnd(8)} ${v} verts  ${e} edges  ${f} faces  ${c} corners`;
    })
    .join("\n");

  let angle = 0;
  const start = performance.now();

  function frame(now) {
    const t = (now - start) / 1000;
    if (spin.checked) {
      angle += 0.01;
      bmain.object_set_rotation_euler(cube, 0, 0, angle);
    }
    if (wave.checked) {
      const p = gridRest.slice();
      for (let i = 0; i < p.length; i += 3) p[i + 2] = 0.3 * Math.sin(p[i] * 1.2 + t * 2);
      bmain.mesh_set_positions(gridMesh, p);
    }

    // What the depsgraph would do: read the changes, refresh what they affect.
    const changes = bmain.take_changes();
    const lines = [];
    for (let i = 0; i < changes.length; i += 3) {
      const [type, handle, flags] = [changes[i], changes[i + 1], changes[i + 2]];
      if (type === 2 && flags & 2) meshCache.delete(handle);
      const name = type === 1 ? bmain.object_name(handle) : type === 2 ? bmain.mesh_name(handle) : handle;
      const flagText = FLAG_NAMES.filter(([bit]) => (flags & bit) >>> 0).map(([, n]) => n).join(" | ");
      lines.push(`${TYPE_NAMES[type].padEnd(6)} ${String(name).padEnd(7)} ${flagText}`);
    }
    document.getElementById("changes").textContent = lines.join("\n") || "(none)";

    draw(bmain, scene, meshBuffers, ctx, canvas);
    requestAnimationFrame(frame);
  }
  requestAnimationFrame(frame);
}

function draw(bmain, scene, meshBuffers, ctx, canvas) {
  const dpr = window.devicePixelRatio || 1;
  const w = canvas.clientWidth * dpr;
  const h = canvas.clientHeight * dpr;
  if (canvas.width !== w || canvas.height !== h) [canvas.width, canvas.height] = [w, h];
  ctx.clearRect(0, 0, w, h);

  // Fixed camera: looking along +Y from y = -12, tilted down.
  const tilt = 0.45, dist = 12, focal = Math.min(w, h) * 1.6;
  const [ct, st] = [Math.cos(tilt), Math.sin(tilt)];
  const project = (x, y, z) => {
    const yv = y * ct - z * st;
    const zv = y * st + z * ct;
    const depth = yv + dist;
    return [w / 2 + (x / depth) * focal, h / 2 - (zv / depth) * focal];
  };

  ctx.lineWidth = dpr;
  for (const ob of bmain.scene_objects(scene)) {
    const mesh = bmain.object_mesh(ob);
    if (mesh === NONE) continue;
    const m = bmain.object_world_matrix(ob); // column-major
    const { positions: p, edges } = meshBuffers(mesh);

    const screen = new Float32Array((p.length / 3) * 2);
    for (let v = 0, s = 0; v < p.length; v += 3, s += 2) {
      const [x, y, z] = [p[v], p[v + 1], p[v + 2]];
      const wx = m[0] * x + m[4] * y + m[8] * z + m[12];
      const wy = m[1] * x + m[5] * y + m[9] * z + m[13];
      const wz = m[2] * x + m[6] * y + m[10] * z + m[14];
      [screen[s], screen[s + 1]] = project(wx, wy, wz);
    }

    ctx.strokeStyle = bmain.object_name(ob) === "Floor" ? "#6b6b6b" : "#e87d0d";
    ctx.beginPath();
    for (let e = 0; e < edges.length; e += 2) {
      const [a, b] = [edges[e] * 2, edges[e + 1] * 2];
      ctx.moveTo(screen[a], screen[a + 1]);
      ctx.lineTo(screen[b], screen[b + 1]);
    }
    ctx.stroke();
  }
}
