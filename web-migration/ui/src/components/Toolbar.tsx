import type { Engine, PrimitiveKind } from "../engine";

const PRIMITIVES: { kind: PrimitiveKind; label: string }[] = [
  { kind: "cube", label: "Cube" },
  { kind: "uv_sphere", label: "UV Sphere" },
  { kind: "plane", label: "Plane" },
  { kind: "grid", label: "Grid" },
  { kind: "empty", label: "Empty" },
];

const NOT_BUILT = "Undo & file save (HACK-8) is not built yet";

export function Toolbar({
  engine,
  active,
  onAdded,
}: {
  engine: Engine;
  active: number | null;
  onAdded: (handle: number) => void;
}) {
  return (
    <header className="toolbar">
      <span className="brand">Blender Web</span>
      <div className="group" role="group" aria-label="Add object">
        <span className="group-label">Add</span>
        {PRIMITIVES.map(({ kind, label }) => (
          <button
            key={kind}
            onClick={() => {
              const handle = engine.addPrimitive(kind);
              if (handle !== undefined) onAdded(handle);
            }}
          >
            {label}
          </button>
        ))}
      </div>
      <div className="group">
        <button
          disabled={active === null}
          onClick={() => active !== null && engine.deleteObject(active)}
          title="Delete the active object (X / Delete)"
        >
          Delete
        </button>
      </div>
      <div className="group">
        <button disabled title={NOT_BUILT}>Undo</button>
        <button disabled title={NOT_BUILT}>Redo</button>
        <button disabled title={NOT_BUILT}>Save</button>
        <button disabled title={NOT_BUILT}>Open</button>
      </div>
    </header>
  );
}
