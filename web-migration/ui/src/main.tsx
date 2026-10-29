import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";
import { Engine } from "./engine";
import "./styles.css";

const root = createRoot(document.getElementById("root")!);
root.render(<div className="splash">Loading Blender Web…</div>);

Engine.create()
  .then((engine) =>
    root.render(
      <StrictMode>
        <App engine={engine} />
      </StrictMode>,
    ),
  )
  .catch((err) => {
    root.render(
      <div className="splash error">
        Failed to load the engine: {String(err)}
        <br />
        Build it first: <code>npm run wasm</code>
      </div>,
    );
    throw err;
  });
