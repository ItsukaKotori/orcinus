import { useEffect, useMemo, useState } from 'react'
import type { HostBridge } from '../bridge'
import { reducePush, type UiState } from '../state'
import { Button, Chip, Skeleton } from '../kit'
import type { UpdateRow, UpdaterState } from '../../shared/types'

function kindLabel(row: UpdateRow): string {
  if (row.manager === 'npm') {
    return 'npm'
  }
  return row.kind === 'cask' ? 'brew cask' : 'brew formula'
}

function formatCheckedAt(checkedAt: number | null): string {
  if (checkedAt === null) {
    return 'not checked'
  }
  const seconds = Math.max(0, Math.round((Date.now() - checkedAt) / 1000))
  return seconds < 60 ? `checked ${seconds}s ago` : `checked ${Math.round(seconds / 60)} min ago`
}

function stateLabel(row: UpdateRow): { text: string; className: string } {
  switch (row.state) {
    case 'updated':
      return { text: '✓ updated', className: 'text-foreground' }
    case 'running':
      return { text: 'running…', className: 'text-foreground' }
    case 'failed':
      return { text: '✗ failed', className: 'text-destructive' }
    case 'skipped':
      return { text: 'skipped', className: 'text-muted-foreground' }
    default:
      return { text: 'pending', className: 'text-muted-foreground' }
  }
}

export function TabApp({ bridge }: { bridge: HostBridge }) {
  const [ui, setUi] = useState<UiState>({ report: null, logs: [] })
  const [connected, setConnected] = useState(false)
  const [filter, setFilter] = useState<'all' | string>('all')
  const [actionError, setActionError] = useState<string | null>(null)

  useEffect(() => {
    void bridge.ready.then(() => {
      setConnected(true)
      bridge.onPush((message) => setUi((current) => reducePush(current, message)))
      void bridge.invokeCommand('check')
    })
  }, [bridge])

  const report = ui.report
  const running = report?.phase === 'checking' || report?.phase === 'updating'
  const pendingCount = report?.rows.filter((row) => row.state === 'pending').length ?? 0
  const managers = (report?.managers ?? []).filter((manager) => manager.enabled)

  const rows = useMemo(() => {
    const list = report?.rows ?? []
    return filter === 'all' ? list : list.filter((row) => row.manager === filter)
  }, [report, filter])

  const invoke = async (commandId: string) => {
    const result = await bridge.invokeCommand(commandId)
    setActionError(result.ok ? null : (result.error ?? result.errorCode ?? 'command failed'))
  }

  return (
    <div className="flex h-full min-h-0 flex-col">
      <header className="flex items-center gap-2 border-b border-border px-3.5 py-2.5">
        <span className="text-[13px] font-semibold">Updates</span>
        <span className="text-xs text-muted-foreground">{formatCheckedAt(report?.checkedAt ?? null)}</span>
        <div className="flex-1" />
        <Button disabled={running || !connected} onClick={() => void invoke('check')}>
          Refresh
        </Button>
        {running ? (
          <Button variant="danger" onClick={() => void invoke('cancel')}>
            Cancel
          </Button>
        ) : (
          <Button variant="primary" disabled={!connected || pendingCount === 0} onClick={() => void invoke('update-all')}>
            {`Update all (${pendingCount})`}
          </Button>
        )}
      </header>
      {actionError ? (
        <div className="border-b border-destructive/40 bg-destructive/10 px-3.5 py-1.5 text-xs text-destructive">
          {actionError}
        </div>
      ) : null}
      <div className="flex min-h-0 flex-1">
        <div className="flex min-h-0 min-w-0 flex-1 flex-col">
          <div className="flex items-center gap-1.5 border-b border-border px-3.5 py-2">
            <Chip selected={filter === 'all'} onClick={() => setFilter('all')}>
              {`All ${report?.rows.length ?? 0}`}
            </Chip>
            {managers.map((manager) => (
              <Chip key={manager.id} selected={filter === manager.id} onClick={() => setFilter(manager.id)}>
                {`${manager.label} ${manager.count ?? '—'}`}
              </Chip>
            ))}
          </div>
          <div className="min-h-0 flex-1 overflow-auto">{renderBody(report, rows, connected)}</div>
        </div>
        <aside className="flex w-[300px] shrink-0 flex-col border-l border-border">
          <div className="border-b border-border px-3.5 py-1.5 text-[11px] uppercase tracking-wide text-muted-foreground">
            Live output
          </div>
          <pre className="min-h-0 flex-1 overflow-auto px-3.5 py-2 font-mono text-[11px] leading-relaxed text-muted-foreground">
            {ui.logs.length > 0 ? ui.logs.join('\n') : 'No output yet.'}
          </pre>
        </aside>
      </div>
    </div>
  )
}

function renderBody(report: UpdaterState | null, rows: UpdateRow[], connected: boolean) {
  if (!connected) {
    return <div className="p-10 text-center text-xs text-muted-foreground">Connecting…</div>
  }
  if (report === null || report.phase === 'checking') {
    return (
      <div className="space-y-2 p-3.5">
        {[0, 1, 2, 3].map((key) => (
          <Skeleton key={key} className="h-5" />
        ))}
      </div>
    )
  }
  if (report.phase === 'failed') {
    return <div className="p-3.5 text-xs text-destructive">{report.error ?? 'Check failed.'}</div>
  }
  if (rows.length === 0) {
    return <div className="p-10 text-center text-xs text-muted-foreground">All up to date</div>
  }
  return (
    <div>
      {report.summary ? (
        <div className="border-b border-border px-3.5 py-1.5 text-xs text-muted-foreground">
          {`✓ ${report.summary.updated} updated · ✗ ${report.summary.failed} failed · ${report.summary.skipped} skipped`}
        </div>
      ) : null}
      <table className="w-full border-collapse text-[12.5px]">
        <thead>
          <tr className="bg-muted text-left text-[11px] uppercase tracking-wide text-muted-foreground">
            <th className="px-3.5 py-1.5 font-semibold">package</th>
            <th className="px-3.5 py-1.5 font-semibold">source</th>
            <th className="px-3.5 py-1.5 font-semibold">version</th>
            <th className="px-3.5 py-1.5 text-right font-semibold">state</th>
          </tr>
        </thead>
        <tbody>
          {rows.map((row) => {
            const label = stateLabel(row)
            return (
              <tr key={`${row.manager}:${row.kind}:${row.name}`} className="border-b border-border">
                <td className="px-3.5 py-1.5">{row.name}</td>
                <td className="px-3.5 py-1.5 text-[11px] text-muted-foreground">{kindLabel(row)}</td>
                <td className="px-3.5 py-1.5 text-muted-foreground [font-variant-numeric:tabular-nums]">
                  {row.current} → <span className="font-medium text-foreground">{row.latest}</span>
                </td>
                <td className={`px-3.5 py-1.5 text-right text-[11px] ${label.className}`}>{label.text}</td>
              </tr>
            )
          })}
        </tbody>
      </table>
    </div>
  )
}
