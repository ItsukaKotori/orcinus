import { useEffect, useState } from 'react'
import { RefreshCw, Settings2 } from 'lucide-react'
import type { PluginHostStatus } from '../../../../preload/api-types'
import { useAppStore } from '@/store'
import { Button } from '../ui/button'
import { Badge } from '../ui/badge'
import { PluginMarketplaceBrowser } from '../settings/PluginMarketplaceBrowser'
import { PluginCenterInstalledList } from './PluginCenterInstalledList'
import { PluginCenterTabs, type PluginCenterTab } from './PluginCenterTabs'
import { ProjectLockfileNotice } from './ProjectLockfileNotice'

const DEV_STATUS_LABELS: Record<PluginHostStatus, string> = {
  running: '运行中',
  restarting: '重启中',
  idle: '已启用',
  pending: '待同意',
  disabled: '已禁用',
  errored: '错误',
  invalid: '无效'
}

function PluginCenterEmptyState({
  title,
  hint,
  action
}: {
  title: string
  hint: string
  action?: React.ReactNode
}): React.JSX.Element {
  return (
    <div className="rounded-lg border border-dashed border-border px-5 py-12 text-center">
      <p className="text-sm text-foreground">{title}</p>
      <p className="mt-1 text-xs text-muted-foreground">{hint}</p>
      {action ? <div className="mt-3 flex justify-center">{action}</div> : null}
    </div>
  )
}

function PluginCenterMarketTab(): React.JSX.Element {
  const entries = useAppStore((s) => s.pluginCenterEntries)
  return (
    <PluginMarketplaceBrowser
      installedPlugins={entries}
      onInstalled={() => useAppStore.getState().loadPluginCenterEntries()}
      onRefreshInstalled={() => useAppStore.getState().loadPluginCenterEntries()}
      renderInstalledContent={() => <PluginCenterInstalledList />}
    />
  )
}

function PluginCenterDevTab(): React.JSX.Element {
  const entries = useAppStore((s) => s.pluginCenterEntries)
  const status = useAppStore((s) => s.pluginCenterStatus)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const devEntries = entries.filter((entry) => entry.isDev)

  const reload = async (): Promise<void> => {
    setBusy(true)
    setError(null)
    try {
      await window.api.plugins.refresh()
      await useAppStore.getState().loadPluginCenterEntries()
    } catch (cause) {
      console.warn('[plugin-center] failed to reload dev plugins:', cause)
      setError('开发插件重新加载失败。')
    } finally {
      setBusy(false)
    }
  }

  const openPluginSettings = (): void => {
    const store = useAppStore.getState()
    store.openSettingsTarget({ pane: 'plugins', repoId: null })
    store.openSettingsPage()
  }

  if (entries.length === 0 && status === 'loading') {
    return <PluginCenterEmptyState title="正在加载开发插件…" hint="读取开发插件路径。" />
  }
  if (entries.length === 0 && status === 'error') {
    return <PluginCenterEmptyState title="开发插件加载失败。" hint="请稍后重试。" />
  }

  return (
    <div className="space-y-3">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <p className="text-xs text-muted-foreground">
          开发插件来自设置中的 devPluginPaths，文件夹变化后会自动重新发现。
        </p>
        <Button variant="outline" size="sm" disabled={busy} onClick={() => void reload()}>
          <RefreshCw className={busy ? 'animate-spin' : undefined} />
          重新加载
        </Button>
      </div>
      {error ? <p className="text-xs text-destructive">{error}</p> : null}
      {devEntries.length === 0 ? (
        <PluginCenterEmptyState
          title="暂无开发中的插件。"
          hint="在「设置 → 插件 → 开发」中配置 devPluginPaths 后，开发插件会显示在这里。"
          action={
            <Button variant="outline" size="sm" onClick={openPluginSettings}>
              <Settings2 />
              打开插件设置
            </Button>
          }
        />
      ) : (
        <div className="divide-y divide-border overflow-hidden rounded-lg border border-border bg-card">
          {devEntries.map((entry) => (
            <div
              key={`${entry.pluginKey}:${entry.scope}:${entry.repoId ?? ''}`}
              data-plugin-key={entry.pluginKey}
              data-plugin-dev="true"
              className="flex items-center justify-between gap-3 px-4 py-3"
            >
              <div className="min-w-0">
                <p className="truncate text-sm font-medium text-foreground">{entry.name}</p>
                <p className="mt-0.5 truncate text-xs text-muted-foreground">
                  {entry.pluginKey} · v{entry.version}
                  {entry.source?.reference ? ` · ${entry.source.reference}` : ''}
                </p>
              </div>
              <Badge variant={entry.status === 'errored' || entry.status === 'invalid' ? 'destructive' : 'secondary'}>
                {DEV_STATUS_LABELS[entry.status]}
              </Badge>
            </div>
          ))}
        </div>
      )}
    </div>
  )
}

export function PluginCenterPage(): React.JSX.Element {
  const [tab, setTab] = useState<PluginCenterTab>('installed')

  useEffect(() => {
    void useAppStore.getState().loadPluginCenterEntries()
    const unsubscribe = window.api.plugins.onChanged(() => {
      void useAppStore.getState().loadPluginCenterEntries()
    })
    return unsubscribe
  }, [])

  return (
    <div className="flex h-full min-h-0 flex-col bg-background">
      <div className="flex shrink-0 items-center gap-3 border-b border-border px-5 py-3">
        <h1 className="text-base font-semibold text-foreground">插件中心</h1>
      </div>
      <div className="flex flex-1 min-h-0 flex-col gap-4 overflow-y-auto p-5 scrollbar-sleek">
        <PluginCenterTabs value={tab} onValueChange={setTab} />
        {tab === 'installed' ? (
          <>
            <ProjectLockfileNotice
              onChanged={() => void useAppStore.getState().loadPluginCenterEntries()}
            />
            <PluginCenterInstalledList />
          </>
        ) : null}
        {tab === 'market' ? <PluginCenterMarketTab /> : null}
        {tab === 'dev' ? <PluginCenterDevTab /> : null}
      </div>
    </div>
  )
}
