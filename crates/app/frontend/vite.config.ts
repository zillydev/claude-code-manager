import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";

const host = process.env.TAURI_DEV_HOST;
// Dev-server port; overridable via `make dev PORT=…` (passed in as CM_PORT).
const port = Number(process.env.CM_PORT) || 5173;

export default defineConfig({
  plugins: [react(), tailwindcss()],
  clearScreen: false,
  server: {
    port,
    strictPort: true,
    host: host || false,
    hmr: host
      ? { protocol: "ws", host, port: port + 1 }
      : undefined,
  },
});
