import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// Vite config tuned for Tauri: a fixed dev port the Rust side points `devUrl`
// at, no screen-clearing (so Tauri's logs stay visible), and env vars are read
// from the workspace. `dist/` is what `tauri.conf.json`'s `frontendDist`
// embeds into the desktop binary.
export default defineConfig({
  plugins: [react()],
  // Prevent Vite from obscuring Rust errors in the Tauri dev console.
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: false,
    watch: {
      // The Rust GUI crate is built by Cargo, not Vite — don't watch it.
      ignored: ["**/target/**", "**/../crates/**"],
    },
  },
  css: {
    preprocessorOptions: {
      // Use Dart Sass's modern compiler API (the legacy API Vite 5 defaults to
      // is deprecated and noisy). `@use`-based Carbon config works unchanged.
      scss: {
        api: "modern-compiler",
      },
    },
  },
  build: {
    outDir: "dist",
    // Tauri v2 targets modern WebViews; match the workspace's Rust edition era.
    target: "es2021",
    // Smaller, faster: sourcemaps only when explicitly debugging.
    sourcemap: false,
  },
});
