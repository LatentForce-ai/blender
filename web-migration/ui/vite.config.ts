import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// The engine is the `app` crate's wasm-pack output in ../app/pkg, outside this folder.
export default defineConfig({
  plugins: [react()],
  server: { fs: { allow: [".."] } },
  build: { target: "es2022" },
});
