import { useEffect, useMemo, useState } from "react";
import {
  Breadcrumb,
  BreadcrumbItem,
  Button,
  DataTableSkeleton,
  InlineNotification,
  Table,
  TableBatchAction,
  TableBatchActions,
  TableBody,
  TableCell,
  TableContainer,
  TableHead,
  TableHeader,
  TableRow,
  TableSelectAll,
  TableSelectRow,
  TableToolbar,
  Tag,
} from "@carbon/react";
import { Document, Download, Folder, Link } from "@carbon/icons-react";

import { errMessage, listDir } from "../lib/tauri";
import { humanSize, joinPath, pathCrumbs } from "../lib/format";
import type { LsResult } from "../lib/types";

interface DirContentsProps {
  path: string;
  /** Bumped by the parent to force a refetch (e.g. the Refresh action). */
  reloadNonce: number;
  /** Path whose metadata the details panel is currently showing. */
  detailPath: string | null;
  onNavigate: (path: string) => void;
  /** Save-as recovery of a single file/symlink. */
  onRecover: (path: string) => void;
  /** Recover several entries (files and/or folders) into a chosen folder. */
  onRecoverBatch: (paths: string[]) => void;
  /** Show an entry's metadata in the details panel. */
  onShowDetails: (path: string) => void;
}

function kindIcon(type: string) {
  if (type === "dir") return <Folder size={16} />;
  if (type === "symlink") return <Link size={16} />;
  return <Document size={16} />;
}

/** Right pane: a breadcrumb plus a Carbon table of the selected directory's
 *  entries. Clicking a row shows its metadata in the details panel (folder
 *  names additionally navigate); every row is checkbox-selectable and the
 *  batch bar recovers the whole selection (folders recurse) in one go. */
export default function DirContents({
  path,
  reloadNonce,
  detailPath,
  onNavigate,
  onRecover,
  onRecoverBatch,
  onShowDetails,
}: DirContentsProps) {
  const [data, setData] = useState<LsResult | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(true);
  // Full paths of the checked rows. Selection is per-directory: navigating
  // away (or refreshing) clears it, so a batch always acts on visible rows.
  const [selected, setSelected] = useState<Set<string>>(new Set());

  useEffect(() => {
    let alive = true;
    setBusy(true);
    setError(null);
    setSelected(new Set());
    listDir(path)
      .then((res) => {
        if (alive) setData(res);
      })
      .catch((err) => {
        if (alive) {
          setError(errMessage(err));
          setData(null);
        }
      })
      .finally(() => {
        if (alive) setBusy(false);
      });
    return () => {
      alive = false;
    };
  }, [path, reloadNonce]);

  const crumbs = pathCrumbs(path);
  const allPaths = useMemo(
    () => (data ? data.entries.map((e) => joinPath(path, e.name)) : []),
    [data, path],
  );
  const allSelected = allPaths.length > 0 && selected.size === allPaths.length;
  const someSelected = selected.size > 0 && !allSelected;

  const toggleAll = () => {
    setSelected(allSelected ? new Set() : new Set(allPaths));
  };
  const toggleOne = (full: string) => {
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(full)) next.delete(full);
      else next.add(full);
      return next;
    });
  };

  return (
    <div className="pane-contents">
      <div className="contents-head">
        <Breadcrumb noTrailingSlash>
          {crumbs.map((c, i) => {
            const isLast = i === crumbs.length - 1;
            return (
              <BreadcrumbItem
                key={c.path}
                href="#"
                isCurrentPage={isLast}
                onClick={(e) => {
                  e.preventDefault();
                  if (!isLast) onNavigate(c.path);
                }}
              >
                {c.name}
              </BreadcrumbItem>
            );
          })}
        </Breadcrumb>
        {data && (
          <Tag type="cool-gray" size="sm" className="fsoid">
            {data.fsoid}
          </Tag>
        )}
      </div>

      {error && (
        <InlineNotification
          kind="error"
          lowContrast
          title="Failed to list directory"
          subtitle={error}
          hideCloseButton
        />
      )}

      {busy && !data ? (
        <DataTableSkeleton columnCount={5} rowCount={8} showHeader={false} showToolbar={false} />
      ) : data && data.entries.length === 0 ? (
        <div className="contents-empty">This folder is empty.</div>
      ) : data ? (
        <TableContainer>
          {selected.size > 0 && (
            <TableToolbar>
              <TableBatchActions
                shouldShowBatchActions={selected.size > 0}
                totalSelected={selected.size}
                onCancel={() => setSelected(new Set())}
              >
                <TableBatchAction
                  renderIcon={Download}
                  onClick={() => onRecoverBatch(Array.from(selected))}
                >
                  Recover selected
                </TableBatchAction>
              </TableBatchActions>
            </TableToolbar>
          )}
          <Table size="sm" useZebraStyles>
            <TableHead>
              <TableRow>
                <TableSelectAll
                  id="select-all"
                  name="select-all"
                  ariaLabel="Select all rows"
                  checked={allSelected}
                  indeterminate={someSelected}
                  onSelect={toggleAll}
                />
                <TableHeader>Name</TableHeader>
                <TableHeader>Kind</TableHeader>
                <TableHeader>Size</TableHeader>
                <TableHeader>Actions</TableHeader>
              </TableRow>
            </TableHead>
            <TableBody>
              {data.entries.map((entry) => {
                const full = joinPath(path, entry.name);
                const isDir = entry.type === "dir";
                const showDetails = () => onShowDetails(full);
                return (
                  <TableRow
                    key={`${entry.fsoid}:${entry.name}`}
                    className={detailPath === full ? "is-detail" : undefined}
                  >
                    <TableSelectRow
                      id={`sel:${entry.fsoid}:${entry.name}`}
                      name={`sel:${entry.fsoid}:${entry.name}`}
                      ariaLabel={`Select ${entry.name}`}
                      checked={selected.has(full)}
                      onSelect={() => toggleOne(full)}
                    />
                    <TableCell onClick={showDetails}>
                      <button
                        type="button"
                        className={`cell-name${isDir ? " is-dir" : ""}`}
                        onClick={() => (isDir ? onNavigate(full) : onShowDetails(full))}
                        title={isDir ? "Open folder" : "Show details"}
                      >
                        {kindIcon(entry.type)}
                        {entry.name}
                      </button>
                    </TableCell>
                    <TableCell onClick={showDetails}>{entry.type}</TableCell>
                    <TableCell onClick={showDetails}>{humanSize(entry.size)}</TableCell>
                    <TableCell>
                      <div className="cell-actions">
                        <Button
                          kind="ghost"
                          size="sm"
                          renderIcon={Download}
                          onClick={() => (isDir ? onRecoverBatch([full]) : onRecover(full))}
                        >
                          Recover
                        </Button>
                      </div>
                    </TableCell>
                  </TableRow>
                );
              })}
            </TableBody>
          </Table>
        </TableContainer>
      ) : null}
    </div>
  );
}
