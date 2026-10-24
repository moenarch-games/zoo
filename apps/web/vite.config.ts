import react from "@vitejs/plugin-react"
import { defineConfig } from "vite"

import catalog from "../../crates/zoo-core/content/catalog.json" with { type: "json" }

const catalogSource = JSON.stringify(catalog)

export default defineConfig({
  plugins: [
    react(),
    {
      name: "zoo-owned-content",
      configureServer(server) {
        server.middlewares.use("/content/catalog.json", (_request, response) => {
          response.setHeader("Content-Type", "application/json")
          response.end(catalogSource)
        })
      },
      generateBundle() {
        this.emitFile({ type: "asset", fileName: "content/catalog.json", source: catalogSource })
      },
    },
  ],
  base: "./",
  build: {
    target: "es2022",
    // three.js' WebGL core is ~540 kB minified on its own and cannot be split further; it is
    // isolated in a vendor chunk (below) and loaded lazily with the park view. Its GLTFLoader,
    // which 3d-lab's static-GLB adapter uses for the scenery pieces, adds about 70 kB.
    chunkSizeWarningLimit: 650,
    rolldownOptions: {
      output: {
        codeSplitting: {
          // Keep three.js in its own long-lived chunk so Zoo-only changes don't invalidate it.
          groups: [{ name: "three", test: /node_modules[\\/]three[\\/]/ }],
        },
      },
    },
  },
})
