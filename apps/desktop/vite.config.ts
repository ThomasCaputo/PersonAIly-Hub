import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
// @ts-expect-error type error without @types/node package
import process from "node:process";
const host = process.env.TAURI_DEV_HOST;

/**
 * Il CSS di KaTeX offre ogni font in tre formati (woff2, woff, ttf) e Vite li
 * metterebbe tutti nell'eseguibile. WebView2 legge woff2: gli altri due sono
 * circa 880 kB inutili.
 */
const katexWoff2Only = {
  name: "katex-woff2-only",
  enforce: "pre" as const,
  transform(code: string, id: string) {
    if (!/katex(\.min)?\.css$/.test(id.split("?")[0])) return null;
    return code.replace(/,\s*url\([^)]+\.(woff|ttf)\)\s*format\("(woff|truetype)"\)/g, "");
  },
};

// https://vite.dev/config/
export default defineConfig(() => ({
  plugins: [react(), katexWoff2Only],

  // Il pezzo più grande (Markdown con KaTeX e highlight.js, circa 600 kB) non è
  // nel file principale: si carica dopo l'apertura della finestra.
  build: {
    chunkSizeWarningLimit: 700,
  },

  // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
  //
  // 1. prevent Vite from obscuring rust errors
  clearScreen: false,
  // 2. tauri expects a fixed port, fail if that port is not available
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      // 3. tell Vite to ignore watching `src-tauri`
      ignored: ["**/src-tauri/**"],
    },
  },
}));
