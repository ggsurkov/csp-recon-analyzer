// Copies the synthetic recon fixture into public/ so the "Try with the demo file" button
// has something to fetch.
//
// It is copied rather than imported because Vite will not serve files from outside the
// project root, and symlinks are a coin flip on Windows. The copy is a build artefact and
// is gitignored; fixtures/ stays the single source of truth.
//
// ## Why the target is named `.bin` and not `.csv.gz`
//
// Static servers — Vite's preview, nginx, S3, Netlify, GitHub Pages — see a `.gz`
// extension and answer with `Content-Encoding: gzip`. The browser then inflates the body
// transparently before `fetch` ever sees it, and **the browser's decoder stops after the
// first gzip member**. Partner Center exports are multi-member, so the app would silently
// receive the first blob only: 5,349 bytes of an 11,246-byte file, roughly half the rows,
// no error anywhere. That is precisely the data-loss bug `MultiGzDecoder` exists to
// prevent, reintroduced one layer below the parser.
//
// An opaque extension keeps the bytes opaque. The parser sniffs magic bytes, so the name
// is irrelevant to it — only to the server.
import { copyFileSync, mkdirSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const source = resolve(here, '../../../fixtures/mock_recon_2026.csv.gz');
const target = resolve(here, '../public/demo/mock_recon_2026.csv.gz.bin');

mkdirSync(dirname(target), { recursive: true });
copyFileSync(source, target);
console.log(`demo fixture -> ${target}`);
