// Copies the manifest into dist and rewrites the popup path produced by Vite.
import { copyFile, readFile, writeFile, mkdir, rename, access, rm } from 'node:fs/promises';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const dist = resolve(root, process.env.LOCALTRACK_DIST ?? 'dist');

// Firefox runs the same code with an event-page background and a Gecko add-on
// id, so the two manifests differ only in those details.
const target = process.env.LOCALTRACK_BROWSER === 'firefox' ? 'firefox' : 'chrome';
const manifestFile = target === 'firefox' ? 'manifest.firefox.json' : 'manifest.json';
const manifest = JSON.parse(await readFile(resolve(root, manifestFile), 'utf8'));

// Vite emits the popup at dist/src/popup/index.html; move it to dist/popup.html.
const emitted = resolve(dist, 'src/popup/index.html');
try {
  await access(emitted);
  await mkdir(dist, { recursive: true });
  await rename(emitted, resolve(dist, 'popup.html'));
  manifest.action.default_popup = 'popup.html';
} catch {
  manifest.action.default_popup = 'popup.html';
}

// Extension pages resolve absolute paths against the extension root, which is
// exactly what Vite emits, so only the now-empty source folder needs removing.
await rm(resolve(dist, 'src'), { recursive: true, force: true });

await writeFile(resolve(dist, 'manifest.json'), `${JSON.stringify(manifest, null, 2)}\n`);
await copyFile(resolve(root, '../../PRIVACY.md'), resolve(dist, 'PRIVACY.md')).catch(() => {});

console.log(`extension packaged in dist/ for ${target}`);
