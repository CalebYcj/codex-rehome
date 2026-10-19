# ReHome brand assets

`brand-mark.svg` is the editable master: charcoal house, blue door and left-pointing return arrow. It intentionally has no surrounding tile. Do not replace it with a font glyph or reintroduce the old R/doorway mark.

The app header consumes the master directly. `app-icon.svg` adds the cream platform tile and padding. Both GitHub themes and the web favicon use this tile for contrast.

Regenerate platform assets after changing the master:

```sh
node scripts/generate-brand-assets.mjs
```

The command needs Node and installed `desktop` npm dependencies. It generates the Windows ICO, macOS ICNS, configured 32/128/256px PNGs, transparent installer mark PNG, favicon and documentation SVG. ICNS chunks are sorted for reproducible output. Unsupported mobile/Store icons are not checked in.

On Windows, regenerate the native-size 24-bit NSIS artwork as well:

```powershell
./scripts/generate-installer-artwork.ps1
```

`tauri.conf.json` references these assets for app packaging, setup and uninstaller. The runtime-default icon regression checks the compiled Tauri context, not just the EXE file icon. Existing installed applications and pinned shell caches are not modified by asset generation; users receive new resources through a new app build/update.

Windows CI also reads the built application's and setup executable's icons and compares them with the current ICO using `scripts/verify-windows-brand.ps1`, without launching either program.

Checks:

```sh
node --test scripts/brand-assets.test.mjs scripts/installer-artwork.test.mjs
```
