import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';
const read = (path) => readFileSync(new URL(`../${path}`, import.meta.url));
const text = (path) => read(path).toString('utf8');

test('house and return-arrow master is used by UI, native icons and installer', () => {
  const mark = text('desktop/brand-mark.svg');
  const shapes = mark.replace(/^<svg[^>]*>\s*/, '').replace(/\s*<\/svg>\s*$/, '');
  const icon = text('desktop/app-icon.svg');
  assert.ok(icon.includes(shapes));
  assert.ok(mark.includes('#1d1b17'));
  assert.ok(mark.includes('#2f5bd3'));
  assert.ok(icon.includes('#fbf8f1'));
  assert.ok(!mark.includes('<rect x="64"'), 'bare mark has no surrounding app tile');
  const app = text('desktop/src/App.tsx');
  assert.ok(app.includes('../brand-mark.svg'));
  assert.ok(!app.includes('M13 24H5V5h15v8'));
  const installer = text('scripts/generate-installer-artwork.ps1');
  assert.ok(installer.includes('../desktop/brand-mark.png'));
  assert.ok(!installer.includes('interlocking doorways'));
});

test('web favicon and documentation use the same cream-tile icon in both themes', () => {
  assert.equal(text('desktop/public/favicon.svg'), text('desktop/app-icon.svg'));
  assert.equal(text('docs/assets/rehome-mark.svg'), text('desktop/app-icon.svg'));
  assert.match(text('desktop/index.html'), /rel="icon"[^>]*href="\/favicon.svg"/);
  for (const file of ['README.md', 'README.en.md', 'docs/index.md']) assert.match(text(file), /rehome-mark\.svg/);
});

test('all shipped PNGs have exact dimensions and alpha channels', () => {
  for (const [name, size] of [['32x32.png',32],['128x128.png',128],['128x128@2x.png',256],['../../brand-mark.png',512]]) {
    const png = read(`desktop/src-tauri/icons/${name}`);
    assert.equal(png.toString('hex',0,8),'89504e470d0a1a0a');
    assert.equal(png.readUInt32BE(16),size);
    assert.equal(png.readUInt32BE(20),size);
    assert.equal(png[25],6,'PNG must retain transparency');
  }
});

test('Windows icon includes taskbar sizes and macOS container is complete', () => {
  const ico = read('desktop/src-tauri/icons/icon.ico');
  assert.equal(ico.readUInt16LE(2),1);
  const sizes=[];
  for(let i=0;i<ico.readUInt16LE(4);i++) {
    const at=6+i*16; sizes.push(ico[at]||256);
    assert.ok(ico.readUInt32LE(at+12)+ico.readUInt32LE(at+8)<=ico.length);
  }
  for(const size of [16,32,48,256]) assert.ok(sizes.includes(size),`missing ${size}px Windows icon`);
  const icns = read('desktop/src-tauri/icons/icon.icns');
  assert.equal(icns.toString('ascii',0,4),'icns');
  assert.equal(icns.readUInt32BE(4),icns.length);
  let at=8; const types=[];
  while(at<icns.length) {
    types.push(icns.toString('ascii',at,at+4));
    const length=icns.readUInt32BE(at+4); assert.ok(length>=8); at+=length;
  }
  assert.equal(at,icns.length);
  assert.deepEqual(types,[...types].sort(),'ICNS chunk order must be reproducible');
  assert.ok(types.includes('ic10'),'macOS needs its 1024px icon');
});
