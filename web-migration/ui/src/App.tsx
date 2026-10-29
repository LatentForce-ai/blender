import { useEffect, useState, useSyncExternalStore } from "react";
import type { Engine } from "./engine";
import { Toolbar } from "./components/Toolbar";
import { Outliner } from "./components/Outliner";
import { ViewportPanel } from "./components/ViewportPanel";
import { Properties } from "./components/Properties";
import { StatusBar } from "./components/StatusBar";

/// Blender-like layout: header toolbar, 3D viewport, outliner + properties on the right,
/// status bar at the bottom.
export function App({ engine }: { engine: Engine }) {
  useSyncExternalStore(engine.subscribe, engine.getRevision);
  const objects = engine.objects();
  const [selected, setSelected] = useState<number | null>(() => objects[0]?.handle ?? null);
  const active = objects.some((o) => o.handle === selected) ? selected : null;

  // X / Delete deletes the active object, as in Blender (not while typing in a field).
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      const target = event.target as HTMLElement;
      if (target.closest("input, textarea, select")) return;
      if ((event.key === "Delete" || event.key === "x") && active !== null) {
        engine.deleteObject(active);
        event.preventDefault();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [engine, active]);

  return (
    <div className="app">
      <Toolbar
        engine={engine}
        active={active}
        onAdded={(handle) => setSelected(handle)}
      />
      <main className="workspace">
        <ViewportPanel engine={engine} />
        <aside className="sidebar">
          <Outliner engine={engine} objects={objects} active={active} onSelect={setSelected} />
          <Properties engine={engine} active={active} />
        </aside>
      </main>
      <StatusBar engine={engine} />
    </div>
  );
}
