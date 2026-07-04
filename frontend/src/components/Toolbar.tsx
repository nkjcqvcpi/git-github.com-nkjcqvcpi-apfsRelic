import { Dropdown, Tag } from "@carbon/react";

import type { AppConfig, VolumeInfo } from "../lib/types";

interface ToolbarProps {
  config: AppConfig | null;
  /** Volume summaries from `inspect`, or null while loading. */
  volumes: VolumeInfo[] | null;
  onVolumeChange: (volume: number) => void;
}

function volumeLabel(v: VolumeInfo): string {
  const name = v.name || "(unnamed)";
  return `${v.index} · ${name} (${v.role})`;
}

/** Secondary toolbar under the header: which container/volume is open and the
 *  volume selector (fed with real volume names from `inspect`). */
export default function Toolbar({ config, volumes, onVolumeChange }: ToolbarProps) {
  const items = volumes ?? [];
  const current = items.find((v) => v.index === config?.volume) ?? null;

  return (
    <div className="app-toolbar">
      <span className="toolbar-meta" title={config?.container ?? ""}>
        {config ? config.container : "loading…"}
      </span>
      {config?.is_sparsebundle && (
        <Tag type="cool-gray" size="sm">
          sparsebundle
        </Tag>
      )}
      {config && !config.container_exists && (
        <Tag type="red" size="sm">
          not found
        </Tag>
      )}

      <span className="toolbar-spacer" />

      <Dropdown
        id="volume"
        className="toolbar-vol"
        size="sm"
        titleText="Volume"
        hideLabel
        label={config ? `Volume ${config.volume}` : "Volume"}
        items={items}
        itemToString={(v) => (v ? volumeLabel(v) : "")}
        selectedItem={current}
        disabled={items.length === 0}
        onChange={({ selectedItem }) => {
          if (selectedItem) onVolumeChange(selectedItem.index);
        }}
      />
    </div>
  );
}
