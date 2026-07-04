import { useEffect, useState } from "react";
import { TreeView, TreeNode } from "@carbon/react";
import { Folder } from "@carbon/icons-react";

import { errMessage, listDir } from "../lib/tauri";
import { joinPath } from "../lib/format";
import type { DirEntry } from "../lib/types";

interface DirTreeProps {
  /** Currently selected directory path (drives highlight + right pane). */
  activePath: string;
  /** Called when a folder node is selected. */
  onSelect: (path: string) => void;
}

/** Lazy, directory-only navigation tree built on Carbon's `TreeView`.
 *
 *  The tree lists only directories: expanding a node fetches that folder and
 *  keeps the sub-directories. Because Carbon only draws an expand caret when a
 *  node has children, every not-yet-loaded folder renders a single placeholder
 *  child so it stays expandable. Listings go through the shared `listDir` cache
 *  (`sizes: false`) so selecting a folder the right pane already loaded is free.
 */
export default function DirTree({ activePath, onSelect }: DirTreeProps) {
  const [roots, setRoots] = useState<DirEntry[] | null>(null);
  const [rootError, setRootError] = useState<string | null>(null);
  const [children, setChildren] = useState<Map<string, DirEntry[]>>(new Map());
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  const [loading, setLoading] = useState<Set<string>>(new Set());
  const [errors, setErrors] = useState<Map<string, string>>(new Map());

  // Load the top-level directories once, when this (re)mounted tree appears.
  useEffect(() => {
    let alive = true;
    (async () => {
      try {
        const res = await listDir("/");
        if (alive) setRoots(res.entries.filter((e) => e.type === "dir"));
      } catch (err) {
        if (alive) setRootError(errMessage(err));
      }
    })();
    return () => {
      alive = false;
    };
  }, []);

  async function loadChildren(path: string) {
    setLoading((prev) => new Set(prev).add(path));
    try {
      const res = await listDir(path);
      const dirs = res.entries.filter((e) => e.type === "dir");
      setChildren((prev) => new Map(prev).set(path, dirs));
    } catch (err) {
      setErrors((prev) => new Map(prev).set(path, errMessage(err)));
    } finally {
      setLoading((prev) => {
        const next = new Set(prev);
        next.delete(path);
        return next;
      });
    }
  }

  function toggle(path: string) {
    const willExpand = !expanded.has(path);
    setExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(path)) next.delete(path);
      else next.add(path);
      return next;
    });
    if (willExpand && !children.has(path) && !loading.has(path)) {
      void loadChildren(path);
    }
  }

  function hint(id: string, text: string) {
    return <TreeNode key={id} id={id} label={<span className="tree-hint">{text}</span>} />;
  }

  function renderDir(entry: DirEntry, path: string) {
    const kids = children.get(path);
    let childNodes;
    if (kids) {
      childNodes = kids.length
        ? kids.map((c) => renderDir(c, joinPath(path, c.name)))
        : [hint(`${path}::empty`, "(no subfolders)")];
    } else if (loading.has(path)) {
      childNodes = [hint(`${path}::loading`, "Loading…")];
    } else if (errors.has(path)) {
      childNodes = [hint(`${path}::error`, errors.get(path) ?? "error")];
    } else {
      // Not yet loaded: an empty placeholder so the expand caret is shown.
      childNodes = [<TreeNode key={`${path}::stub`} id={`${path}::stub`} label="" />];
    }

    return (
      <TreeNode
        key={path}
        id={path}
        value={path}
        isExpanded={expanded.has(path)}
        onToggle={() => toggle(path)}
        renderIcon={Folder}
        label={entry.name}
      >
        {childNodes}
      </TreeNode>
    );
  }

  if (rootError) {
    return <div className="contents-empty tree-hint">Failed to list root: {rootError}</div>;
  }
  if (!roots) {
    return <div className="contents-loading">Loading directories…</div>;
  }

  return (
    <TreeView
      label="Directories"
      hideLabel
      size="xs"
      selected={[activePath]}
      active={activePath}
      onSelect={(_event, node: { value?: unknown }) => {
        const p = node?.value;
        // Ignore placeholder nodes (their ids contain "::").
        if (typeof p === "string" && p.startsWith("/")) onSelect(p);
      }}
    >
      {roots.map((entry) => renderDir(entry, joinPath("/", entry.name)))}
    </TreeView>
  );
}
