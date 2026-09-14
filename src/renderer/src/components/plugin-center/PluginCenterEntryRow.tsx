import { Database, FileText, Plug, ScrollText, Trash2, type LucideIcon } from 'lucide-react'
import type { PluginCenterEntry } from '../../../../bridge/mock/fixtures'
import { Badge } from '../ui/badge'
import { Button } from '../ui/button'
import { Switch } from '../ui/switch'

// Why: importing lucide's full icon map would bundle every icon and defeat
// tree-shaking, so plugin manifests pick from a curated set (fallback: Plug).
// Phase 0 maps only the mock fixture icons; the real set lands with ade-plugins.
const PLUGIN_ENTRY_ICONS: Record<string, LucideIcon> = {
  database: Database,
  filetext: FileText,
  plug: Plug
}

function resolveEntryIcon(entry: PluginCenterEntry): LucideIcon {
  const iconName = entry.panels[0]?.icon
  if (!iconName) {
    return Plug
  }
  // Accept both lucide naming styles ('file-text' and 'FileText').
  const normalized = iconName.replaceAll('-', '').toLowerCase()
  return Object.hasOwn(PLUGIN_ENTRY_ICONS, normalized)
    ? (PLUGIN_ENTRY_ICONS[normalized] ?? Plug)
    : Plug
}

/** Mirrors PluginSettingsRow: anything that is not explicitly disabled counts as enabled. */
function isPluginCenterEntryEnabled(entry: PluginCenterEntry): boolean {
  return entry.status !== 'disabled'
}

type PluginCenterEntryRowProps = {
  entry: PluginCenterEntry
  onToggle: (entry: PluginCenterEntry, enabled: boolean) => void
  onOpenLogs: (entry: PluginCenterEntry) => void
  onRemove?: (entry: PluginCenterEntry) => void
}

export function PluginCenterEntryRow({
  entry,
  onToggle,
  onOpenLogs,
  onRemove
}: PluginCenterEntryRowProps): React.JSX.Element {
  const Icon = resolveEntryIcon(entry)
  const enabled = isPluginCenterEntryEnabled(entry)

  return (
    <div className="flex items-start gap-3 px-4 py-3" data-plugin-key={entry.pluginKey}>
      <div className="flex size-9 shrink-0 items-center justify-center rounded-md border border-border bg-muted/30">
        <Icon className="size-4 text-muted-foreground" />
      </div>
      <div className="min-w-0 flex-1">
        <p className="truncate text-sm font-medium text-foreground">{entry.name}</p>
        <p className="mt-0.5 truncate text-xs text-muted-foreground">
          {entry.publisher ? `${entry.publisher} · ` : null}v{entry.version}
        </p>
        <p className="mt-1 line-clamp-2 text-xs leading-5 text-muted-foreground">
          {entry.description ?? '暂无描述'}
        </p>
      </div>
      <div className="flex shrink-0 items-center gap-1.5">
        <Badge variant={entry.scope === 'global' ? 'secondary' : 'outline'}>
          {entry.scope === 'global' ? '全局' : '当前项目'}
        </Badge>
        <Button
          variant="ghost"
          size="icon-xs"
          aria-label={`查看 ${entry.name} 的运行日志`}
          title="运行日志"
          onClick={() => onOpenLogs(entry)}
        >
          <ScrollText />
        </Button>
        {onRemove ? (
          <Button
            variant="ghost"
            size="icon-xs"
            aria-label={`卸载 ${entry.name}`}
            title="卸载"
            onClick={() => onRemove(entry)}
          >
            <Trash2 />
          </Button>
        ) : null}
        <Switch
          checked={enabled}
          onCheckedChange={(checked) => onToggle(entry, checked)}
          aria-label={enabled ? `禁用 ${entry.name}` : `启用 ${entry.name}`}
        />
      </div>
    </div>
  )
}
