import { defineConfig, loadEnv } from 'vite'
import react from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'
import path from 'node:path'
import process from 'node:process'
import dns from 'node:dns'
import { Buffer } from 'node:buffer'

// Force IPv4 resolution first to prevent dual-stack IPv6 connection timeouts (ETIMEDOUT / ENETUNREACH)
// on systems without a configured IPv6 default route.
if (typeof dns.setDefaultResultOrder === 'function') {
  dns.setDefaultResultOrder('ipv4first')
}

// Resilient NEAR RPC Proxy Plugin with automatic upstream failover
function nearRpcProxyPlugin({ primaryUrl, fallbackUrl, fallbacks = ['https://rpc.testnet.fastnear.com'] }) {
  return {
    name: 'near-rpc-proxy-middleware',
    configureServer(server) {
      server.middlewares.use('/near-rpc', async (req, res, next) => {
        if (req.method === 'OPTIONS') {
          res.writeHead(204, {
            'Access-Control-Allow-Origin': '*',
            'Access-Control-Allow-Methods': 'POST, OPTIONS',
            'Access-Control-Allow-Headers': 'Content-Type',
          })
          return res.end()
        }
        if (req.method !== 'POST') {
          return next()
        }

        const chunks = []
        for await (const chunk of req) {
          chunks.push(chunk)
        }
        const bodyBuffer = Buffer.concat(chunks)

        const endpoints = [...new Set([primaryUrl, ...fallbacks, fallbackUrl].filter(Boolean))]
        let lastError = null

        for (const endpoint of endpoints) {
          try {
            const controller = new AbortController()
            const timer = setTimeout(() => controller.abort(), 6000)
            let response
            try {
              response = await fetch(endpoint, {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: bodyBuffer,
                signal: controller.signal,
              })
            } finally {
              clearTimeout(timer)
            }

            if (response.ok) {
              const data = await response.arrayBuffer()
              res.writeHead(response.status, {
                'Content-Type': response.headers.get('content-type') || 'application/json',
                'Access-Control-Allow-Origin': '*',
              })
              return res.end(Buffer.from(data))
            }

            if (response.status >= 500) {
              console.warn(`[NEAR RPC Proxy] ${endpoint} returned HTTP ${response.status}. Failing over to fallback...`)
              continue
            }

            const data = await response.arrayBuffer()
            res.writeHead(response.status, {
              'Content-Type': response.headers.get('content-type') || 'application/json',
              'Access-Control-Allow-Origin': '*',
            })
            return res.end(Buffer.from(data))
          } catch (err) {
            lastError = err
            console.warn(`[NEAR RPC Proxy] ${endpoint} failed (${err.name}: ${err.message}). Failing over...`)
          }
        }

        console.error('[NEAR RPC Proxy Error] All upstreams failed:', lastError?.message)
        res.writeHead(502, {
          'Content-Type': 'application/json',
          'Access-Control-Allow-Origin': '*',
        })
        res.end(JSON.stringify({
          jsonrpc: '2.0',
          error: {
            code: -32000,
            message: `RPC Proxy Error: All endpoints timed out or failed (${lastError?.message || 'unknown error'})`,
          },
          id: null,
        }))
      })
    },
  }
}

// The NEAR Wallet Selector / near-api-js tree accesses Node's `Buffer`
// global. Vite externalizes `buffer` for browser compatibility, which makes
// the browser bundle throw `Cannot read properties of undefined (reading 'from')`.
// Point the `buffer` module to the real package so Vite bundles it for the browser.
export default defineConfig(({ mode }) => {
  const env = loadEnv(mode, process.cwd(), '')
  const primaryRpc = env.NEAR_RPC_TARGET || env.VITE_NEAR_UPSTREAM_RPC || 'https://test.rpc.fastnear.com'
  const fallbackRpc = 'https://rpc.testnet.near.org'

  return {
    plugins: [
      react(),
      tailwindcss(),
      nearRpcProxyPlugin({ primaryUrl: primaryRpc, fallbackUrl: fallbackRpc }),
    ],
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
  }
})
