import { useEffect, useState } from "react";
import { Button, ClickableTile, SkeletonText, Tag } from "@carbon/react";
import { DataBase, FolderOpen, TrashCan } from "@carbon/icons-react";

import { errMessage, recentImages, removeRecent } from "../lib/tauri";
import { baseName, formatEpoch } from "../lib/format";
import type { RecentImage } from "../lib/types";

interface StartPageProps {
  /** Open the native file picker. */
  onPick: () => void;
  /** Open a specific path from the recents list. */
  onOpenPath: (path: string) => void;
  /** Surface a load error as a toast. */
  onError: (title: string, subtitle?: string) => void;
}

/** The landing view when no image is open: a hero with the open action plus
 *  the recently-opened images, each re-openable with one click. */
export default function StartPage({ onPick, onOpenPath, onError }: StartPageProps) {
  const [recents, setRecents] = useState<RecentImage[] | null>(null);

  useEffect(() => {
    let alive = true;
    recentImages()
      .then((list) => {
        if (alive) setRecents(list);
      })
      .catch((err) => {
        if (alive) {
          setRecents([]);
          onError("Could not load recent images", errMessage(err));
        }
      });
    return () => {
      alive = false;
    };
  }, [onError]);

  const handleRemove = async (path: string) => {
    try {
      setRecents(await removeRecent(path));
    } catch (err) {
      onError("Could not update recent images", errMessage(err));
    }
  };

  return (
    <div className="start-page">
      <div className="start-hero">
        <DataBase size={48} className="start-mark" aria-hidden />
        <h1 className="start-title">apfsRelic</h1>
        <p className="start-subtitle">
          Read-only APFS explorer and file recovery. Open a <code>.sparsebundle</code> or a raw
          disk image — nothing is ever written to it.
        </p>
        <Button kind="primary" size="lg" renderIcon={FolderOpen} onClick={onPick}>
          Open disk image…
        </Button>
      </div>

      <div className="start-recents">
        <h2 className="start-recents-title">Recent</h2>
        {recents === null ? (
          <SkeletonText paragraph lineCount={3} />
        ) : recents.length === 0 ? (
          <p className="start-empty">
            No recent images yet. Images you open appear here for quick access.
          </p>
        ) : (
          <ul className="recent-list">
            {recents.map((r) => (
              <li key={r.path} className="recent-row">
                <ClickableTile
                  className="recent-tile"
                  onClick={() => {
                    if (r.exists) onOpenPath(r.path);
                    else onError("Image not found", r.path);
                  }}
                >
                  <div className="recent-line">
                    <span className="recent-name">{baseName(r.path)}</span>
                    {r.is_sparsebundle && (
                      <Tag type="cool-gray" size="sm">
                        sparsebundle
                      </Tag>
                    )}
                    {!r.exists && (
                      <Tag type="red" size="sm">
                        missing
                      </Tag>
                    )}
                  </div>
                  <div className="recent-line recent-meta">
                    <span className="recent-path">{r.path}</span>
                    <span className="recent-time">{formatEpoch(r.last_opened)}</span>
                  </div>
                </ClickableTile>
                <Button
                  kind="ghost"
                  size="md"
                  hasIconOnly
                  renderIcon={TrashCan}
                  iconDescription="Remove from recents"
                  tooltipAlignment="end"
                  onClick={() => void handleRemove(r.path)}
                />
              </li>
            ))}
          </ul>
        )}
      </div>
    </div>
  );
}
