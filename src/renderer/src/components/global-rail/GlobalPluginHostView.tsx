import { useAppStore } from '@/store'

/** Placeholder host surface for a global plugin; the real panel mounts here in Phase 3. */
export function GlobalPluginHostView({
  pluginKey
}: {
  pluginKey: string
}): React.JSX.Element {
  const entry = useAppStore((s) =>
    s.pluginCenterEntries.find((candidate) => candidate.pluginKey === pluginKey)
  )

  return (
    <div className="flex h-full min-h-0 flex-col items-center justify-center gap-2 p-6">
      <h1 className="text-sm font-semibold text-foreground">{entry?.name ?? pluginKey}</h1>
      <p className="text-sm text-muted-foreground">插件面板将在 Phase 3 接入</p>
    </div>
  )
}
