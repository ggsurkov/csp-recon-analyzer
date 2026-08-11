import { svelte } from '@sveltejs/vite-plugin-svelte';
import tailwindcss from '@tailwindcss/vite';
import type { Plugin } from 'vite';
import { defineConfig } from 'vite';

/**
 * Injects a Content-Security-Policy into the built page.
 *
 * The product claim is that a recon file never leaves the machine. `connect-src 'self'`
 * turns that from a promise into something the browser enforces: no script on this page
 * can reach a third-party host, whatever ends up in the dependency tree. `'wasm-unsafe-eval'`
 * is what lets WebAssembly compile at all.
 *
 * Build only. Vite's dev server needs inline scripts and an HMR socket, and a CSP that
 * fights the dev server just gets switched off — better to ship it where it counts.
 */
function contentSecurityPolicy(): Plugin {
  const policy = [
    "default-src 'self'",
    "script-src 'self' 'wasm-unsafe-eval'",
    "style-src 'self' 'unsafe-inline'",
    "img-src 'self' data:",
    "font-src 'self'",
    "connect-src 'self'",
    "worker-src 'self' blob:",
    "object-src 'none'",
    "base-uri 'none'",
    "form-action 'none'",
    "frame-ancestors 'none'"
  ].join('; ');

  return {
    name: 'reconledger-csp',
    apply: 'build',
    transformIndexHtml(html) {
      return {
        html,
        tags: [
          {
            tag: 'meta',
            attrs: { 'http-equiv': 'Content-Security-Policy', content: policy },
            injectTo: 'head-prepend'
          }
        ]
      };
    }
  };
}

export default defineConfig({
  plugins: [tailwindcss(), svelte(), contentSecurityPolicy()],
  build: {
    target: 'es2022',
    // The .wasm is ~330 KB and must stay a separate fetchable asset, never inlined.
    assetsInlineLimit: 0
  },
  worker: {
    format: 'es'
  }
});
