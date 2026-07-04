/** Format a byte count as a short human-readable string (matches the legacy
 *  webview's `humanSize`). Returns "" for null/undefined so empty cells stay
 *  blank. Sizes arrive as JSON numbers, per the AGENTS.md `--json` contract. */
export function humanSize(n: number | null | undefined): string {
  if (n == null) return "";
  const units = ["B", "KB", "MB", "GB", "TB", "PB"];
  let value = n;
  let i = 0;
  while (value >= 1024 && i < units.length - 1) {
    value /= 1024;
    i++;
  }
  return `${i === 0 ? value : value.toFixed(1)} ${units[i]}`;
}

/** Join a parent directory path with a child name, keeping a single leading
 *  slash and no doubled separators. */
export function joinPath(parent: string, name: string): string {
  if (parent === "/" || parent === "") return `/${name}`;
  return `${parent.replace(/\/$/, "")}/${name}`;
}

/** The last path component of an image path (recents display name). */
export function baseName(path: string): string {
  const parts = path.split("/").filter(Boolean);
  return parts[parts.length - 1] ?? path;
}

/** Format a Unix-seconds timestamp for the recents list ("3 Jul 2026, 14:02").
 *  Returns "" for 0/undefined so unknown times stay blank. */
export function formatEpoch(secs: number | null | undefined): string {
  if (!secs) return "";
  return new Date(secs * 1000).toLocaleString(undefined, {
    day: "numeric",
    month: "short",
    year: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });
}

/** Trim an ISO-8601 timestamp to a compact "YYYY-MM-DD HH:MM" for dashboards.
 *  Falls back to the raw string when it doesn't look like ISO-8601. */
export function shortIso(iso: string | null | undefined): string {
  if (!iso) return "";
  const m = iso.match(/^(\d{4}-\d{2}-\d{2})T(\d{2}:\d{2})/);
  return m ? `${m[1]} ${m[2]}` : iso;
}

/** Split an absolute path into `{ name, path }` crumbs for a breadcrumb trail.
 *  The root is represented as `{ name: "/", path: "/" }`. */
export function pathCrumbs(path: string): Array<{ name: string; path: string }> {
  const crumbs = [{ name: "/", path: "/" }];
  let acc = "";
  for (const seg of path.split("/").filter(Boolean)) {
    acc = `${acc}/${seg}`;
    crumbs.push({ name: seg, path: acc });
  }
  return crumbs;
}
