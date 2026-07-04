import { useEffect, useState } from "react";
import {
  Accordion,
  AccordionItem,
  Button,
  InlineNotification,
  SkeletonText,
  Tag,
} from "@carbon/react";
import { Close } from "@carbon/icons-react";

import { errMessage, statPath } from "../lib/tauri";
import { humanSize, shortIso } from "../lib/format";
import type { InspectResult, StatResult } from "../lib/types";

interface InfoPanelProps {
  inspect: InspectResult | null;
  inspectError: string | null;
  /** Currently selected 1-based volume index. */
  volume: number;
  onVolumeChange: (volume: number) => void;
  /** Path of the entry whose details to show, or null for none. */
  detailPath: string | null;
  /** User-draggable panel width in px. */
  width: number;
  onClose: () => void;
}

/** One key/value line of a dashboard section. */
function KV({ k, mono, children }: { k: string; mono?: boolean; children: React.ReactNode }) {
  if (children == null || children === "") return null;
  return (
    <div className="kv-row">
      <span className="kv-key">{k}</span>
      <span className={`kv-val${mono ? " mono" : ""}`}>{children}</span>
    </div>
  );
}

function recoverabilityTag(status: string) {
  const type = status.startsWith("ok") ? "green" : status === "encrypted" ? "purple" : "red";
  return (
    <Tag type={type} size="sm">
      {status}
    </Tag>
  );
}

/** Right-hand dashboard: metadata extracted from the open image (device,
 *  container superblock, volumes, sparsebundle bands, snapshots) plus details
 *  for the entry selected in the explorer. */
export default function InfoPanel({
  inspect,
  inspectError,
  volume,
  onVolumeChange,
  detailPath,
  width,
  onClose,
}: InfoPanelProps) {
  const [stat, setStat] = useState<StatResult | null>(null);
  const [statError, setStatError] = useState<string | null>(null);

  useEffect(() => {
    setStat(null);
    setStatError(null);
    if (!detailPath) return;
    let alive = true;
    statPath(detailPath)
      .then((s) => {
        if (alive) setStat(s);
      })
      .catch((err) => {
        if (alive) setStatError(errMessage(err));
      });
    return () => {
      alive = false;
    };
  }, [detailPath]);

  const snapshots = inspect?.snapshots ?? [];

  return (
    <aside className="info-panel" aria-label="Image details" style={{ width }}>
      <div className="info-head">
        <h2 className="info-title">Details</h2>
        <Button
          kind="ghost"
          size="sm"
          hasIconOnly
          renderIcon={Close}
          iconDescription="Close panel"
          tooltipAlignment="end"
          onClick={onClose}
        />
      </div>

      {inspectError && (
        <InlineNotification
          kind="error"
          lowContrast
          title="Could not inspect image"
          subtitle={inspectError}
          hideCloseButton
        />
      )}

      {!inspect && !inspectError && (
        <div className="info-loading">
          <SkeletonText paragraph lineCount={6} />
        </div>
      )}

      {inspect && (
        <Accordion size="sm" align="start">
          {detailPath && (
            <AccordionItem title="Selected item" open>
              {statError && <p className="info-error">{statError}</p>}
              {!stat && !statError && <SkeletonText paragraph lineCount={4} />}
              {stat && (
                <div className="kv">
                  <KV k="Path" mono>
                    {stat.path}
                  </KV>
                  <KV k="FSOID" mono>
                    {stat.fsoid}
                  </KV>
                  {stat.inode && (
                    <>
                      <KV k="Kind">
                        {stat.inode.kind}
                        {stat.inode.sparse && (
                          <Tag type="teal" size="sm">
                            sparse
                          </Tag>
                        )}
                        {stat.inode.has_rsrc_fork && (
                          <Tag type="gray" size="sm">
                            rsrc fork
                          </Tag>
                        )}
                      </KV>
                      <KV k="Size">
                        {stat.inode.size != null ? humanSize(stat.inode.size) : undefined}
                      </KV>
                      <KV k="On disk">
                        {stat.inode.allocated_size != null
                          ? humanSize(stat.inode.allocated_size)
                          : undefined}
                      </KV>
                      <KV k="Mode" mono>
                        {stat.inode.mode}
                      </KV>
                      <KV k="Owner" mono>
                        {stat.inode.uid}:{stat.inode.gid}
                      </KV>
                      <KV k="Created">{shortIso(stat.inode.create_time)}</KV>
                      <KV k="Modified">{shortIso(stat.inode.mod_time)}</KV>
                      <KV k="Changed">{shortIso(stat.inode.change_time)}</KV>
                      <KV k="Accessed">{shortIso(stat.inode.access_time)}</KV>
                    </>
                  )}
                  <KV k="Symlink" mono>
                    {stat.symlink_target}
                  </KV>
                  {stat.recoverability && (
                    <KV k="Recovery">
                      {recoverabilityTag(stat.recoverability.status)}
                      {stat.recoverability.reason && (
                        <span className="kv-note">{stat.recoverability.reason}</span>
                      )}
                    </KV>
                  )}
                  {stat.xattrs.length > 0 && (
                    <KV k="Xattrs">
                      <ul className="xattr-list">
                        {stat.xattrs.map((x) => (
                          <li key={x.name} className="mono">
                            {x.name} <span className="kv-note">({humanSize(x.data_len)})</span>
                          </li>
                        ))}
                      </ul>
                    </KV>
                  )}
                </div>
              )}
            </AccordionItem>
          )}

          <AccordionItem title="Image" open={!detailPath}>
            <div className="kv">
              <KV k="Path" mono>
                {inspect.image.path}
              </KV>
              <KV k="Kind">{inspect.image.kind}</KV>
              <KV k="Size">{humanSize(inspect.image.size)}</KV>
              <KV k="Container at">
                {inspect.image.container_offset.toLocaleString()} B
                {inspect.image.partition_index != null &&
                  ` (partition ${inspect.image.partition_index})`}
              </KV>
            </div>
          </AccordionItem>

          <AccordionItem title="Container" open={!detailPath}>
            <div className="kv">
              <KV k="UUID" mono>
                {inspect.container.uuid}
              </KV>
              <KV k="Capacity">
                {humanSize(inspect.container.total_bytes)} (
                {inspect.container.block_count.toLocaleString()} ×{" "}
                {inspect.container.block_size} B)
              </KV>
              <KV k="Checkpoint" mono>
                XID {inspect.container.checkpoint_xid} · index {inspect.container.checkpoint_index}
              </KV>
              <KV k="Features" mono>
                {inspect.container.features}
              </KV>
              <KV k="Incompatible" mono>
                {inspect.container.incompatible_features}
              </KV>
              <KV k="Keylocker">{inspect.container.keylocker ? "yes" : "no"}</KV>
              <KV k="Volume slots">{inspect.container.max_file_systems}</KV>
            </div>
          </AccordionItem>

          <AccordionItem title={`Volumes (${inspect.volumes.length})`} open>
            <ul className="vol-list">
              {inspect.volumes.map((v) => (
                <li key={v.index}>
                  <button
                    type="button"
                    className={`vol-row${v.index === volume ? " is-active" : ""}`}
                    onClick={() => onVolumeChange(v.index)}
                    title={`Switch to volume ${v.index}`}
                  >
                    <span className="vol-line">
                      <span className="vol-name">
                        {v.index} · {v.name || "(unnamed)"}
                      </span>
                      <Tag type="blue" size="sm">
                        {v.role}
                      </Tag>
                      {v.encrypted && (
                        <Tag type="purple" size="sm">
                          encrypted
                        </Tag>
                      )}
                      {v.sealed && (
                        <Tag type="teal" size="sm">
                          sealed
                        </Tag>
                      )}
                    </span>
                    <span className="vol-stats">
                      {v.num_files.toLocaleString()} files ·{" "}
                      {v.num_directories.toLocaleString()} folders · {v.num_snapshots} snapshots
                    </span>
                  </button>
                </li>
              ))}
            </ul>
          </AccordionItem>

          {inspect.sparsebundle && (
            <AccordionItem title="Sparsebundle bands">
              <div className="kv">
                <KV k="Band size">{humanSize(inspect.sparsebundle.band_size)}</KV>
                <KV k="Logical size">{humanSize(inspect.sparsebundle.logical_size)}</KV>
                <KV k="Bands">
                  {inspect.sparsebundle.present_bands.toLocaleString()} of{" "}
                  {inspect.sparsebundle.expected_bands.toLocaleString()} present
                </KV>
                {/* Absent bands are normal sparse holes (never-written space),
                    not damage — short bands are the actual red flag. */}
                <KV k="Absent">
                  {inspect.sparsebundle.missing_band_count > 0
                    ? `${inspect.sparsebundle.missing_band_count.toLocaleString()} (sparse holes)`
                    : "none"}
                </KV>
                {inspect.sparsebundle.short_band_count > 0 && (
                  <KV k="Short bands">
                    <Tag type="red" size="sm">
                      {inspect.sparsebundle.short_band_count.toLocaleString()} truncated
                    </Tag>
                  </KV>
                )}
                <KV k="UUID" mono>
                  {inspect.sparsebundle.uuid}
                </KV>
              </div>
            </AccordionItem>
          )}

          <AccordionItem title={`Snapshots (${snapshots.length})`}>
            {inspect.snapshots_error && <p className="info-error">{inspect.snapshots_error}</p>}
            {snapshots.length === 0 && !inspect.snapshots_error && (
              <p className="kv-note">No snapshots on this volume.</p>
            )}
            {snapshots.length > 0 && (
              <ul className="snap-list">
                {snapshots.map((s) => (
                  <li key={s.xid} className="snap-row">
                    <span className="snap-name">{s.name}</span>
                    <span className="snap-meta mono">
                      {shortIso(s.create_time)} · {s.xid}
                    </span>
                  </li>
                ))}
              </ul>
            )}
          </AccordionItem>

          {inspect.warnings.length > 0 && (
            <AccordionItem title={`Warnings (${inspect.warnings.length})`} open>
              <ul className="warn-list">
                {inspect.warnings.map((w, i) => (
                  <li key={i}>{w}</li>
                ))}
              </ul>
            </AccordionItem>
          )}
        </Accordion>
      )}
    </aside>
  );
}
