import { NONE, hexToLinear, linearToHex, type Engine } from "../engine";
import { NumberField } from "./NumberField";

const AXES = ["X", "Y", "Z"] as const;
const RAD_TO_DEG = 180 / Math.PI;
const ROTATION_MODE_QUATERNION = 0;

/// Properties editor for the active object: Object (name, transform, parent), Material
/// (slots with viewport display values) and Mesh data stats.
export function Properties({ engine, active }: { engine: Engine; active: number | null }) {
  if (active === null) {
    return (
      <section className="panel properties">
        <h2>Properties</h2>
        <p className="empty">Select an object in the Outliner.</p>
      </section>
    );
  }
  return (
    <section className="panel properties">
      <h2>Properties</h2>
      <ObjectProperties engine={engine} object={active} />
      <MaterialProperties engine={engine} object={active} />
      <MeshProperties engine={engine} object={active} />
      {engine.error && <p className="error" role="alert">{engine.error}</p>}
    </section>
  );
}

function ObjectProperties({ engine, object }: { engine: Engine; object: number }) {
  const b = engine.bmain;
  const location = b.object_location(object);
  const rotation = b.object_rotation_euler(object);
  const scale = b.object_scale(object);
  const quaternionMode = b.object_rotation_mode(object) === ROTATION_MODE_QUATERNION;
  const parent = b.object_parent(object);
  const others = engine.objects().filter((o) => o.handle !== object);

  const setVector = (
    current: Float32Array,
    axis: number,
    value: number,
    apply: (x: number, y: number, z: number) => void,
  ) => {
    const next = [...current];
    next[axis] = value;
    engine.edit(() => apply(next[0], next[1], next[2]));
  };

  return (
    <details open className="section">
      <summary>Object</summary>
      <label className="text-field">
        <span>Name</span>
        <input
          key={`${object}-${b.object_name(object)}`}
          defaultValue={b.object_name(object)}
          onBlur={(e) => engine.edit(() => b.object_rename(object, e.target.value))}
          onKeyDown={(e) => e.key === "Enter" && (e.target as HTMLInputElement).blur()}
        />
      </label>

      <fieldset>
        <legend>Location</legend>
        {AXES.map((axis, i) => (
          <NumberField
            key={axis}
            label={axis}
            value={location[i]}
            onChange={(v) => setVector(location, i, v, (x, y, z) => b.object_set_location(object, x, y, z))}
          />
        ))}
      </fieldset>

      <fieldset>
        <legend>Rotation (°)</legend>
        {quaternionMode ? (
          <p className="note">Quaternion rotation: not editable here yet.</p>
        ) : (
          AXES.map((axis, i) => (
            <NumberField
              key={axis}
              label={axis}
              step={5}
              precision={2}
              value={rotation[i] * RAD_TO_DEG}
              onChange={(deg) =>
                setVector(rotation, i, deg / RAD_TO_DEG, (x, y, z) =>
                  b.object_set_rotation_euler(object, x, y, z),
                )
              }
            />
          ))
        )}
      </fieldset>

      <fieldset>
        <legend>Scale</legend>
        {AXES.map((axis, i) => (
          <NumberField
            key={axis}
            label={axis}
            value={scale[i]}
            onChange={(v) => setVector(scale, i, v, (x, y, z) => b.object_set_scale(object, x, y, z))}
          />
        ))}
      </fieldset>

      <label className="text-field">
        <span>Parent</span>
        <select
          value={parent}
          onChange={(e) => engine.edit(() => b.object_set_parent(object, Number(e.target.value)))}
        >
          <option value={NONE}>None</option>
          {others.map((o) => (
            <option key={o.handle} value={o.handle}>
              {o.name}
            </option>
          ))}
        </select>
      </label>
    </details>
  );
}

function MaterialProperties({ engine, object }: { engine: Engine; object: number }) {
  const b = engine.bmain;
  const mesh = b.object_mesh(object);
  if (mesh === NONE) return null;
  const slots = [...b.mesh_materials(mesh)];

  return (
    <details open className="section">
      <summary>Material</summary>
      {slots.length === 0 && <p className="note">No material slots.</p>}
      {slots.map((material, slot) => (
        <div className="material-slot" key={`${slot}-${material}`}>
          <div className="slot-header">
            <span>Slot {slot + 1}</span>
            <button
              className="small"
              onClick={() => engine.removeMaterialSlot(object, slot)}
              title="Remove this material slot"
            >
              Remove
            </button>
          </div>
          {material === NONE ? (
            <p className="note">Empty slot</p>
          ) : (
            <>
              <MaterialFields engine={engine} material={material} />
              <SurfaceFields engine={engine} material={material} />
            </>
          )}
        </div>
      ))}
      <button onClick={() => engine.addMaterial(object)}>New Material</button>
    </details>
  );
}

function MaterialFields({ engine, material }: { engine: Engine; material: number }) {
  const b = engine.bmain;
  const [r, g, bl, a, metallic, roughness] = b.material_viewport(material);
  const set = (values: Partial<{ rgb: [number, number, number]; metallic: number; roughness: number }>) => {
    const [nr, ng, nb] = values.rgb ?? [r, g, bl];
    engine.edit(() =>
      b.material_set_viewport(
        material,
        nr,
        ng,
        nb,
        a,
        values.metallic ?? metallic,
        values.roughness ?? roughness,
      ),
    );
  };

  return (
    <>
      <label className="text-field">
        <span>Name</span>
        <input
          key={`${material}-${b.material_name(material)}`}
          defaultValue={b.material_name(material)}
          onBlur={(e) => engine.edit(() => b.material_rename(material, e.target.value))}
          onKeyDown={(e) => e.key === "Enter" && (e.target as HTMLInputElement).blur()}
        />
      </label>
      <label className="text-field">
        <span>Color</span>
        <input
          type="color"
          value={linearToHex([r, g, bl])}
          onChange={(e) => set({ rgb: hexToLinear(e.target.value) })}
        />
      </label>
      <NumberField label="Metallic" value={metallic} step={0.05} min={0} max={1} onChange={(v) => set({ metallic: v })} />
      <NumberField label="Roughness" value={roughness} step={0.05} min={0} max={1} onChange={(v) => set({ roughness: v })} />
      <p className="note">Viewport display values (Solid mode).</p>
    </>
  );
}

/// Blender's "Surface" panel: the Principled BSDF feeding the material output, which Material
/// Preview draws.
function SurfaceFields({ engine, material }: { engine: Engine; material: number }) {
  const b = engine.bmain;
  if (!b.material_has_node_tree(material)) {
    return <button onClick={() => engine.useNodes(material)}>Use Nodes</button>;
  }
  const bsdf = engine.surfaceBsdf(material);
  if (bsdf === undefined) {
    return <p className="note">Surface isn't a Principled BSDF; edit its nodes through the API.</p>;
  }
  const input = (socket: string) => [...b.material_node_input(material, bsdf, socket)];
  const set = (socket: string, values: number[]) => engine.setNodeInput(material, bsdf, socket, values);
  const baseColor = input("Base Color");
  const emissionColor = input("Emission Color");

  return (
    <>
      <h4 className="subsection">Surface · Principled BSDF</h4>
      <label className="text-field">
        <span>Base Color</span>
        <input
          type="color"
          value={linearToHex([baseColor[0], baseColor[1], baseColor[2]])}
          onChange={(e) => set("Base Color", [...hexToLinear(e.target.value), baseColor[3]])}
        />
      </label>
      <NumberField label="Metallic" value={input("Metallic")[0]} step={0.05} min={0} max={1} onChange={(v) => set("Metallic", [v])} />
      <NumberField label="Roughness" value={input("Roughness")[0]} step={0.05} min={0} max={1} onChange={(v) => set("Roughness", [v])} />
      <label className="text-field">
        <span>Emission</span>
        <input
          type="color"
          value={linearToHex([emissionColor[0], emissionColor[1], emissionColor[2]])}
          onChange={(e) => set("Emission Color", [...hexToLinear(e.target.value), emissionColor[3]])}
        />
      </label>
      <NumberField
        label="Strength"
        value={input("Emission Strength")[0]}
        step={0.1}
        min={0}
        onChange={(v) => set("Emission Strength", [v])}
      />
      <p className="note">Node values (Material Preview).</p>
    </>
  );
}

function MeshProperties({ engine, object }: { engine: Engine; object: number }) {
  const mesh = engine.bmain.object_mesh(object);
  if (mesh === NONE) return null;
  const stats = engine.meshStats(mesh);
  return (
    <details className="section">
      <summary>Mesh Data</summary>
      <dl className="stats">
        <dt>Name</dt>
        <dd>{engine.bmain.mesh_name(mesh)}</dd>
        <dt>Vertices</dt>
        <dd>{stats.verts.toLocaleString()}</dd>
        <dt>Edges</dt>
        <dd>{stats.edges.toLocaleString()}</dd>
        <dt>Faces</dt>
        <dd>{stats.faces.toLocaleString()}</dd>
        <dt>Triangles</dt>
        <dd>{stats.tris.toLocaleString()}</dd>
      </dl>
    </details>
  );
}
