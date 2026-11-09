import { defineConfig } from "vite";
import tailwindcss from "@tailwindcss/vite";

// Port fixe exigé par Tauri (devUrl dans tauri.conf.json).
export default defineConfig({
  plugins: [tailwindcss()],
  clearScreen: false,
  server: { port: 1420, strictPort: true, watch: { ignored: ["**/src-tauri/**"] } },
});
