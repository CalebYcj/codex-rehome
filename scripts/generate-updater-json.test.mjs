import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { spawnSync } from "node:child_process";
import test from "node:test";

test("generates one manifest for Windows and both Universal macOS architectures", () => {
  const directory = mkdtempSync(join(tmpdir(), "rehome-updater-manifest-"));
  try {
    const appVersion = JSON.parse(readFileSync(resolve("desktop/package.json"), "utf8")).version;
    const windows = `ReHome Desktop_${appVersion}_x64-setup.exe`;
    const mac = "ReHome Desktop.app.tar.gz";
    writeFileSync(join(directory, windows), "windows");
    writeFileSync(join(directory, `${windows}.sig`), "windows-signature\n");
    writeFileSync(join(directory, mac), "macos");
    writeFileSync(join(directory, `${mac}.sig`), "mac-signature\n");

    const script = resolve("scripts/generate-updater-json.mjs");
    const result = spawnSync(
      process.execPath,
      [script, directory, `desktop-v${appVersion}`, "CalebYcj/codex-rehome"],
      { encoding: "utf8" },
    );
    assert.equal(result.status, 0, result.stderr);

    const manifest = JSON.parse(readFileSync(join(directory, "latest.json"), "utf8"));
    assert.equal(manifest.version, appVersion);
    assert.equal(manifest.platforms["windows-x86_64"].signature, "windows-signature");
    assert.equal(manifest.platforms["darwin-x86_64"].signature, "mac-signature");
    assert.deepEqual(
      manifest.platforms["darwin-aarch64"],
      manifest.platforms["darwin-x86_64"],
    );
    assert.match(manifest.platforms["windows-x86_64"].url, /ReHome\.Desktop/);
    assert.doesNotMatch(manifest.platforms["windows-x86_64"].url, /%20/);
    assert.match(manifest.platforms["darwin-x86_64"].url, /ReHome\.Desktop\.app\.tar\.gz$/);
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});

test("rejects a release tag that does not match the app version", () => {
  const directory = mkdtempSync(join(tmpdir(), "rehome-updater-version-"));
  try {
    const script = resolve("scripts/generate-updater-json.mjs");
    const result = spawnSync(
      process.execPath,
      [script, directory, "desktop-v99.0.0", "CalebYcj/codex-rehome"],
      { encoding: "utf8" },
    );

    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /does not match package version/);
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});
