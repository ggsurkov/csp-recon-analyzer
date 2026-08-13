/**
 * The synthetic demo export, compiled into the bundle as bytes.
 *
 * `?url&inline` makes Vite read the fixture at build time and hand back a
 * `data:application/gzip;base64,…` string instead of a URL to an asset on disk. The bytes
 * therefore arrive with the JavaScript, and pressing "Try with the demo file" performs no
 * request of any kind — the button works with the network unplugged, the dev server killed,
 * or the page restored from bfcache on a train.
 *
 * ## Why not `fetch('demo/…')`
 *
 * A same-origin fetch is still a network call, and a tool whose entire claim is that it
 * works offline should not have its own demo button be the one thing that needs a server.
 * It also cost us a build step (copy the fixture into `public/`) and a naming hack (`.bin`,
 * because static hosts answer `.gz` with `Content-Encoding: gzip` and browsers stop
 * inflating after the first gzip member — silently halving a multi-blob export). Inlining
 * deletes that whole class of problem: these bytes are never served, so nothing can
 * re-encode them on the way out.
 *
 * ## Why this does not inline the `.wasm` too
 *
 * `vite.config.ts` sets `assetsInlineLimit: 0` to keep the 330 KB analyser a separate,
 * cacheable asset. An explicit `?inline` query is checked before that limit in Vite's
 * `shouldInline`, so this 2 KB fixture opts in without weakening the rule for anything else.
 *
 * The fixture lives at the repository root, outside the Vite root — deliberately, so that
 * `fixtures/` stays the single source of truth shared with the Rust tests.
 */
import demoDataUrl from '../../../../fixtures/mock_recon_2026.csv.gz?url&inline';

/** Name shown in the UI. The extension is honest about what the bytes are. */
export const DEMO_FILE_NAME = 'mock_recon_2026.csv.gz';

const BASE64_MARKER = ';base64,';

/**
 * Decode the embedded fixture into a fresh buffer.
 *
 * Fresh every call, and deliberately not cached: {@link analyzeBytes} *transfers* the
 * buffer to the worker, which detaches it on this side. A shared instance would work once
 * and then hand the analyser an empty buffer on the second press. Decoding 2 KB is
 * microseconds, so there is nothing to gain by holding one.
 */
export function readDemoFile(): ArrayBuffer {
  const marker = demoDataUrl.indexOf(BASE64_MARKER);
  if (!demoDataUrl.startsWith('data:') || marker === -1) {
    // Vite handed back a URL rather than the bytes, which means the import query was
    // changed and the demo button has quietly gone back to needing a server. Fail loudly
    // here rather than at the click, offline, in front of a user.
    throw new Error(
      'The demo file was not inlined into the build. Restore the `?url&inline` import query in src/lib/demo-file.ts.'
    );
  }

  const binary = atob(demoDataUrl.slice(marker + BASE64_MARKER.length));
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i += 1) bytes[i] = binary.charCodeAt(i);
  return bytes.buffer;
}
