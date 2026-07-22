# ReHome Desktop MVP Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a self-contained Windows/macOS desktop application that creates, previews, restores, verifies, and rolls back universal `.rehome` Codex migration packages.

**Architecture:** A Tauri 2 desktop shell presents a React/TypeScript interface and calls a Rust Core through typed commands. Rust owns discovery, package I/O, checksum validation, merge planning, SQLite/index updates, Codex project registration, backups, and rollback; the existing PowerShell/Bash scripts remain supported and serve as regression oracles.

**Tech Stack:** Tauri 2, Rust stable MSVC, React, TypeScript, Vite, Vitest, Testing Library, `serde`, `serde_json`, `zip`, `sha2`, `walkdir`, `rusqlite` with bundled SQLite, `uuid`, `chrono`, `tempfile`, GitHub Actions.

---

## File Structure

```text
desktop/
  package.json                         Frontend scripts and dependencies
  index.html                           Tauri webview entry
  vite.config.ts                       Vite and Vitest configuration
  tsconfig.json                        TypeScript configuration
  src/
    main.tsx                           React entry
    App.tsx                            App navigation and global state
    styles.css                         Responsive application styling
    lib/api.ts                         Typed Tauri command wrapper
    lib/types.ts                       Frontend command/result types
    features/home/HomePage.tsx         Local Codex status and recent handoff
    features/send/SendPage.tsx         Project/chat selection and package creation
    features/receive/ReceivePage.tsx   Package preview, restore, and verification
    features/history/HistoryPage.tsx   Backup history and rollback actions
    test/setup.ts                      Frontend test setup
    App.test.tsx                       Main workflow smoke tests
  src-tauri/
    Cargo.toml                         Rust dependencies
    Cargo.lock                         Reproducible Rust dependency graph
    tauri.conf.json                    Bundle identity and Windows/macOS targets
    capabilities/default.json          Minimal Tauri permissions
    src/
      main.rs                          Native entry
      lib.rs                           Tauri builder and command registration
      commands.rs                      Narrow UI-facing command layer
      core/
        mod.rs                         Core module exports
        error.rs                       Serializable error codes
        models.rs                      Manifest, inventory, plan, and report types
        paths.rs                       OS-neutral paths and source/target mapping
        exclusions.rs                  Mandatory secret/runtime exclusions
        discovery.rs                   Codex and project discovery
        package.rs                     `.rehome` writer/reader/checksum logic
        legacy.rs                      Existing schema v3 ZIP adapter
        planner.rs                     Project/session conflict classification
        backup.rs                      Transaction journal and backup store
        bridge.rs                      Session/index/SQLite/project registration adapter
        restore.rs                     Restore transaction orchestration
    tests/
      fixtures.rs                      Synthetic Codex/project fixture builder
      discovery_test.rs                Standard and custom path detection
      package_test.rs                  Cross-platform package contract tests
      planner_test.rs                  Merge/conflict classification tests
      bridge_test.rs                   Session/index/SQLite/path mapping tests
      restore_test.rs                  Commit, rollback, and recovery tests
      legacy_test.rs                   Schema v3 compatibility tests
.github/workflows/desktop.yml          Test and installer builds
docs/desktop-install.md                Chinese installation and first-open guide
docs/desktop-install.en.md             English installation and first-open guide
```

## Task 1: Install and verify the local build toolchain

**Files:**
- No repository changes

- [ ] **Step 1: Verify the existing Windows prerequisites**

Run:

```powershell
node --version
npm --version
& "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe" -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
```

Expected: Node 24.x, npm 11.x, and `F:\VisualStudio\2022\Community`.

- [ ] **Step 2: Install Rust with the MSVC toolchain**

Run:

```powershell
winget install --id Rustlang.Rustup --exact --accept-package-agreements --accept-source-agreements
```

Expected: successful rustup installation.

- [ ] **Step 3: Open a fresh shell and select stable MSVC**

Run:

```powershell
rustup default stable-msvc
rustc --version
cargo --version
```

Expected: both commands print stable versions and the host contains `pc-windows-msvc`.

## Task 2: Scaffold a minimal Tauri shell

**Files:**
- Create: `desktop/package.json`
- Create: `desktop/index.html`
- Create: `desktop/vite.config.ts`
- Create: `desktop/tsconfig.json`
- Create: `desktop/src/main.tsx`
- Create: `desktop/src/App.tsx`
- Create: `desktop/src/styles.css`
- Create: `desktop/src/test/setup.ts`
- Create: `desktop/src/App.test.tsx`
- Create: `desktop/src-tauri/Cargo.toml`
- Create: `desktop/src-tauri/build.rs`
- Create: `desktop/src-tauri/tauri.conf.json`
- Create: `desktop/src-tauri/capabilities/default.json`
- Create: `desktop/src-tauri/src/main.rs`
- Create: `desktop/src-tauri/src/lib.rs`

- [ ] **Step 1: Generate the official React/TypeScript scaffold**

Run from the repository root:

```powershell
npm create tauri-app@latest desktop -- --template react-ts --manager npm --identifier com.calebycj.rehome
```

Expected: `desktop/package.json` and `desktop/src-tauri/Cargo.toml` exist.

- [ ] **Step 2: Add frontend test and icon dependencies**

Run:

```powershell
Set-Location desktop
npm install lucide-react
npm install --save-dev vitest jsdom @testing-library/react @testing-library/jest-dom
```

Expected: `package-lock.json` records the dependencies.

- [ ] **Step 3: Write the failing shell test**

Create `desktop/src/App.test.tsx`:

```tsx
import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import App from "./App";

describe("ReHome Desktop shell", () => {
  it("shows the two primary handoff actions", () => {
    render(<App />);
    expect(screen.getByRole("button", { name: "发送" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "接收" })).toBeInTheDocument();
  });
});
```

Run: `npm test -- --run`

Expected: FAIL because the product shell has not been implemented.

- [ ] **Step 4: Implement the minimal application shell**

Replace `desktop/src/App.tsx` with a three-view shell using icon buttons for Home, History, and Settings, plus fixed-size `发送` and `接收` command buttons. Keep all visible copy in `desktop/src/App.tsx` for this step and move it into feature components in Task 9.

Required component contract:

```tsx
export type View = "home" | "send" | "receive" | "history";

export default function App() {
  const [view, setView] = useState<View>("home");
  return <main data-view={view}>{/* navigation and active view */}</main>;
}
```

Run: `npm test -- --run`

Expected: PASS.

- [ ] **Step 5: Verify the native shell builds**

Run:

```powershell
npm run build
npm run tauri build -- --debug
```

Expected: frontend build passes and a debug Windows bundle is produced.

- [ ] **Step 6: Commit**

```powershell
git add desktop
git commit -m "feat: scaffold ReHome Desktop shell"
```

## Task 3: Define the Core contract and synthetic fixtures

**Files:**
- Create: `desktop/src-tauri/src/core/mod.rs`
- Create: `desktop/src-tauri/src/core/error.rs`
- Create: `desktop/src-tauri/src/core/models.rs`
- Create: `desktop/src-tauri/tests/fixtures.rs`
- Modify: `desktop/src-tauri/src/lib.rs`

- [ ] **Step 1: Write the failing manifest round-trip test**

Create a test that constructs this exact minimal manifest and round-trips it through `serde_json`:

```rust
let manifest = PackageManifest {
    format: "codex-rehome".into(),
    schema_version: 1,
    package_id: Uuid::nil(),
    created_at: "2026-07-22T00:00:00Z".into(),
    source_os: SourceOs::Windows,
    source_arch: "x86_64".into(),
    source_device_id: Uuid::nil(),
    mode: PackageMode::Full,
    parent_checkpoint: None,
    counts: ContentCounts::default(),
    projects: vec![],
    conversations: vec![],
    exclusions: ExclusionSummary::default(),
};
assert_eq!(serde_json::from_str::<PackageManifest>(&serde_json::to_string(&manifest).unwrap()).unwrap(), manifest);
```

Run: `cargo test manifest_round_trip`

Expected: FAIL because the types do not exist.

- [ ] **Step 2: Implement the serializable domain types**

Define `PackageManifest`, `SourceOs`, `PackageMode`, `ContentCounts`, `ProjectEntry`, `ConversationEntry`, `ExclusionSummary`, `CodexInventory`, `TargetInventory`, `CreatePackageRequest`, `CreatePackageReport`, `PackagePreview`, `RestorePlan`, `RestoreOptions`, `RestoreReport`, `RollbackReport`, `PendingRecovery`, `VerificationReport`, and `RehomeError`. Derive `Debug`, `Clone`, `Serialize`, `Deserialize`, and `PartialEq`; use `snake_case` enum serialization.

Define stable error codes:

```rust
pub enum ErrorCode {
    CodexNotFound,
    PackageInvalid,
    ChecksumMismatch,
    UnsupportedSchema,
    CodexRunning,
    DiskSpaceInsufficient,
    ProjectConflict,
    RestoreFailed,
    RollbackFailed,
    RegistrationIncomplete,
}
```

Run: `cargo test manifest_round_trip`

Expected: PASS.

- [ ] **Step 3: Add reusable fixture builders**

`fixtures.rs` must create a temporary `.codex` tree with one session JSONL, one index entry, one SQLite `threads` row, one Skill, one plugin manifest, one generated image, and one project containing `README.md`, `.env`, `.git`, and `node_modules`. All IDs and timestamps are fixed for deterministic snapshots.

Run: `cargo test`

Expected: PASS.

- [ ] **Step 4: Commit**

```powershell
git add desktop/src-tauri
git commit -m "feat: define ReHome Core package contract"
```

## Task 4: Implement discovery and mandatory exclusions

**Files:**
- Create: `desktop/src-tauri/src/core/paths.rs`
- Create: `desktop/src-tauri/src/core/exclusions.rs`
- Create: `desktop/src-tauri/src/core/discovery.rs`
- Create: `desktop/src-tauri/tests/discovery_test.rs`

- [ ] **Step 1: Write failing path and exclusion tests**

Cover these exact cases:

```rust
assert!(is_forbidden(Path::new("project/.env")));
assert!(is_forbidden(Path::new("project/.env.local")));
assert!(is_forbidden(Path::new("project/.git/config")));
assert!(is_forbidden(Path::new("project/node_modules/a.js")));
assert!(is_forbidden(Path::new("home/.codex/auth.json")));
assert!(!is_forbidden(Path::new("project/src/main.ts")));
assert_eq!(normalize_entry(Path::new(r"projects\visual\README.md")), "projects/visual/README.md");
```

Also test `CODEX_HOME` precedence over the default user profile path.

Run: `cargo test --test discovery_test`

Expected: FAIL.

- [ ] **Step 2: Implement path normalization and exclusions**

Use component-aware matching rather than substring matching. Reject absolute ZIP entries, `..`, device prefixes, alternate data streams, and symbolic links escaping selected roots. The forbidden set must include all entries from the design spec.

Run: `cargo test --test discovery_test`

Expected: PASS.

- [ ] **Step 3: Implement read-only Codex discovery**

`discover_codex(override_home: Option<PathBuf>) -> Result<CodexInventory, RehomeError>` must report paths and counts without changing files. Standard locations:

```rust
// Windows
%CODEX_HOME% or %USERPROFILE%\.codex
// macOS
$CODEX_HOME or $HOME/.codex
```

Read `.codex-global-state.json` and SQLite thread rows to propose project roots. Invalid optional metadata becomes a warning, not a fatal error.

Run: `cargo test`

Expected: PASS.

- [ ] **Step 4: Commit**

```powershell
git add desktop/src-tauri/src/core desktop/src-tauri/tests
git commit -m "feat: discover Codex data safely"
```

## Task 5: Create and inspect universal `.rehome` packages

**Files:**
- Create: `desktop/src-tauri/src/core/package.rs`
- Create: `desktop/src-tauri/tests/package_test.rs`

- [ ] **Step 1: Write failing package contract tests**

The test must package the synthetic fixture and assert:

```rust
assert_eq!(preview.manifest.format, "codex-rehome");
assert_eq!(preview.manifest.schema_version, 1);
assert_eq!(preview.manifest.source_os, SourceOs::Windows);
assert_eq!(preview.forbidden_files_total, 0);
assert!(preview.checksum_valid);
assert!(preview.entries.iter().all(|entry| !entry.contains('\\')));
assert!(!preview.entries.iter().any(|entry| entry.contains("/.git/")));
assert!(!preview.entries.iter().any(|entry| entry.ends_with("/.env")));
```

Add corruption, ZIP traversal, missing manifest, and unsupported schema tests.

Run: `cargo test --test package_test`

Expected: FAIL.

- [ ] **Step 2: Implement the staging writer**

Implement:

```rust
pub fn create_package(request: CreatePackageRequest) -> Result<CreatePackageReport, RehomeError>;
pub fn inspect_package(path: &Path) -> Result<PackagePreview, RehomeError>;
```

Write files to a private temporary staging directory, calculate SHA-256 for every payload entry, write LF UTF-8 `checksums.sha256`, write `manifest.json` last, and atomically rename the finished archive to the chosen `.rehome` path.

- [ ] **Step 3: Enforce deterministic, portable ZIP metadata**

Sort entries lexicographically, use `/`, preserve regular-file executable bits only where needed, give directories user-readable/traversable permissions, and reject source files that change size or mtime while being copied.

Run: `cargo test --test package_test`

Expected: PASS for valid packages and the expected error code for each negative case.

- [ ] **Step 4: Compare with existing script invariants**

Run:

```powershell
powershell.exe -NoProfile -ExecutionPolicy Bypass -File tests/redskill_package_test.ps1
powershell.exe -NoProfile -ExecutionPolicy Bypass -File tests/windows_restore_ui_ready_test.ps1
```

Expected: existing tests still pass.

- [ ] **Step 5: Commit**

```powershell
git add desktop/src-tauri
git commit -m "feat: create portable rehome packages"
```

## Task 6: Plan project and conversation merges

**Files:**
- Create: `desktop/src-tauri/src/core/planner.rs`
- Create: `desktop/src-tauri/tests/planner_test.rs`

- [ ] **Step 1: Write failing classification tests**

Use table tests for:

```rust
TargetMissing => ChangeKind::Add
SameHash => ChangeKind::Unchanged
TargetPresentWithoutBaseline => ChangeKind::Conflict
ExistingSessionSameIdSameHash => SessionAction::Skip
ExistingSessionSameIdDifferentHash => SessionAction::ImportAsBranch
NewSessionId => SessionAction::Import
```

Run: `cargo test --test planner_test`

Expected: FAIL.

- [ ] **Step 2: Implement a side-effect-free restore planner**

Implement:

```rust
pub fn build_restore_plan(
    package: &PackagePreview,
    target: &TargetInventory,
    projects_root: &Path,
) -> Result<RestorePlan, RehomeError>;
```

Every write operation must appear in the plan with source, target, expected previous hash, action, and rollback requirement. The planner must never silently choose a side for a project conflict.

- [ ] **Step 3: Implement conversation branch IDs**

For an ID collision with different content, derive a deterministic imported ID from package ID plus source task ID, retain the original title with ` · ReHome`, and rewrite all package-local references to the derived ID.

Run: `cargo test --test planner_test`

Expected: PASS.

- [ ] **Step 4: Commit**

```powershell
git add desktop/src-tauri/src/core desktop/src-tauri/tests
git commit -m "feat: plan safe ReHome merges"
```

## Task 7: Implement Codex Bridge

**Files:**
- Create: `desktop/src-tauri/src/core/bridge.rs`
- Create: `desktop/src-tauri/tests/bridge_test.rs`

- [ ] **Step 1: Write failing index and SQLite tests**

Restore a synthetic Windows session into a synthetic macOS target and assert:

```rust
assert!(restored_jsonl.contains("/Users/test/Documents/Codex-Restored-Projects/visual"));
assert!(!restored_jsonl.contains(r"C:\Users\OldUser\Documents\visual"));
assert_eq!(index_entries_for(&target_index, THREAD_ID), 1);
assert_eq!(sqlite_thread(&state_db, THREAD_ID).cwd, target_project);
assert_eq!(sqlite_thread(&state_db, THREAD_ID).rollout_path, target_session);
```

Run: `cargo test --test bridge_test`

Expected: FAIL.

- [ ] **Step 2: Implement JSONL and session index merging**

Parse every JSONL line as JSON and recursively rewrite exact source path variants. Do not use blind global string replacement. Merge index entries by ID, preserving target entries and choosing the newer metadata only for imported IDs.

- [ ] **Step 3: Implement minimal SQLite thread import**

Inspect `PRAGMA table_info(threads)` at runtime, update only columns present in the target schema, preserve target-only columns, and transact all rows. Never replace state, memory, or goal databases.

- [ ] **Step 4: Implement platform project registration commands**

Return a structured registration result:

```rust
pub enum RegistrationStatus {
    Registered,
    CommandUnavailable,
    InvocationFailed { message: String },
    ManualOpenRequired,
}
```

On macOS prefer the detected application CLI with `app <path>`. On Windows try the detected `codex app <path>` entry and otherwise return `ManualOpenRequired`. Keep registration failure distinct from file restore failure.

Run: `cargo test --test bridge_test`

Expected: PASS using a fake command runner; tests must not launch the real Codex app.

- [ ] **Step 5: Commit**

```powershell
git add desktop/src-tauri/src/core desktop/src-tauri/tests
git commit -m "feat: restore Codex indexes and project registration"
```

## Task 8: Add transactional restore, verification, and rollback

**Files:**
- Create: `desktop/src-tauri/src/core/backup.rs`
- Create: `desktop/src-tauri/src/core/restore.rs`
- Create: `desktop/src-tauri/tests/restore_test.rs`

- [ ] **Step 1: Write failing transaction tests**

Cover successful commit, injected failure after project copy, injected failure during SQLite update, app restart with an incomplete journal, and user-requested rollback. Assert that the pre-restore file hashes return exactly after rollback.

Run: `cargo test --test restore_test`

Expected: FAIL.

- [ ] **Step 2: Implement the transaction journal**

Store one JSON journal per restore under the ReHome application data directory:

```json
{
  "transaction_id": "uuid",
  "package_id": "uuid",
  "status": "prepared",
  "created_at": "RFC3339",
  "operations": [],
  "backup_root": "absolute local path"
}
```

Allowed states are `prepared`, `applying`, `verifying`, `committed`, `rolling_back`, `rolled_back`, and `rollback_failed`. Journal writes use temp-file plus rename.

- [ ] **Step 3: Implement restore orchestration**

Implement:

```rust
pub fn apply_restore(plan: RestorePlan, options: RestoreOptions) -> Result<RestoreReport, RehomeError>;
pub fn rollback(transaction_id: Uuid) -> Result<RollbackReport, RehomeError>;
pub fn recover_incomplete_transactions() -> Result<Vec<PendingRecovery>, RehomeError>;
```

Refuse to start while Codex is running unless the UI passes explicit confirmation. Back up every mutable target before the first write. Restore files, bridge metadata, verify, then commit.

- [ ] **Step 4: Implement layered verification**

Report separate booleans for package checksum, files, sessions, session index, SQLite threads, path mapping, forbidden files, project files, and app registration. `app_visible_ready` is true only when every data/index check passes and registration is `Registered`.

Run: `cargo test --test restore_test`

Expected: PASS.

- [ ] **Step 5: Commit**

```powershell
git add desktop/src-tauri/src/core desktop/src-tauri/tests
git commit -m "feat: restore and roll back ReHome transactions"
```

## Task 9: Expose typed commands and build the complete UI

**Files:**
- Create: `desktop/src-tauri/src/commands.rs`
- Modify: `desktop/src-tauri/src/lib.rs`
- Create: `desktop/src/lib/api.ts`
- Create: `desktop/src/lib/types.ts`
- Create: `desktop/src/features/home/HomePage.tsx`
- Create: `desktop/src/features/send/SendPage.tsx`
- Create: `desktop/src/features/receive/ReceivePage.tsx`
- Create: `desktop/src/features/history/HistoryPage.tsx`
- Modify: `desktop/src/App.tsx`
- Modify: `desktop/src/App.test.tsx`
- Modify: `desktop/src/styles.css`

- [ ] **Step 1: Write failing frontend workflow tests**

Mock `desktop/src/lib/api.ts` and test:

- detected Codex home and counts appear on Home;
- Send requires a project and at least one conversation/content selection;
- Receive shows package source OS, counts, checksum, conflicts, and target folder before enabling restore;
- registration failure uses the exact status `项目文件已恢复，需要在 Codex 中手动打开`;
- History can request rollback only for a committed transaction.

Run: `npm test -- --run`

Expected: FAIL.

- [ ] **Step 2: Add a narrow command API**

Expose only:

```rust
discover_codex
create_package
inspect_package
build_restore_plan
apply_restore
list_transactions
rollback_transaction
open_path
open_restored_thread
```

Commands return serializable result types and stable error codes. Do not expose arbitrary shell execution or unrestricted filesystem commands.

- [ ] **Step 3: Implement the four screens**

Use a quiet operational layout with a 240px sidebar, compact status rows, native file pickers, checkboxes for included chats, a conflict table, progress steps, and a verification result list. Use Lucide icons for navigation, folder selection, reveal, rollback, and refresh. Cards may frame repeated transaction rows but must not nest cards or turn page sections into floating cards.

- [ ] **Step 4: Add responsive constraints and accessibility**

At 1024x720 and above, keep the sidebar and main workflow visible. Below 760px, collapse navigation to an icon rail and stack form rows. All buttons require accessible names; progress and error status use text plus icon, not color alone.

Run:

```powershell
npm test -- --run
npm run build
cargo test --manifest-path src-tauri/Cargo.toml
```

Expected: PASS.

- [ ] **Step 5: Start the app and verify visually**

Run: `npm run tauri dev`

Capture Home, Send selection, Receive preview, conflict, success, and partial-registration states at 1440x900 and 1024x720. Verify no blank view, overlap, clipped text, or layout shift.

- [ ] **Step 6: Commit**

```powershell
git add desktop
git commit -m "feat: add ReHome Desktop workflows"
```

## Task 10: Import existing schema v3 packages

**Files:**
- Create: `desktop/src-tauri/src/core/legacy.rs`
- Create: `desktop/src-tauri/tests/legacy_test.rs`
- Modify: `desktop/src-tauri/src/core/package.rs`

- [ ] **Step 1: Build a synthetic schema v3 fixture and failing test**

The fixture must use the existing neutral layout: `MANIFEST.txt`, `MANIFEST.json`, `SHA256SUMS.txt`, `home/.codex`, `projects`, `selected_chats`, and `metadata`. Assert that inspection returns the same `PackagePreview` type used for `.rehome`.

Run: `cargo test --test legacy_test`

Expected: FAIL.

- [ ] **Step 2: Implement a read-only legacy adapter**

Map schema v3 fields into schema 1 in memory. Preserve original checksums and metadata; do not rewrite the source archive. Reject BOM/CRLF only when direct checksum verification actually fails, and return a repairable legacy error instead of modifying the archive silently.

- [ ] **Step 3: Run all legacy script tests**

Run:

```powershell
powershell.exe -NoProfile -ExecutionPolicy Bypass -File tests/windows_restore_ui_ready_test.ps1
powershell.exe -NoProfile -ExecutionPolicy Bypass -File tests/redskill_package_test.ps1
powershell.exe -NoProfile -ExecutionPolicy Bypass -File tests/readme_language_test.ps1
```

Run macOS script syntax checks in CI in Task 11.

Expected: PASS.

- [ ] **Step 4: Commit**

```powershell
git add desktop/src-tauri
git commit -m "feat: import ReHome schema v3 packages"
```

## Task 11: Build installers, CI, and installation documentation

**Files:**
- Create: `.github/workflows/desktop.yml`
- Create: `docs/desktop-install.md`
- Create: `docs/desktop-install.en.md`
- Modify: `README.md`
- Modify: `README.en.md`
- Modify: `desktop/src-tauri/tauri.conf.json`

- [ ] **Step 1: Add failing README contract assertions**

Extend `tests/readme_language_test.ps1` to require both READMEs to link to the desktop install guide and GitHub Releases, and to state that Core and Bridge are bundled.

Run: `powershell.exe -NoProfile -ExecutionPolicy Bypass -File tests/readme_language_test.ps1`

Expected: FAIL.

- [ ] **Step 2: Configure application bundles**

Set product name `ReHome Desktop`, identifier `com.calebycj.rehome`, per-user NSIS install mode, DMG target, minimum macOS 12, and bundled icons. Do not enable updater, autostart, shell plugin, network capabilities, or admin elevation.

- [ ] **Step 3: Add GitHub Actions**

The workflow must:

1. run Rust and frontend tests on Windows and macOS;
2. run existing PowerShell tests on Windows;
3. run `bash -n` and existing shell tests on macOS;
4. build Windows x64 NSIS;
5. build macOS x86_64 and aarch64 artifacts;
6. combine the two macOS app binaries into a Universal app before producing the DMG;
7. upload installers as workflow artifacts;
8. attach installers to a GitHub Release only for a `desktop-v*` tag.

- [ ] **Step 4: Write concise installation guides**

Chinese first, English second. Document:

- Windows EXE installation;
- macOS DMG drag-to-Applications installation;
- unsigned first-open steps through Privacy & Security;
- automatic Codex path detection and custom path fallback;
- `.rehome` files versus installer files;
- exact system impact and excluded secrets;
- Core and Bridge require no separate installation.

- [ ] **Step 5: Run the complete verification suite**

Run:

```powershell
Set-Location desktop
npm test -- --run
npm run build
cargo test --manifest-path src-tauri/Cargo.toml
Set-Location ..
powershell.exe -NoProfile -ExecutionPolicy Bypass -File tests/windows_restore_ui_ready_test.ps1
powershell.exe -NoProfile -ExecutionPolicy Bypass -File tests/redskill_package_test.ps1
powershell.exe -NoProfile -ExecutionPolicy Bypass -File tests/readme_language_test.ps1
git diff --check
```

Expected: all commands pass.

- [ ] **Step 6: Build the local Windows installer**

Run:

```powershell
Set-Location desktop
npm run tauri build
```

Expected: `desktop/src-tauri/target/release/bundle/nsis/ReHome Desktop_*_x64-setup.exe` exists and launches without administrator elevation.

- [ ] **Step 7: Commit**

```powershell
git add .github desktop docs README.md README.en.md tests/readme_language_test.ps1
git commit -m "build: package ReHome Desktop installers"
```

## Final Acceptance

- [ ] Create a `.rehome` file from a synthetic and a consented real Windows Codex project.
- [ ] Inspect the file without changing target data.
- [ ] Restore into an isolated Windows profile and verify sessions, indexes, SQLite, project files, exclusions, and rollback.
- [ ] Build and install the Windows EXE on the current machine.
- [ ] Send the macOS artifact and acceptance instructions to the available Intel Mac for x86_64 validation.
- [ ] Run the same package through macOS receive, project registration, verification, and rollback.
- [ ] Confirm the Universal DMG launches on Apple Silicon through CI artifact validation or a real Apple Silicon test machine before labeling it fully validated.
- [ ] Publish a prerelease in GitHub Releases with both installers and SHA-256 files.
