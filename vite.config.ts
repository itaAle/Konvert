import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// https://vite.dev/config/ — options tailored for Tauri development.
export default defineConfig({
  plugins: [react()],
  // Keep Rust compiler errors visible.
  clearScreen: false,
  server: {
    // Tauri expects a fixed port; fail if it is taken. Bound to localhost only.
    port: 1420,
    strictPort: true,
    host: "localhost",
    watch: {
      ignored: ["**/src-tauri/**", "**/crates/**", "**/target/**"],
    },
  },
  build: {
    // WebView2 (Chromium) on Windows; Safari 15 / WebKitGTK for later platforms.
    target: ["chrome120", "safari15"],
  },
});
