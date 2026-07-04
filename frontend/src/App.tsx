import { useCallback, useEffect, useRef, useState } from "react";
import { InlineLoading, InlineNotification, Modal, Tag, Theme } from "@carbon/react";

import AppHeader from "./components/AppHeader";
import Toolbar from "./components/Toolbar";
import DirTree from "./components/DirTree";
import DirContents from "./components/DirContents";
import InfoPanel from "./components/InfoPanel";
import StartPage from "./components/StartPage";
import Toasts, { type ToastItem, type ToastKind } from "./components/Toasts";

import {
  clearLsCache,
  closeContainer,
  errMessage,
  getConfig,
  inspectImage,
  onRecoverProgress,
  openContainer,
  pickContainer,
  recoverBatch,
  recoverPath,
  setVolume,
} from "./lib/tauri";
import { humanSize } from "./lib/format";
import type {
  AppConfig,
  BatchProgress,
  BatchResult,
  InspectResult,
} from "./lib/types";

/** A persisted pane width, clamped to the given range. */
function loadWidth(key: string, fallback: number, min: number, max: number): number {
  const raw = Number(localStorage.getItem(key));
  if (!Number.isFinite(raw) || raw <= 0) return fallback;
  return Math.min(max, Math.max(min, raw));
}

const TREE_MIN = 160;
const TREE_MAX = 520;
const INFO_MIN = 260;
const INFO_MAX = 640;

export default function App() {
  const [config, setConfig] = useState<AppConfig | null>(null);
  const [activePath, setActivePath] = useState("/");
  const [toasts, setToasts] = useState<ToastItem[]>([]);
  // Bumping `reloadKey` remounts both panes (fresh state after the container or
  // volume changes); `contentsNonce` re-fetches only the right pane (Refresh).
  const [reloadKey, setReloadKey] = useState(0);
  const [contentsNonce, setContentsNonce] = useState(0);

  // Dashboard state. `detailPath` follows every click: rows, tree nodes, and
  // folder navigation all point the panel at what was last clicked.
  const [inspect, setInspect] = useState<InspectResult | null>(null);
  const [inspectError, setInspectError] = useState<string | null>(null);
  const [infoOpen, setInfoOpen] = useState(true);
  const [detailPath, setDetailPath] = useState<string | null>(null);

  // User-draggable pane widths (persisted).
  const [treeWidth, setTreeWidth] = useState(() =>
    loadWidth("apfsrelic.treeWidth", 260, TREE_MIN, TREE_MAX),
  );
  const [infoWidth, setInfoWidth] = useState(() =>
    loadWidth("apfsrelic.infoWidth", 340, INFO_MIN, INFO_MAX),
  );
  useEffect(() => {
    localStorage.setItem("apfsrelic.treeWidth", String(treeWidth));
  }, [treeWidth]);
  useEffect(() => {
    localStorage.setItem("apfsrelic.infoWidth", String(infoWidth));
  }, [infoWidth]);

  // Batch-recovery state: live progress while running, then a report.
  const [batch, setBatch] = useState<BatchProgress | null>(null);
  const [report, setReport] = useState<BatchResult | null>(null);
  const batchRunning = useRef(false);

  const nextToastId = useRef(1);

  const pushToast = useCallback((kind: ToastKind, title: string, subtitle?: string) => {
    const id = nextToastId.current++;
    setToasts((prev) => [...prev, { id, kind, title, subtitle }]);
  }, []);

  const dismissToast = useCallback((id: number) => {
    setToasts((prev) => prev.filter((t) => t.id !== id));
  }, []);

  const loadConfig = useCallback(async () => {
    try {
      setConfig(await getConfig());
    } catch (err) {
      pushToast("error", "Could not read container config", errMessage(err));
    }
  }, [pushToast]);

  useEffect(() => {
    void loadConfig();
  }, [loadConfig]);

  // Live progress from `recover_batch` (ignored when no batch is running).
  useEffect(() => {
    const unlisten = onRecoverProgress((p) => {
      if (batchRunning.current) setBatch(p);
    });
    return () => {
      void unlisten.then((f) => f());
    };
  }, []);

  // Refresh the dashboard whenever the browsed image/volume changes.
  const containerOpen = config?.container != null;
  useEffect(() => {
    if (!containerOpen) {
      setInspect(null);
      setInspectError(null);
      return;
    }
    let alive = true;
    setInspect(null);
    setInspectError(null);
    inspectImage()
      .then((res) => {
        if (alive) setInspect(res);
      })
      .catch((err) => {
        if (alive) setInspectError(errMessage(err));
      });
    return () => {
      alive = false;
    };
  }, [containerOpen, config?.container, config?.volume, reloadKey]);

  // Reset selection + caches and remount the panes after the browsed image
  // changes, so nothing from the previous container/volume lingers.
  const resetView = useCallback(() => {
    clearLsCache();
    setActivePath("/");
    setDetailPath(null);
    setReloadKey((k) => k + 1);
  }, []);

  // Drag a pane divider: track the pointer until mouseup, clamped.
  const startResize = useCallback(
    (side: "tree" | "info") => (e: React.MouseEvent) => {
      e.preventDefault();
      document.body.classList.add("is-resizing");
      const onMove = (ev: MouseEvent) => {
        if (side === "tree") {
          setTreeWidth(Math.min(TREE_MAX, Math.max(TREE_MIN, ev.clientX)));
        } else {
          setInfoWidth(
            Math.min(INFO_MAX, Math.max(INFO_MIN, window.innerWidth - ev.clientX)),
          );
        }
      };
      const onUp = () => {
        document.body.classList.remove("is-resizing");
        window.removeEventListener("mousemove", onMove);
        window.removeEventListener("mouseup", onUp);
      };
      window.addEventListener("mousemove", onMove);
      window.addEventListener("mouseup", onUp);
    },
    [],
  );

  const handlePick = useCallback(async () => {
    try {
      const res = await pickContainer();
      if ("cancelled" in res) return;
      setConfig(res);
      resetView();
    } catch (err) {
      pushToast("error", "Open failed", errMessage(err));
    }
  }, [pushToast, resetView]);

  const handleOpenPath = useCallback(
    async (path: string) => {
      try {
        setConfig(await openContainer(path));
        resetView();
      } catch (err) {
        pushToast("error", "Open failed", errMessage(err));
      }
    },
    [pushToast, resetView],
  );

  const handleCloseImage = useCallback(async () => {
    try {
      setConfig(await closeContainer());
      resetView();
    } catch (err) {
      pushToast("error", "Could not close image", errMessage(err));
    }
  }, [pushToast, resetView]);

  const handleVolume = useCallback(
    async (volume: number) => {
      if (config && volume === config.volume) return;
      try {
        setConfig(await setVolume(volume));
        resetView();
      } catch (err) {
        pushToast("error", "Could not switch volume", errMessage(err));
      }
    },
    [config, pushToast, resetView],
  );

  const handleRefresh = useCallback(() => {
    clearLsCache();
    void loadConfig();
    setReloadKey((k) => k + 1);
    setContentsNonce((n) => n + 1);
  }, [loadConfig]);

  // Navigating (tree click, folder-name click, breadcrumb) also points the
  // details panel at the destination folder.
  const handleNavigate = useCallback((path: string) => {
    setActivePath(path);
    setDetailPath(path);
  }, []);

  const handleShowDetails = useCallback((path: string) => {
    setDetailPath(path);
    setInfoOpen(true);
  }, []);

  const handleRecover = useCallback(
    async (path: string) => {
      pushToast("info", "Recovering…", path);
      try {
        const r = await recoverPath(path);
        if (r.cancelled) {
          pushToast("info", "Recovery cancelled");
          return;
        }
        const where = r.path ?? "";
        const note = r.symlink
          ? `symlink → ${r.symlink}`
          : r.complete === false
            ? `partial: ${humanSize(r.bytes)}`
            : humanSize(r.bytes);
        pushToast("success", "Recovered", `${where}${note ? ` (${note})` : ""}`);
      } catch (err) {
        pushToast("error", "Recover failed", errMessage(err));
      }
    },
    [pushToast],
  );

  const handleRecoverBatch = useCallback(
    async (paths: string[]) => {
      if (batchRunning.current) {
        pushToast("info", "A recovery is already running");
        return;
      }
      batchRunning.current = true;
      setBatch({ done: 0, bytes: 0, current: "" });
      try {
        const r = await recoverBatch(paths);
        if (r.cancelled) return;
        const failed = (r.errors ?? 0) + (r.partial ?? 0);
        const summary = `${(r.recovered ?? 0).toLocaleString()} file(s), ${humanSize(
          r.bytes_total ?? 0,
        )} → ${r.dest ?? ""}`;
        if (failed > 0) {
          pushToast("warning", `Recovered with ${failed} problem(s)`, summary);
          setReport(r);
        } else {
          pushToast("success", "Recovered", summary);
        }
      } catch (err) {
        pushToast("error", "Batch recovery failed", errMessage(err));
      } finally {
        batchRunning.current = false;
        setBatch(null);
      }
    },
    [pushToast],
  );

  const containerMissing = containerOpen && config != null && !config.container_exists;

  // Initial config still loading.
  if (config === null) {
    return (
      <Theme theme="g100">
        <div className="app-root app-splash">
          <InlineLoading description="Starting…" />
        </div>
      </Theme>
    );
  }

  // No image open: the start page.
  if (!containerOpen) {
    return (
      <Theme theme="g100">
        <div className="app-root">
          <AppHeader
            imageOpen={false}
            infoOpen={false}
            onOpen={handlePick}
            onRefresh={handleRefresh}
            onToggleInfo={() => {}}
            onCloseImage={() => {}}
          />
          <div className="app-body">
            <StartPage onPick={handlePick} onOpenPath={handleOpenPath} onError={(t, s) => pushToast("error", t, s)} />
          </div>
          <Toasts toasts={toasts} onDismiss={dismissToast} />
        </div>
      </Theme>
    );
  }

  return (
    <Theme theme="g100">
      <div className="app-root">
        <AppHeader
          imageOpen
          infoOpen={infoOpen}
          onOpen={handlePick}
          onRefresh={handleRefresh}
          onToggleInfo={() => setInfoOpen((v) => !v)}
          onCloseImage={handleCloseImage}
        />
        <div className="app-body">
          <Toolbar
            config={config}
            volumes={inspect?.volumes ?? null}
            onVolumeChange={handleVolume}
          />

          {containerMissing && (
            <InlineNotification
              kind="warning"
              lowContrast
              title="Container not found"
              subtitle={`${config.container} — use “Open…” to pick a .sparsebundle or raw image.`}
              hideCloseButton
            />
          )}

          <div className="app-panes">
            <div className="pane-tree" style={{ width: treeWidth }}>
              <DirTree key={reloadKey} activePath={activePath} onSelect={handleNavigate} />
            </div>
            <div
              className="pane-resizer"
              role="separator"
              aria-orientation="vertical"
              aria-label="Resize navigation pane"
              onMouseDown={startResize("tree")}
            />
            <DirContents
              key={reloadKey}
              path={activePath}
              reloadNonce={contentsNonce}
              detailPath={detailPath}
              onNavigate={handleNavigate}
              onRecover={handleRecover}
              onRecoverBatch={handleRecoverBatch}
              onShowDetails={handleShowDetails}
            />
            {infoOpen && (
              <>
                <div
                  className="pane-resizer"
                  role="separator"
                  aria-orientation="vertical"
                  aria-label="Resize details panel"
                  onMouseDown={startResize("info")}
                />
                <InfoPanel
                  inspect={inspect}
                  inspectError={inspectError}
                  volume={config.volume}
                  onVolumeChange={handleVolume}
                  detailPath={detailPath}
                  width={infoWidth}
                  onClose={() => setInfoOpen(false)}
                />
              </>
            )}
          </div>
        </div>

        {batch && (
          <div className="batch-overlay" role="status" aria-live="polite">
            <InlineLoading
              description={`Recovering… ${batch.done.toLocaleString()} file(s), ${humanSize(batch.bytes)}`}
            />
            {batch.current && <div className="batch-current">{batch.current}</div>}
          </div>
        )}

        <Modal
          open={report != null}
          passiveModal
          modalHeading="Recovery report"
          onRequestClose={() => setReport(null)}
        >
          {report && (
            <div className="report-body">
              <p className="report-summary">
                {(report.recovered ?? 0).toLocaleString()} recovered ·{" "}
                {(report.partial ?? 0).toLocaleString()} partial ·{" "}
                {(report.errors ?? 0).toLocaleString()} failed · {humanSize(report.bytes_total ?? 0)}{" "}
                total → <span className="mono">{report.dest}</span>
              </p>
              {report.results_truncated && (
                <p className="report-note">
                  Only the first {report.results?.length.toLocaleString()} outcomes are listed.
                </p>
              )}
              <ul className="report-list">
                {(report.results ?? [])
                  .filter((r) => r.status !== "recovered")
                  .map((r, i) => (
                    <li key={i} className="report-row">
                      <Tag type={r.status === "partial" ? "magenta" : "red"} size="sm">
                        {r.status}
                      </Tag>
                      <span className="mono report-path">{r.path}</span>
                      {r.note && <span className="report-note">{r.note}</span>}
                    </li>
                  ))}
              </ul>
            </div>
          )}
        </Modal>

        <Toasts toasts={toasts} onDismiss={dismissToast} />
      </div>
    </Theme>
  );
}
