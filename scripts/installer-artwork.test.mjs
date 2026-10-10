import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';

const root = new URL('../desktop/src-tauri/', import.meta.url);
const config = JSON.parse(readFileSync(new URL('tauri.conf.json', root), 'utf8'));
const nsis = config.bundle.windows.nsis;

test('setup and uninstaller use the current application icon', () => {
  assert.equal(nsis.installerIcon, 'icons/icon.ico');
  assert.equal(nsis.uninstallerIcon, nsis.installerIcon);
  const icon = readFileSync(new URL(nsis.installerIcon, root));
  assert.equal(icon.readUInt16LE(0), 0);
  assert.equal(icon.readUInt16LE(2), 1);
  assert.ok(icon.readUInt16LE(4) > 0);
});

for (const [field, width, height] of [['sidebarImage', 164, 314], ['headerImage', 150, 57]]) {
  test(`${field} is a supported 24-bit native-size NSIS bitmap`, () => {
    const bitmap = readFileSync(new URL(nsis[field], root));
    assert.equal(bitmap.toString('ascii', 0, 2), 'BM');
    assert.equal(bitmap.readUInt32LE(2), bitmap.length);
    assert.equal(bitmap.readInt32LE(18), width);
    assert.equal(bitmap.readInt32LE(22), height);
    assert.equal(bitmap.readUInt16LE(26), 1);
    assert.equal(bitmap.readUInt16LE(28), 24);
    assert.equal(bitmap.readUInt32LE(30), 0);
  });
}
