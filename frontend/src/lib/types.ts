// Shapes returned by the Rust Tauri commands in `crates/gui/src/commands.rs`.
// These mirror the JSON those commands emit; keep them in sync with that file.

/** `type_name()` from the core engine: a directory, regular file, symlink, or
 *  some other named kind (fifo, socket, …) we render as a generic file. */
export type EntryKind = "dir" | "file" | "symlink" | string;

/** One row of a directory listing. `size` is only present when the caller
 *  passed `sizes: true` and the inode reported a logical size. */
export interface DirEntry {
  name: string;
  type: EntryKind;
  fsoid: string;
  size?: number;
}

/** Result of the `ls` command. */
export interface LsResult {
  path: string;
  fsoid: string;
  entries: DirEntry[];
}

/** Result of the `config` / `open_container` / `set_volume` commands.
 *  `container` is null until an image is opened (the start page state). */
export interface AppConfig {
  container: string | null;
  volume: number;
  container_exists: boolean;
  is_sparsebundle: boolean;
}

/** `pick_container` returns the new config, or `{ cancelled: true }` when the
 *  native dialog was dismissed. */
export type PickResult = AppConfig | { cancelled: true };

/** One entry of the start page's recently-opened list. */
export interface RecentImage {
  path: string;
  /** Unix seconds of the last successful open. */
  last_opened: number;
  exists: boolean;
  is_sparsebundle: boolean;
}

/** Result of the `recover` command. On success `saved` is true; on a cancelled
 *  save dialog `cancelled` is true and `saved` is false. Errors are thrown. */
export interface RecoverResult {
  saved: boolean;
  cancelled?: boolean;
  path?: string;
  symlink?: string;
  bytes?: number;
  complete?: boolean;
}

/** One volume summary in the `inspect` dashboard payload. */
export interface VolumeInfo {
  index: number;
  name: string;
  role: string;
  uuid: string;
  encrypted: boolean;
  sealed: boolean;
  case_insensitive: boolean;
  num_files: number;
  num_directories: number;
  num_symlinks: number;
  num_snapshots: number;
  last_modified?: string | null;
}

/** One snapshot of the selected volume. XIDs are hex strings (u64-safe). */
export interface SnapshotInfo {
  name: string;
  xid: string;
  create_time?: string | null;
}

/** Result of the `inspect` command — everything the dashboard shows. */
export interface InspectResult {
  image: {
    path: string;
    kind: "sparsebundle" | "raw";
    size: number;
    container_offset: number;
    partition_index?: number | null;
  };
  container: {
    uuid: string;
    block_size: number;
    block_count: number;
    total_bytes: number;
    features: string;
    readonly_compatible_features: string;
    incompatible_features: string;
    keylocker: boolean;
    max_file_systems: number;
    checkpoint_xid: string;
    checkpoint_index: number;
  };
  volumes: VolumeInfo[];
  sparsebundle?: {
    band_size: number;
    logical_size: number;
    expected_bands: number;
    present_bands: number;
    missing_band_count: number;
    missing_truncated: boolean;
    short_band_count: number;
    uuid?: string | null;
    backingstore_version?: number | null;
  };
  snapshots?: SnapshotInfo[];
  snapshots_error?: string;
  warnings: string[];
}

/** Inode metadata from the `stat` command. */
export interface StatInode {
  kind: EntryKind;
  mode: string;
  uid: number;
  gid: number;
  nchildren_or_nlink: number;
  sparse: boolean;
  has_rsrc_fork: boolean;
  has_finder_info: boolean;
  internal_flags: string;
  bsd_flags: string;
  size?: number;
  allocated_size?: number;
  create_time?: string;
  mod_time?: string;
  change_time?: string;
  access_time?: string;
}

/** Result of the `stat` command — the "Selected item" dashboard section. */
export interface StatResult {
  path: string;
  fsoid: string;
  inode: StatInode | null;
  recoverability?: {
    status: string;
    reason?: string;
    size?: number;
    has_extents?: boolean;
  };
  symlink_target?: string;
  xattrs: Array<{ name: string; data_len: number }>;
}

/** One per-file outcome of a batch recovery. */
export interface BatchFileResult {
  path: string;
  status: "recovered" | "partial" | "error" | string;
  bytes: number;
  note?: string;
}

/** Result of the `recover_batch` command. */
export interface BatchResult {
  saved: boolean;
  cancelled?: boolean;
  dest?: string;
  files_total?: number;
  bytes_total?: number;
  recovered?: number;
  partial?: number;
  errors?: number;
  results?: BatchFileResult[];
  results_truncated?: boolean;
}

/** Payload of the `recover-progress` event streamed during a batch. */
export interface BatchProgress {
  done: number;
  bytes: number;
  current: string;
}
