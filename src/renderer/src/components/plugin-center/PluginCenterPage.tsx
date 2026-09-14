import { useEffect, useState } from 'react'
import type { PluginMarketplaceHostSourceState } from '../../../../preload/api-types'
import { useAppStore } from '@/store'
import { PluginCenterInstalledList } from './PluginCenterInstalledList'
import { PluginCenterTabs, type PluginCenterTab } from './PluginCenterTabs'

function PluginCenterEmptyState({
  title,
  hint
}: {
  title: string
  hint: string
}): React.JSX.Element {
  return (
    <div className="rounded-lg border border-dashed border-border px-5 py-12 text-center">
      <p className="text-sm text-foreground">{title}</p>
      <p className="mt-1 text-xs text-muted-foreground">{hint}</p>
    </div>
  )
}

function PluginCenterMarketTab(): React.JSX.Element {
  const [status, setStatus] = useState<'loading' | 'ready' | 'error'>('loading')
  const [marketplaces, setMarketplaces] = useState<PluginMarketplaceHostSourceState[]>([])

  useEffect(() => {
    let cancelled = false
    window.api.plugins
      .listMarketplaces()
      .then((sources) => {
        if (!cancelled) {
          setMarketplaces(sources)
          setStatus('ready')
        }
      })
      .catch((cause: unknown) => {
        console.warn('[plugin-center] failed to load marketplaces:', cause)
        if (!cancelled) {
          setStatus('error')
        }
      })
    return () => {
      cancelled = true
    }
  }, [])

  if (status === 'loading') {
    return <PluginCenterEmptyState title="正在加载插件市场…" hint="读取已配置的市场源。" />
  }
  if (status === 'error') {
    return <PluginCenterEmptyState title="插件市场加载失败。" hint="请稍后重试。" />
  }
  if (marketplaces.length === 0) {
    return (
      <PluginCenterEmptyState title="暂无插件市场。" hint="市场源接入后，可在此浏览并安装插件。" />
    )
  }

  return (
    <ul className="divide-y divide-border overflow-hidden rounded-lg border border-border bg-card">
      {marketplaces.map((source) => (
        <li key={source.id} className="flex items-center justify-between gap-3 px-4 py-3">
          <div className="min-w-0">
            <p className="truncate text-sm font-medium text-foreground">
              {source.marketplace?.name ?? source.id}
            </p>
            <p className="truncate text-xs text-muted-foreground">
              {source.marketplace?.owner ?? source.source.url}
            </p>
          </div>
        </li>
      ))}
    </ul>
  )
}

function PluginCenterDevTab(): React.JSX.Element {
  return (
    <PluginCenterEmptyState
      title="暂无开发中的插件。"
      hint="在设置中配置开发插件路径后，将在此显示。"
    />
  )
}

export function PluginCenterPage(): React.JSX.Element {
  const [tab, setTab] = useState<PluginCenterTab>('installed')

  useEffect(() => {
    void useAppStore.getState().loadPluginCenterEntries()
  }, [])

  return (
    <div className="flex h-full min-h-0 flex-col bg-background">
      <div className="flex shrink-0 items-center gap-3 border-b border-border px-5 py-3">
        <h1 className="text-base font-semibold text-foreground">插件中心</h1>
      </div>
      <div className="flex flex-1 min-h-0 flex-col gap-4 overflow-y-auto p-5 scrollbar-sleek">
        <PluginCenterTabs value={tab} onValueChange={setTab} />
        {tab === 'installed' ? <PluginCenterInstalledList /> : null}
        {tab === 'market' ? <PluginCenterMarketTab /> : null}
        {tab === 'dev' ? <PluginCenterDevTab /> : null}
      </div>
    </div>
  )
}
