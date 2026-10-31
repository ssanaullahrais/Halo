import { defineConfig } from "vite";
import { resolve } from "node:path";

export default defineConfig({
  clearScreen: false,
  server: { port: 1420, strictPort: true, host: "127.0.0.1", watch: { ignored: ["**/output/**"] } },
  envPrefix: ["VITE_", "TAURI_"],
  build: {
    target: "chrome105",
    outDir: "dist",
    emptyOutDir: true,
    rollupOptions: {
      input: { main: resolve(__dirname, "index.html"),zoomPreview:resolve(__dirname,"recorder-zoom-preview.html"), home: resolve(__dirname, "home.html"), notice: resolve(__dirname, "notice.html"), recorder:resolve(__dirname,"recorder.html"), selection:resolve(__dirname,"recorder-select.html"),countdown:resolve(__dirname,"recorder-countdown.html"),annotations:resolve(__dirname,"recorder-annotations.html") },
    },
  },
});
