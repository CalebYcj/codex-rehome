import { readFileSync, readdirSync, writeFileSync } from "node:fs";
import { basename, join, resolve } from "node:path";

const [assetsArgument, tag, repository] = process.argv.slice(2);
if (!assetsArgument || !tag || !repository) {
  throw new Error("Usage: node generate-updater-json.mjs <assets-dir> <tag> <owner/repo>");
}

const version = tag.replace(/^desktop-v/, "");
if (!/^\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?$/.test(version)) {
  throw new Error(`Unsupported release tag: ${tag}`);
}

const assetsDirectory = resolve(assetsArgument);
const files = readdirSync(assetsDirectory);
const windowsBundle = findOne(files, (name) => name.endsWith("-setup.exe"), "Windows updater");
const macBundle = findOne(files, (name) => name.endsWith(".app.tar.gz"), "macOS updater");
const windowsSignature = readSignature(assetsDirectory, `${windowsBundle}.sig`);
const macSignature = readSignature(assetsDirectory, `${macBundle}.sig`);
const releaseBase = `https://github.com/${repository}/releases/download/${encodeURIComponent(tag)}`;
const assetUrl = (name) => `${releaseBase}/${encodeURIComponent(basename(name))}`;

const manifest = {
  version,
  notes: `ReHome Desktop ${version}`,
  pub_date: new Date().toISOString(),
  platforms: {
    "windows-x86_64": {
      url: assetUrl(windowsBundle),
      signature: windowsSignature,
    },
    "darwin-x86_64": {
      url: assetUrl(macBundle),
      signature: macSignature,
    },
    "darwin-aarch64": {
      url: assetUrl(macBundle),
      signature: macSignature,
    },
  },
};

writeFileSync(join(assetsDirectory, "latest.json"), `${JSON.stringify(manifest, null, 2)}\n`);

function findOne(items, predicate, label) {
  const matches = items.filter(predicate);
  if (matches.length !== 1) {
    throw new Error(`${label}: expected exactly one file, found ${matches.length}`);
  }
  return matches[0];
}

function readSignature(directory, name) {
  const signature = readFileSync(join(directory, name), "utf8").trim();
  if (!signature) throw new Error(`Empty updater signature: ${name}`);
  return signature;
}
