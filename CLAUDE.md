# CLAUDE.md — DiskOrbit

Quick context for AI-assisted development. See README.md for user-facing docs.

## Architecture

Three source files, no external config:

| File | Role |
|---|---|
| `src/main.rs` | Entry point — loads the icon, creates the eframe window |
| `src/scanner.rs` | Pure scan logic — `FolderNode` tree, `start_scan`, `fmt_bytes` |
| `src/app.rs` | All GUI code — `DiskOrbitApp` state, egui rendering, OS helpers |

`scanner.rs` has zero GUI dependencies and is independently testable. `app.rs` imports from it via `use crate::scanner::*`.

## Key patterns

**Scan lifecycle:**
1. `do_scan()` creates a fresh `Arc<AtomicBool>` cancel flag and a `mpsc::channel`
2. `start_scan(root, tx, cancel)` spawns a thread; rayon fans out across subdirectories
3. `poll_scan()` drains the channel each frame — `ScanMsg::{Progress, Done, Error}`
4. `done` writes `scan_result: Option<FolderNode>`; the egui tree renders from it

**Tree rendering:**
- `draw_node` takes ownership of `FolderNode`, renders it, and returns it — the caller reassembles `scan_result`
- `expanded: HashSet<String>` tracks expanded paths by `full_path` string
- Column layout constants (`PCT_W`, `SIZE_W`, `BAR_W`, `COL_GAP`, `RIGHT_MARGIN`) are shared between `draw_node` and `show_column_headers` — keep them in sync

**Symlink safety:** `symlink_metadata()` is used throughout; symlinks are explicitly skipped to prevent infinite loops on Windows junctions.

## Platform-conditional code

All Windows-specific features are guarded with `#[cfg(target_os = "windows")]`:

- `is_admin()` — calls `OpenProcessToken` / `GetTokenInformation`
- `drive_usage()` — calls `GetDiskFreeSpaceExW`
- `available_drives()` — iterates `A:\` through `Z:\`; non-Windows returns `vec!["/"]`
- `open_in_explorer()` — spawns `explorer.exe`; non-Windows is a no-op
- `build.rs` — `winresource` embeds the icon into the `.exe`; non-Windows is a no-op

CI runs on Ubuntu; the Linux stubs compile cleanly but have limited runtime functionality.

## Tooling

- `cargo lint` → `cargo fmt --check` (alias in `.cargo/config.toml`)
- `cargo clippy -- -D warnings` is the lint gate in CI
- `just check` runs lint + clippy + test (mirrors CI)
- Releases are triggered by pushing a semver tag (`v1.2.3`) — see `.github/workflows/release.yml`

## Conventions

- Match arms aligned with spaces (not tabs) to the `=>` column — see `fmt_bytes`
- Const names use SCREAMING_SNAKE_CASE for palette/layout values
- `unwrap()` is acceptable inside `#[cfg(test)]` test helpers
- Keep `scanner.rs` free of egui imports — it must remain independently testable
