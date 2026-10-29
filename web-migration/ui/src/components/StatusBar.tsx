import { useEffect, useState } from "react";
import type { Engine } from "../engine";

/// Scene statistics and frame rate, like Blender's status bar.
export function StatusBar({ engine }: { engine: Engine }) {
  const [fps, setFps] = useState(0);
  useEffect(() => {
    const timer = setInterval(() => setFps(engine.fps()), 1000);
    return () => clearInterval(timer);
  }, [engine]);

  const stats = engine.sceneStats();
  return (
    <footer className="statusbar">
      <span>Scene</span>
      <span>Objects {stats.objects}</span>
      <span>Vertices {stats.verts.toLocaleString()}</span>
      <span>Faces {stats.faces.toLocaleString()}</span>
      <span>Triangles {stats.tris.toLocaleString()}</span>
      <span className="spacer" />
      <span>{engine.backend || "…"}</span>
      <span>{fps} fps</span>
    </footer>
  );
}
