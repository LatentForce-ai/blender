import { useEffect, useRef } from "react";
import type { Engine } from "../engine";

/// The 3D viewport: the render crate draws into this canvas (WebGPU, or WebGL2 as fallback).
/// Drag to orbit, scroll to zoom.
export function ViewportPanel({ engine }: { engine: Engine }) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const attached = useRef(false);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas || attached.current) return;
    attached.current = true;
    engine.attachViewport(canvas).catch((err) => {
      engine.error = `Viewport failed: ${err}`;
      engine.changed();
    });

    const onWheel = (event: WheelEvent) => {
      event.preventDefault();
      engine.zoom(event.deltaY);
    };
    canvas.addEventListener("wheel", onWheel, { passive: false });
    return () => canvas.removeEventListener("wheel", onWheel);
  }, [engine]);

  return (
    <section className="viewport">
      <div className="viewport-header">
        <span>3D Viewport</span>
        <span className="pill">Solid</span>
        <span className="hint">Drag to orbit · Scroll to zoom</span>
        <span className="backend">{engine.backend || "starting…"}</span>
      </div>
      <canvas
        ref={canvasRef}
        aria-label="3D viewport"
        onPointerDown={(e) => e.currentTarget.setPointerCapture(e.pointerId)}
        onPointerMove={(e) => {
          if (e.currentTarget.hasPointerCapture(e.pointerId)) engine.orbit(e.movementX, e.movementY);
        }}
      />
    </section>
  );
}
