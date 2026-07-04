// Typed bridge to the Rust Tauri commands. The old webview reached for the
// `window.__TAURI__` global (`withGlobalTauri`); with a real bundler we import
// `invoke` from `@tauri-apps/api` instead, which is tree-shakeable and typed.

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

import type {
  AppConfig,
  BatchProgress,
  BatchResult,
  InspectResult,
  LsResult,
  PickResult,
  RecentImage,
  RecoverResult,
  StatResult,
} from "./types";

/** Report the current container/volume selection. */
export function getConfig(): Promise<AppConfig> {
  return invoke<AppConfig>("config");
}

/** Open a native file picker and switch the browsed container. */
export function pickContainer(): Promise<PickResult> {
  return invoke<PickResult>("pick_container");
}

/** Open a known path (start page recents) as the browsed container. */
export function openContainer(path: string): Promise<AppConfig> {
  return invoke<AppConfig>("open_container", { path });
}

/** Close the current image and return to the start page. */
export function closeContainer(): Promise<AppConfig> {
  return invoke<AppConfig>("close_container");
}

/** The recently-opened images, most recent first. */
export function recentImages(): Promise<RecentImage[]> {
  return invoke<RecentImage[]>("recent_images");
}

/** Drop one path from the recent list; returns the updated list. */
export function removeRecent(path: string): Promise<RecentImage[]> {
  return invoke<RecentImage[]>("remove_recent", { path });
}

/** Change the 1-based volume index. */
export function setVolume(volume: number): Promise<AppConfig> {
  return invoke<AppConfig>("set_volume", { volume });
}

/** Full image/container/volume/snapshot metadata for the dashboard. */
export function inspectImage(): Promise<InspectResult> {
  return invoke<InspectResult>("inspect");
}

/** All known metadata for one file/dir/symlink (dashboard selection). */
export function statPath(path: string): Promise<StatResult> {
  return invoke<StatResult>("stat", { path });
}

/** Recover one file/symlink via a native save dialog (handled Rust-side). */
export function recoverPath(path: string): Promise<RecoverResult> {
  return invoke<RecoverResult>("recover", { path });
}

/** Recover files/folders into a folder chosen via a native dialog. Progress
 *  streams through {@link onRecoverProgress} while this promise is pending. */
export function recoverBatch(paths: string[]): Promise<BatchResult> {
  return invoke<BatchResult>("recover_batch", { paths });
}

/** Subscribe to batch-recovery progress events. Returns the unlisten fn. */
export function onRecoverProgress(
  handler: (progress: BatchProgress) => void,
): Promise<UnlistenFn> {
  return listen<BatchProgress>("recover-progress", (event) => handler(event.payload));
}

// The tree pane and the contents pane both list the same directory when a
// folder is selected. Cache the in-flight/last promise per path so we issue
// one `ls` instead of two, and so re-expanding a node is instant.
const lsCache = new Map<string, Promise<LsResult>>();

/** List a directory (always with sizes). Results are memoised; a rejected
 *  listing is evicted so a retry re-issues the call. Call {@link clearLsCache}
 *  after any change to the container or volume. */
export function listDir(path: string): Promise<LsResult> {
  let pending = lsCache.get(path);
  if (!pending) {
    pending = invoke<LsResult>("ls", { path, sizes: true }).catch((err) => {
      lsCache.delete(path);
      throw err;
    });
    lsCache.set(path, pending);
  }
  return pending;
}

/** Drop every cached listing (after the container/volume selection changes,
 *  so stale entries are never shown). */
export function clearLsCache(): void {
  lsCache.clear();
}

/** Normalise a thrown Tauri error (string or Error) into a message. */
export function errMessage(err: unknown): string {
  if (typeof err === "string") return err;
  if (err instanceof Error) return err.message;
  if (err && typeof err === "object" && "message" in err) {
    return String((err as { message: unknown }).message);
  }
  return String(err);
}
