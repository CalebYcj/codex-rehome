# Cross-platform plugin conflict handling

## Problem

ReHome Desktop v0.1.6 classifies every existing file whose hash differs from
the package as a conflict. A real Windows-to-Mac package produced 274 blocking
conflicts even though all of them belonged to plugin version directories that
were already installed on the Mac. The conflicting files were split between
`openai-bundled` (113) and `openai-primary-runtime` (161).

Overwriting those files is unsafe because a plugin cache can contain
platform-specific content. Treating each file independently can also create a
mixed plugin installation assembled from Windows and macOS files.

## Desired behavior

Plugin restore decisions are made at the plugin version directory boundary:

- If the target does not contain that plugin version, restore every selected
  file in the version directory normally.
- If the target already contains a valid marker for the same plugin version,
  preserve the complete target version directory and do not copy any package
  files into it.
- If the target path exists but is not a valid plugin version directory, keep
  reporting a conflict.
- Keep existing conflict behavior for projects, skills, images, and other
  Codex content.

This rule gives merge semantics for plugins: installed target versions are
preserved and missing versions are added. It never replaces a macOS plugin
version with bytes taken from a Windows package.

## Planner model

Add a `preserve` change kind to the restore-plan contract. During planning:

1. Discover package plugin version roots from plugin marker payloads under
   `codex/plugins/cache`.
2. Resolve each marker to its version directory using normalized archive
   paths.
3. Inspect the matching target marker without following symbolic links.
4. Mark every package payload under an already-installed valid version root as
   `preserve`.
5. Continue using `add`, `update`, `unchanged`, and `conflict` for all other
   operations.

Preserved operations retain the observed target hash when a target file is
present, but they do not require package and target hashes to match. The whole
version root receives one decision so a restore cannot create a partially
mixed plugin.

## Restore and verification

`preserve` operations are immutable:

- They are excluded from required restore bytes.
- They do not create transaction mutations or backup entries.
- The restore writer never writes them.
- Verification confirms that the preserved plugin marker still exists and
  that no preserved target observed during planning changed before or during
  restore.

The existing sealed-plan and target-change checks remain in force. A target
plugin version that changes after planning causes the restore to stop instead
of silently accepting the new state.

## User interface

The receive-plan table labels `preserve` as `保留本机`. Preserved files do not
increase the conflict badge, so a plan containing only safe plugin
preservations can proceed. Genuine conflicts still disable the restore button
and retain the existing warning.

## Compatibility and safety

The package format is unchanged. Older `.rehome` files remain readable because
`preserve` exists only in the locally generated restore plan. Authentication,
configuration, migration package contents, JSONL sessions, SQLite databases,
and `session_index.jsonl` are not modified by this planning rule.

## Tests

Add coverage for:

- an absent plugin version being added;
- an existing same-version plugin being preserved as one complete unit;
- differing files under a preserved version not becoming conflicts;
- a malformed or non-directory target remaining a conflict;
- preserved operations producing no writes or backup mutations;
- verification detecting a preserved target changed after planning;
- the receive page rendering `保留本机` and enabling restore when no genuine
  conflicts remain;
- existing project and skill conflicts continuing to block restore.

Finally, run the focused frontend and Rust tests, the production frontend
build, and a restore-plan check with the untouched real v0.1.6 Windows package.
