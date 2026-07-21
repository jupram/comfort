import { defineConfig } from "vite";
import { viteStaticCopy } from "vite-plugin-static-copy";
import { fileURLToPath } from "node:url";

export default defineConfig({
  base: "./",
  build: {
    outDir: "dist",
    emptyOutDir: true,
    sourcemap: true,
    rollupOptions: {
      input: {
        sidepanel: fileURLToPath(new URL("sidepanel.html", import.meta.url)),
        cameraPermission: fileURLToPath(new URL("camera-permission.html", import.meta.url)),
      },
    },
  },
  plugins: [
    viteStaticCopy({
      targets: [
        {
          src: "node_modules/@mediapipe/tasks-vision/wasm/*",
          dest: "wasm",
          rename: { stripBase: true },
        },
      ],
    }),
  ],
});
