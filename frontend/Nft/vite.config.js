import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'
import path from 'node:path'

// The NEAR Wallet Selector / near-api-js tree accesses Node's `Buffer`
// global. Vite externalizes `buffer` for browser compatibility, which makes
// the browser bundle throw `Cannot read properties of undefined (reading 'from')`.
// Point the `buffer` module to the real package so Vite bundles it for the browser.
export default defineConfig({
  plugins: [react(), tailwindcss()],
  define: {
    global: 'globalThis',
  },
  resolve: {
    alias: {
      buffer: path.resolve(import.meta.dirname, 'node_modules/buffer/index.js'),
    },
  },
  optimizeDeps: {
    // Pre-bundle `buffer` in dev so the browser polyfill is available immediately.
    include: ['buffer'],
  },
  server: {
    host: true,
    port: 5173,
  },
})
