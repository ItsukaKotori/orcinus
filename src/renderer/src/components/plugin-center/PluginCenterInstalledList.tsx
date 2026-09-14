import { useState } from 'react'
import { Blocks, Loader2 } from 'lucide-react'
import type { PluginHostLogLine } from '../../../../preload/api-types'
import type { PluginCenterEntry } from '../../../../bridge/mock/fixtures'
import { useAppStore } from '@/store'
import { PluginCenterEntryRow } from './PluginCenterEntryRow'

type PluginCenterLogsState = {
  loading: boolean
  lines?: PluginHostLogLine[]
  error?: boolean
}

function PluginLogsPanel({ state }: { state?: PluginCenterLogsState }): React.JSX.Element {
  if (state?.loading) {
    return (
      <div className="flex items-center gap-2 border-t border-border bg-muted/40 px-4 py-2.5 text-xs text-muted-foreground">
        <Loader2 className="size-3.5 animate-spin" />
        正在加载日志…
      </div>
    )
  }
  if (state?.error) {
    return (
      <div className="border-t border-border bg-muted/40 px-4 py-2.5 text-xs text-destructive">
        日志加载失败。
      </div>
    )
  }
  return (
    <div className="border-t border-border bg-muted/40 px-4 py-2.5">
      <pre
        tabIndex={0}
        className="max-h-44 overflow-auto font-mono text-[11px] leading-5 scrollbar-sleek"
      >
        {state?.lines?.length
          ? state.lines
              .map(
                (line) => `${new Date(line.ts).toLocaleTimeString()} ${line.level.padEnd(5)} ${line.line}`
              )
              .join('\n')
          : '暂无日志。'}
      </pre>
    </div>
  )
}

function InstalledListState({ title }: { title: string }): React.JSX.Element {
  return (
    <div className="flex items-center gap-2 rounded-lg border border-dashed border-border px-5 py-8 text-[13px] text-muted-foreground">
      <Blocks className="size-4 shrink-0" />
      {title}
    </div>
  )
}

/** 已安装插件列表：数据源为 pluginCenterEntries（Task 7 的右侧过滤共用同一数据源）。 */
export function PluginCenterInstalledList(): React.JSX.Element {
  const entries = useAppStore((s) => s.pluginCenterEntries)
  const status = useAppStore((s) => s.pluginCenterStatus)
  const [openLogsKey, setOpenLogsKey] = useState<string | null>(null)
  const [logsByPlugin, setLogsByPlugin] = useState<Record<string, PluginCenterLogsState>>({})

  const reload = (): void => {
    void useAppStore.getState().loadPluginCenterEntries()
  }

  const handleToggle = (entry: PluginCenterEntry, enabled: boolean): void => {
    void window.api.plugins
      .setEnabled({ pluginKey: entry.pluginKey, enabled })
      .then(reload)
      .catch((cause: unknown) => {
        console.warn('[plugin-center] failed to toggle plugin:', cause)
      })
  }

  const handleRemove = (entry: PluginCenterEntry): void => {
    void window.api.plugins
      .remove({ pluginKey: entry.pluginKey })
      .then(() => {
        setOpenLogsKey((current) => (current === entry.pluginKey ? null : current))
        reload()
      })
      .catch((cause: unknown) => {
        console.warn('[plugin-center] failed to remove plugin:', cause)
      })
  }

  const handleOpenLogs = (entry: PluginCenterEntry): void => {
    if (openLogsKey === entry.pluginKey) {
      setOpenLogsKey(null)
      return
    }
    setOpenLogsKey(entry.pluginKey)
    if (logsByPlugin[entry.pluginKey]?.lines) {
      return
    }
    setLogsByPlugin((current) => ({ ...current, [entry.pluginKey]: { loading: true } }))
    void window.api.plugins
      .getLogs({ pluginKey: entry.pluginKey })
      .then((lines) => {
        setLogsByPlugin((current) => ({
          ...current,
          [entry.pluginKey]: { loading: false, lines }
        }))
      })
      .catch((cause: unknown) => {
        console.warn('[plugin-center] failed to load plugin logs:', cause)
        setLogsByPlugin((current) => ({
          ...current,
          [entry.pluginKey]: { loading: false, error: true }
        }))
      })
  }

  if (entries.length === 0) {
    if (status === 'loading') {
      return <InstalledListState title="正在加载插件…" />
    }
    if (status === 'error') {
      return <InstalledListState title="插件列表加载失败。" />
    }
    return <InstalledListState title="暂无已安装插件。" />
  }

  return (
    <div className="divide-y divide-border overflow-hidden rounded-lg border border-border bg-card">
      {entries.map((entry) => (
        <div key={entry.pluginKey}>
          <PluginCenterEntryRow
            entry={entry}
            onToggle={handleToggle}
            onOpenLogs={handleOpenLogs}
            onRemove={handleRemove}
          />
          {openLogsKey === entry.pluginKey ? (
            <PluginLogsPanel state={logsByPlugin[entry.pluginKey]} />
          ) : null}
        </div>
      ))}
    </div>
  )
}
