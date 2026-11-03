import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import { fileURLToPath } from "node:url";

// Fixed port: tauri.conf.json devUrl points here. Two pages: the app, and the 速记小窗 (capture.html).
export default defineConfig({
  plugins: [react(), tailwindcss()],
  resolve: { alias: { "@": fileURLToPath(new URL("./src", import.meta.url)) } },
  clearScreen: false,
  build: { rollupOptions: { input: { main: fileURLToPath(new URL("./index.html", import.meta.url)), capture: fileURLToPath(new URL("./capture.html", import.meta.url)) } } },
  server: {
    port: 1420,
    strictPort: true,
    watch: { ignored: ["**/src-tauri/**"] },
  },
});
