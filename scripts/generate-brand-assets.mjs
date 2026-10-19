// Single vector source for the in-app mark, desktop icons and installer artwork.
// Run from any directory with Node and the checked-in desktop npm dependencies.
import { readFileSync, writeFileSync, mkdirSync, mkdtempSync, copyFileSync, rmSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { tmpdir } from 'node:os';
import { createRequire } from 'node:module';
import { spawnSync } from 'node:child_process';

const root = fileURLToPath(new URL('../', import.meta.url));
const desktop = join(root, 'desktop');
const require = createRequire(join(desktop, 'package.json'));
const cli = require.resolve('@tauri-apps/cli/tauri.js');
const mark = readFileSync(join(desktop, 'brand-mark.svg'), 'utf8');
const shapes = mark.replace(/^<svg[^>]*>\s*/, '').replace(/\s*<\/svg>\s*$/, '');
const icon = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1024 1024" width="1024" height="1024">
  <rect x="64" y="64" width="896" height="896" rx="208" fill="#fbf8f1"/>
  <svg x="176" y="176" width="672" height="672" viewBox="240 240 544 544">
${shapes}
  </svg>
</svg>\n`;
writeFileSync(join(desktop, 'app-icon.svg'), icon);
const scratch = mkdtempSync(join(tmpdir(), 'rehome-brand-'));
function generate(input, output, extra = []) {
  const result = spawnSync(process.execPath, [cli, 'icon', input, '--output', output, ...extra], { cwd: desktop, stdio: 'inherit' });
  if (result.status !== 0) throw new Error('Tauri icon generation failed');
}
try {
  const icons = join(desktop, 'src-tauri/icons');
  generate(join(desktop, 'app-icon.svg'), join(scratch, 'icons'));
  for (const name of ['32x32.png', '128x128.png', '128x128@2x.png', 'icon.ico', 'icon.icns']) {
    copyFileSync(join(scratch, 'icons', name), join(icons, name));
  }
  // ICNS chunk order can vary with the encoder's map iteration order.
  const icnsPath = join(icons, 'icon.icns');
  const icns = readFileSync(icnsPath);
  const chunks = [];
  for (let at = 8; at < icns.length;) {
    const length = icns.readUInt32BE(at + 4);
    if (length < 8 || at + length > icns.length) throw new Error('Invalid generated ICNS chunk');
    chunks.push(icns.subarray(at, at + length));
    at += length;
  }
  chunks.sort((a,b) => Buffer.compare(a.subarray(0,4),b.subarray(0,4)));
  writeFileSync(icnsPath,Buffer.concat([icns.subarray(0,8),...chunks]));
  generate(join(desktop, 'brand-mark.svg'), join(scratch, 'mark'), ['--png', '512']);
  copyFileSync(join(scratch, 'mark/512x512.png'), join(desktop, 'brand-mark.png'));
  const publicDir = join(desktop, 'public');
  const docsAssets = join(root, 'docs/assets');
  mkdirSync(publicDir, { recursive: true });
  mkdirSync(docsAssets, { recursive: true });
  copyFileSync(join(desktop, 'app-icon.svg'), join(publicDir, 'favicon.svg'));
  // The cream app tile keeps the dark mark readable in both GitHub themes.
  copyFileSync(join(desktop, 'app-icon.svg'), join(docsAssets, 'rehome-mark.svg'));
  console.log('Updated desktop platform icons, transparent mark PNG, favicon and documentation mark.');
} finally {
  // Only the exact mkdtemp-owned scratch directory is disposable.
  if (resolve(scratch).startsWith(resolve(tmpdir()) + (process.platform === 'win32' ? '\\' : '/'))) rmSync(scratch, { recursive: true });
}
