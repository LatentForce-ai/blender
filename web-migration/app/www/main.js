import init, { BlenderMain, Depsgraph, Shading, Viewport, tick } from "../pkg/app.js";

/// Blender's default cube is 2 m across.
const DEFAULT_CUBE_SIZE = 2.0;
/// Browsers report one mouse-wheel notch as 100 pixels of `deltaY`.
const WHEEL_PIXELS_PER_STEP = 100;
const DEMO_SPIN_RADIANS_PER_SECOND = 0.5;
const DEMO_FLOOR_VERTS = 40;
const DEMO_FLOOR_SIZE = 12;
const DEMO_WAVE_HEIGHT = 0.3;
const DEMO_WAVE_FREQUENCY = 0.8;
/// Default cube faces in order: -Z, +Z, -Y, +Y, -X, +X. Top and bottom use the second slot.
const DEMO_CUBE_FACE_SLOTS = [1, 1, 0, 0, 0, 0];

/// Adds a material with viewport display values (scene-linear colour), as Solid mode draws.
function addMaterial(bmain, name, [r, g, b], metallic, roughness) {
  const material = bmain.material_add(name, true);
  bmain.material_set_viewport(material, r, g, b, 1.0, metallic, roughness);
  return material;
}

const canvas = document.getElementById("viewport");
const status = document.getElementById("status");

function fitCanvas() {
  const scale = window.devicePixelRatio || 1;
  canvas.width = Math.max(1, Math.round(canvas.clientWidth * scale));
  canvas.height = Math.max(1, Math.round(canvas.clientHeight * scale));
}

/// Blender's startup scene: the default cube with its default "Material".
function defaultScene(bmain) {
  const scene = bmain.scene_add("Scene");
  const cubeMesh = bmain.mesh_add_cube(DEFAULT_CUBE_SIZE);
  bmain.mesh_set_materials(cubeMesh, [bmain.material_add("Material", true)]);
  bmain.scene_link_object(scene, bmain.object_add_mesh("Cube", cubeMesh));
  return { scene, animate: () => {} };
}

/// Exercises every update path: a spinning cube (transforms), a mirrored sphere parented to
/// it (parenting, negative scale) and a floor whose vertices wave (geometry edits).
function demoScene(bmain) {
  const scene = bmain.scene_add("Scene");
  const cubeMesh = bmain.mesh_add_cube(DEFAULT_CUBE_SIZE);
  const sphereMesh = bmain.mesh_add_uv_sphere(0.6, 32, 16);
  const floorMesh = bmain.mesh_add_grid(DEMO_FLOOR_VERTS, DEMO_FLOOR_VERTS, DEMO_FLOOR_SIZE, DEMO_FLOOR_SIZE);
  bmain.mesh_set_materials(cubeMesh, [
    bmain.material_add("Material", true),
    addMaterial(bmain, "Accent", [0.8, 0.25, 0.04], 0.0, 0.4),
  ]);
  bmain.mesh_set_material_indices(cubeMesh, DEMO_CUBE_FACE_SLOTS);
  bmain.mesh_set_materials(sphereMesh, [addMaterial(bmain, "Gold", [0.9, 0.6, 0.2], 1.0, 0.3)]);
  bmain.mesh_set_materials(floorMesh, [addMaterial(bmain, "Water", [0.1, 0.25, 0.6], 0.0, 0.8)]);
  const cube = bmain.object_add_mesh("Cube", cubeMesh);
  const sphere = bmain.object_add_mesh("Sphere", sphereMesh);
  const floor = bmain.object_add_mesh("Floor", floorMesh);
  for (const object of [cube, sphere, floor]) {
    bmain.scene_link_object(scene, object);
  }
  bmain.object_set_parent(sphere, cube);
  bmain.object_set_location(sphere, 2.5, 0, 0);
  bmain.object_set_scale(sphere, -1, 1, 1);
  bmain.object_set_location(floor, 0, 0, -2);
  const rest = bmain.mesh_positions(floorMesh);

  return {
    scene,
    animate(seconds) {
      bmain.object_set_rotation_euler(cube, 0, 0, seconds * DEMO_SPIN_RADIANS_PER_SECOND);
      const waved = rest.slice();
      for (let i = 0; i < waved.length; i += 3) {
        const phase = seconds + DEMO_WAVE_FREQUENCY * (waved[i] + waved[i + 1]);
        waved[i + 2] = DEMO_WAVE_HEIGHT * Math.sin(phase);
      }
      bmain.mesh_set_positions(floorMesh, waved);
    },
  };
}

const SCENES = { default: defaultScene, demo: demoScene };

function bindControls(viewport) {
  canvas.addEventListener("pointerdown", (event) => canvas.setPointerCapture(event.pointerId));
  canvas.addEventListener("pointermove", (event) => {
    if (canvas.hasPointerCapture(event.pointerId)) {
      viewport.orbit(event.movementX, event.movementY);
    }
  });
  canvas.addEventListener(
    "wheel",
    (event) => {
      event.preventDefault();
      viewport.zoom(event.deltaY / WHEEL_PIXELS_PER_STEP);
    },
    { passive: false },
  );
}

try {
  await init();
  fitCanvas();
  const viewport = await Viewport.create(canvas);
  const bmain = new BlenderMain();
  const sceneName = new URLSearchParams(location.search).get("scene") ?? "default";
  const { scene, animate } = (SCENES[sceneName] ?? defaultScene)(bmain);
  const depsgraph = new Depsgraph(scene);
  const shading = new Shading();
  status.textContent = viewport.backend();
  bindControls(viewport);

  new ResizeObserver(() => {
    fitCanvas();
    viewport.resize(canvas.width, canvas.height);
  }).observe(canvas);

  const frame = (milliseconds) => {
    animate(milliseconds / 1000);
    tick(bmain, depsgraph, shading, viewport);
    requestAnimationFrame(frame);
  };
  requestAnimationFrame(frame);
} catch (error) {
  status.textContent = `Viewport failed: ${error}`;
  throw error;
}
