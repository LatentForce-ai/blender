import { useState } from "react";
import { NONE, type Engine, type ObjectInfo } from "../engine";

/// Scene hierarchy by parenting, like Blender's Outliner in View Layer mode.
export function Outliner({
  engine,
  objects,
  active,
  onSelect,
}: {
  engine: Engine;
  objects: ObjectInfo[];
  active: number | null;
  onSelect: (handle: number) => void;
}) {
  const inScene = new Set(objects.map((o) => o.handle));
  const roots = objects.filter((o) => o.parent === NONE || !inScene.has(o.parent));
  const childrenOf = (handle: number) => objects.filter((o) => o.parent === handle);

  const renderRow = (object: ObjectInfo, depth: number) => (
    <li key={object.handle}>
      <OutlinerRow
        engine={engine}
        object={object}
        depth={depth}
        active={object.handle === active}
        onSelect={onSelect}
      />
      {childrenOf(object.handle).length > 0 && (
        <ul>{childrenOf(object.handle).map((child) => renderRow(child, depth + 1))}</ul>
      )}
    </li>
  );

  return (
    <section className="panel outliner">
      <h2>Outliner</h2>
      <div className="outliner-scene">Scene Collection</div>
      {objects.length === 0 ? (
        <p className="empty">No objects. Add one from the toolbar.</p>
      ) : (
        <ul role="tree">{roots.map((o) => renderRow(o, 0))}</ul>
      )}
    </section>
  );
}

function OutlinerRow({
  engine,
  object,
  depth,
  active,
  onSelect,
}: {
  engine: Engine;
  object: ObjectInfo;
  depth: number;
  active: boolean;
  onSelect: (handle: number) => void;
}) {
  const [renaming, setRenaming] = useState(false);
  const isMesh = object.mesh !== NONE;

  return (
    <div
      className={`outliner-row${active ? " active" : ""}`}
      style={{ paddingLeft: 12 + depth * 16 }}
      role="treeitem"
      aria-selected={active}
      onClick={() => onSelect(object.handle)}
      onDoubleClick={() => setRenaming(true)}
      title="Click to select, double-click to rename"
    >
      <span className={`icon ${isMesh ? "icon-mesh" : "icon-empty"}`} aria-hidden>
        {isMesh ? "▲" : "✛"}
      </span>
      {renaming ? (
        <input
          autoFocus
          defaultValue={object.name}
          onClick={(e) => e.stopPropagation()}
          onBlur={(e) => {
            engine.edit(() => engine.bmain.object_rename(object.handle, e.target.value));
            setRenaming(false);
          }}
          onKeyDown={(e) => {
            if (e.key === "Enter") (e.target as HTMLInputElement).blur();
            if (e.key === "Escape") setRenaming(false);
          }}
        />
      ) : (
        <span className="name">{object.name}</span>
      )}
    </div>
  );
}
