import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
// Vite serves only the development renderer. The packaged desktop app loads bundled assets.
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: { host: "127.0.0.1", port: 1420, strictPort: true },
  envPrefix: ["VITE_", "TAURI_ENV_*"],
  build: { outDir: "dist", target: "es2022" },
});
