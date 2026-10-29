// Bridge between React and the Blender engine (the `app` crate's WebAssembly build).
// The scene lives in Rust; React reads it through `BlenderMain` and re-renders when `revision`
// changes. Every UI edit goes through an Engine method, which bumps the revision.

import init, { BlenderMain, Depsgraph, Shading, Viewport, tick } from "../../app/pkg/app.js";

/// Handle value meaning "none" in the blenkernel API.
export const NONE = 0xffffffff;

/// Browsers report one mouse-wheel notch as 100 pixels of `deltaY`.
const WHEEL_PIXELS_PER_STEP = 100;
/// Blender's default cube is 2 m across.
const DEFAULT_CUBE_SIZE = 2;

export type PrimitiveKind = "cube" | "uv_sphere" | "plane" | "grid" | "empty";

export interface ObjectInfo {
  handle: number;
  name: string;
  parent: number;
  mesh: number;
}

export interface MeshStats {
  verts: number;
  edges: number;
  faces: number;
  tris: number;
}

export class Engine {
  readonly bmain: BlenderMain;
  readonly scene: number;
  private readonly depsgraph: Depsgraph;
  /// Evaluates material node trees each frame (HACK-7).
  private readonly shading = new Shading();
  private viewport: Viewport | null = null;
  private listeners = new Set<() => void>();
  private frameTimes: number[] = [];
  revision = 0;
  backend = "";
  error = "";

  private constructor() {
    this.bmain = new BlenderMain();
    this.scene = this.bmain.scene_add("Scene");
    this.depsgraph = new Depsgraph(this.scene);
    this.addStartupScene();
  }

  static async create(): Promise<Engine> {
    await init();
    return new Engine();
  }

  /// Blender's startup file: the default cube with its default "Material".
  private addStartupScene() {
    const mesh = this.bmain.mesh_add_cube(DEFAULT_CUBE_SIZE);
    this.bmain.mesh_set_materials(mesh, new Uint32Array([this.bmain.material_add("Material", true)]));
    this.bmain.scene_link_object(this.scene, this.bmain.object_add_mesh("Cube", mesh));
  }

  // ---------------------------------------------------------------- change notification

  subscribe = (listener: () => void) => {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  };

  getRevision = () => this.revision;

  /// Call after any edit so panels re-read the scene.
  changed() {
    this.revision++;
    this.listeners.forEach((listener) => listener());
  }

  /// Run an edit; a thrown engine error is reported instead of crashing the UI.
  edit<T>(action: () => T): T | undefined {
    try {
      const result = action();
      this.error = "";
      return result;
    } catch (err) {
      this.error = String(err instanceof Error ? err.message : err);
      return undefined;
    } finally {
      this.changed();
    }
  }

  // ---------------------------------------------------------------- viewport

  async attachViewport(canvas: HTMLCanvasElement) {
    fitCanvas(canvas);
    this.viewport = await Viewport.create(canvas);
    this.backend = this.viewport.backend();
    this.changed();

    const observer = new ResizeObserver(() => {
      fitCanvas(canvas);
      this.viewport?.resize(canvas.width, canvas.height);
    });
    observer.observe(canvas);

    const frame = (now: number) => {
      if (this.viewport) {
        try {
          tick(this.bmain, this.depsgraph, this.shading, this.viewport);
        } catch (err) {
          this.error = `Draw failed: ${err}`;
        }
      }
      this.frameTimes.push(now);
      while (this.frameTimes.length && now - this.frameTimes[0] > 1000) this.frameTimes.shift();
      requestAnimationFrame(frame);
    };
    requestAnimationFrame(frame);
  }

  orbit(dx: number, dy: number) {
    this.viewport?.orbit(dx, dy);
  }

  zoom(deltaY: number) {
    this.viewport?.zoom(deltaY / WHEEL_PIXELS_PER_STEP);
  }

  fps(): number {
    return this.frameTimes.length;
  }

  // ---------------------------------------------------------------- scene queries

  objects(): ObjectInfo[] {
    return [...this.bmain.scene_objects(this.scene)].map((handle) => ({
      handle,
      name: this.bmain.object_name(handle),
      parent: this.bmain.object_parent(handle),
      mesh: this.bmain.object_mesh(handle),
    }));
  }

  meshStats(mesh: number): MeshStats {
    const [verts, edges, faces] = this.bmain.mesh_counts(mesh);
    return { verts, edges, faces, tris: this.bmain.mesh_triangles(mesh).length / 3 };
  }

  sceneStats(): MeshStats & { objects: number } {
    const totals = { objects: 0, verts: 0, edges: 0, faces: 0, tris: 0 };
    for (const object of this.objects()) {
      totals.objects++;
      if (object.mesh === NONE) continue;
      const stats = this.meshStats(object.mesh);
      totals.verts += stats.verts;
      totals.edges += stats.edges;
      totals.faces += stats.faces;
      totals.tris += stats.tris;
    }
    return totals;
  }

  // ---------------------------------------------------------------- edits

  /// Add a primitive at the origin (Blender adds at the 3D cursor), linked to the scene.
  addPrimitive(kind: PrimitiveKind): number | undefined {
    return this.edit(() => {
      const b = this.bmain;
      let object: number;
      switch (kind) {
        case "cube":
          object = b.object_add_mesh("Cube", b.mesh_add_cube(DEFAULT_CUBE_SIZE));
          break;
        case "uv_sphere":
          object = b.object_add_mesh("Sphere", b.mesh_add_uv_sphere(1, 32, 16));
          break;
        case "plane":
          object = b.object_add_mesh("Plane", b.mesh_add_grid(2, 2, 2, 2));
          break;
        case "grid":
          object = b.object_add_mesh("Grid", b.mesh_add_grid(10, 10, 2, 2));
          break;
        case "empty":
          object = b.object_add_empty("Empty");
          break;
      }
      b.scene_link_object(this.scene, object);
      return object;
    });
  }

  /// Delete an object; its mesh goes too once nothing else uses it.
  deleteObject(object: number) {
    this.edit(() => {
      const mesh = this.bmain.object_mesh(object);
      this.bmain.object_remove(object);
      if (mesh !== NONE) {
        try {
          this.bmain.mesh_remove(mesh);
        } catch {
          // Still used by another object.
        }
      }
    });
  }

  /// Give a mesh object a new default material in a new slot.
  addMaterial(object: number): number | undefined {
    return this.edit(() => {
      const mesh = this.bmain.object_mesh(object);
      if (mesh === NONE) throw new Error("Only mesh objects have materials");
      const material = this.bmain.material_add("Material", true);
      const slots = [...this.bmain.mesh_materials(mesh), material];
      this.bmain.mesh_set_materials(mesh, new Uint32Array(slots));
      return material;
    });
  }

  removeMaterialSlot(object: number, slot: number) {
    this.edit(() => {
      const mesh = this.bmain.object_mesh(object);
      const slots = [...this.bmain.mesh_materials(mesh)];
      slots.splice(slot, 1);
      this.bmain.mesh_set_materials(mesh, new Uint32Array(slots));
    });
  }
}

function fitCanvas(canvas: HTMLCanvasElement) {
  const scale = window.devicePixelRatio || 1;
  canvas.width = Math.max(1, Math.round(canvas.clientWidth * scale));
  canvas.height = Math.max(1, Math.round(canvas.clientHeight * scale));
}

// Material colours are scene-linear, like Blender's; colour inputs show sRGB.
export function linearToSrgb(c: number): number {
  return c <= 0.0031308 ? c * 12.92 : 1.055 * Math.pow(c, 1 / 2.4) - 0.055;
}

export function srgbToLinear(c: number): number {
  return c <= 0.04045 ? c / 12.92 : Math.pow((c + 0.055) / 1.055, 2.4);
}

export function linearToHex([r, g, b]: Iterable<number>): string {
  const hex = (v: number) =>
    Math.round(Math.min(1, Math.max(0, linearToSrgb(v))) * 255)
      .toString(16)
      .padStart(2, "0");
  return `#${hex(r)}${hex(g)}${hex(b)}`;
}

export function hexToLinear(hex: string): [number, number, number] {
  const channel = (i: number) => srgbToLinear(parseInt(hex.slice(i, i + 2), 16) / 255);
  return [channel(1), channel(3), channel(5)];
}
