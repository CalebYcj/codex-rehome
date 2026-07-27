# Portable Codex Thread Fields

## Goal

Make packages created by the updated ReHome Desktop portable between current Windows and macOS Codex thread databases. Existing packages that omit required thread fields are not supported by this change and must be regenerated with the updated app.

## Package data

Extend the existing allowlisted thread metadata export and import with five Codex fields:

- `created_at`
- `source`
- `model_provider`
- `sandbox_policy`
- `approval_mode`

Values are copied from the selected source row without inference or target-specific substitution. SQLite integer, text, null, and JSON-encoded text representations continue to use the existing serialization path.

## Restore behavior

The importer writes only fields that are both present in the authenticated package metadata and supported by the target `threads` table. A newly generated package can therefore satisfy the current Mac schema without overwriting unrelated target-only columns.

If a package omits a target column that is `NOT NULL` and has no database default, restore fails before insertion and the transaction rolls back. ReHome does not invent values and does not derive them from session JSONL.

Existing-row updates continue to preserve target-only fields that are not present in the package.

## Compatibility

- New Windows package to new Mac app: supported.
- New Mac package to new Windows app: supported when both databases expose these standard fields.
- Old package missing the fields: rejected when the target schema requires them.
- Unknown future required columns: rejected safely.

The package schema version remains unchanged because this is an additive expansion of an allowlist; readers already ignore fields absent from their target schema.

## Validation

Add focused tests proving that:

1. Package creation exports all five fields when the source database provides them.
2. SQLite import inserts a missing thread into a target where all five fields are required and have no defaults.
3. The existing test for an unknown required target-only column still fails without changing the database.
4. The previously added SQLite/WAL rollback and restore success tests remain passing.

