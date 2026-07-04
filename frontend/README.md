# apfsRelic frontend

The webview for the apfsRelic desktop GUI ([`crates/gui`](../crates/gui)) — a
**React + [`@carbon/react`](https://carbondesignsystem.com/)** app built with
**Vite** and **TypeScript**. It is intentionally *not* a Cargo crate and lives
outside `crates/`; Tauri embeds its built `dist/` via `frontendDist` in
[`crates/gui/tauri.conf.json`](../crates/gui/tauri.conf.json).

## How it talks to Rust

There is no HTTP server and nothing shells out to the CLI. The UI calls the
Tauri commands in [`crates/gui/src/commands.rs`](../crates/gui/src/commands.rs)
through `@tauri-apps/api` (`invoke`), wrapped in [`src/lib/tauri.ts`](src/lib/tauri.ts):

| Command          | Wrapper            | Purpose                                     |
| ---------------- | ------------------ | ------------------------------------------- |
| `config`         | `getConfig`        | current container/volume selection          |
| `ls`             | `listDir`          | list a directory with per-entry sizes         |
| `stat`           | `statPath`         | full metadata for one entry (details panel) |
| `inspect`        | `inspectImage`     | image/container/volume/snapshot dashboard   |
| `recover`        | `recoverPath`      | recover a file/symlink via a save dialog    |
| `recover_batch`  | `recoverBatch`     | recover files/folders into a chosen folder  |
| `pick_container` | `pickContainer`    | pick a new `.sparsebundle`/image            |
| `open_container` | `openContainer`    | open a known path (start-page recents)      |
| `close_container`| `closeContainer`   | back to the start page                      |
| `set_volume`     | `setVolume`        | switch the 1-based volume index             |
| `recent_images`  | `recentImages`     | recently-opened images for the start page   |
| `remove_recent`  | `removeRecent`     | drop one entry from the recents list        |

`recover_batch` additionally streams `recover-progress` events
(`onRecoverProgress`) while it runs.

The TypeScript shapes in [`src/lib/types.ts`](src/lib/types.ts) mirror the JSON
those commands emit — keep them in sync with `commands.rs`.

## UI structure

- `src/App.tsx` — orchestrates state (config, selection, toasts, refresh,
  batch-recovery progress + report modal); routes start page vs explorer.
- `src/components/AppHeader.tsx` — Carbon UI-Shell header + global actions
  (open, refresh, details-panel toggle, back to start page).
- `src/components/StartPage.tsx` — landing view: open action + recent images.
- `src/components/Toolbar.tsx` — container info, volume `Dropdown` (real volume
  names from `inspect`).
- `src/components/DirTree.tsx` — lazy, directory-only Carbon `TreeView`.
- `src/components/DirContents.tsx` — breadcrumb + Carbon table of the selected
  folder with checkbox multi-select and a "Recover selected" batch bar.
- `src/components/InfoPanel.tsx` — right-hand dashboard: image, container,
  volumes (click to switch), sparsebundle bands, snapshots, selected-item stat.
- `src/components/Toasts.tsx` — bottom-right Carbon `ToastNotification` stack.

## Commands

```sh
pnpm install       # install deps (once)
pnpm dev           # Vite dev server on http://localhost:1420
pnpm build         # type-check (tsc) + bundle into dist/
```

Run the whole desktop app from the repo root:

- `make gui` — builds this bundle and launches the GUI **statically** with the
  assets embedded (`cargo run -p apfsrelic-gui --features custom-protocol`).
- `make gui-dev` — **hot-reload** loop: starts Vite and runs the GUI in dev mode
  so it loads `http://localhost:1420` with live reload.

Tauri chooses which to load at compile time — `tauri::is_dev()` is
`!cfg(custom-protocol)`. A plain `cargo run` (no feature) is "dev" and points at
the Vite dev server; if that server isn't running you get a blank window, which
is why `make gui` passes `--features custom-protocol`.

App/bundle icons live in [`icons/`](icons) and are referenced by
`crates/gui/tauri.conf.json` — they are Tauri bundle assets, not web assets, so
Vite does not process them.
