import { useEffect, useRef } from "react";
import type { Engine, ShadingType } from "../engine";

const SHADING_TYPES: [ShadingType, string][] = [
  ["SOLID", "Solid"],
  ["MATERIAL", "Material Preview"],
];

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
        <div className="shading-toggle" role="group" aria-label="Viewport shading">
          {SHADING_TYPES.map(([type, label]) => (
            <button
              key={type}
              className={engine.shadingType === type ? "active" : ""}
              aria-pressed={engine.shadingType === type}
              onClick={() => engine.setShadingType(type)}
            >
              {label}
            </button>
          ))}
        </div>
        {engine.shadingType === "MATERIAL" && (
          <div className="environment">
            <label className="button" title="Light Material Preview with a Radiance .hdr environment">
              {engine.environment || "Studio"} · Load HDRI…
              <input
                type="file"
                accept=".hdr"
                hidden
                onChange={(e) => {
                  const file = e.target.files?.[0];
                  if (file) engine.loadEnvironment(file);
                  e.target.value = "";
                }}
              />
            </label>
            {engine.environment && <button onClick={() => engine.resetEnvironment()}>Studio</button>}
          </div>
        )}
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
