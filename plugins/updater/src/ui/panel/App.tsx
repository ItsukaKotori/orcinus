import { useEffect, useState } from 'react'
import type { HostBridge } from '../bridge'
import { Button, Toggle } from '../kit'
import type { ManagerState } from '../../shared/types'

function managerSummary(manager: ManagerState): string {
  if (!manager.detected) {
    return 'not installed'
  }
  const scope = manager.id === 'npm' ? 'global' : 'formula · cask'
  const count = manager.count === null ? '—' : `${manager.count} updates`
  return `${scope} · ${count}`
}

function formatCheckedAt(checkedAt: number | null): string {
  if (checkedAt === null) {
    return 'not checked'
  }
  const seconds = Math.max(0, Math.round((Date.now() - checkedAt) / 1000))
  return seconds < 60 ? `${seconds}s ago` : `${Math.round(seconds / 60)} min ago`
}

export function PanelApp({ bridge }: { bridge: HostBridge }) {
  const [managers, setManagers] = useState<ManagerState[]>([])
  const [checkedAt, setCheckedAt] = useState<number | null>(null)
  const [connected, setConnected] = useState(false)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    void bridge.ready.then(() => {
      setConnected(true)
      bridge.onPush((message) => {
        if (message.topic !== 'state') {
          return
        }
        const state = message.payload as { managers: ManagerState[]; checkedAt: number | null }
        setManagers(state.managers)
        setCheckedAt(state.checkedAt)
      })
      void bridge
        .invokeCommand('snapshot')
        .then((result) => {
          if (!result.ok) {
            setError(result.error ?? result.errorCode ?? 'snapshot failed')
            return
          }
          const snapshot = result.value as { managers: ManagerState[]; state: { checkedAt: number | null } }
          setManagers(snapshot.managers)
          setCheckedAt(snapshot.state.checkedAt)
        })
    })
  }, [bridge])

  const toggle = async (manager: ManagerState, next: boolean) => {
    const previous = managers
    const nextIds = managers
      .filter((item) => (item.id === manager.id ? next : item.enabled))
      .map((item) => item.id)
    setManagers((current) =>
      current.map((item) => (item.id === manager.id ? { ...item, enabled: next } : item))
    )
    const result = await bridge.invokeCommand('set-managers', { managers: nextIds })
    if (result.ok) {
      setError(null)
    } else {
      setManagers(previous)
      setError(result.error ?? result.errorCode ?? 'could not save')
    }
  }

  return (
    <div className="flex h-full min-h-0 flex-col">
      <header className="flex items-center gap-2 border-b border-border px-3 py-2.5">
        <span className="text-[13px] font-semibold">Package Updater</span>
        <div className="flex-1" />
        <span className="text-xs text-muted-foreground">{formatCheckedAt(checkedAt)}</span>
      </header>
      <div className="flex gap-2 border-b border-border px-3 py-2.5">
        <Button disabled={!connected} onClick={() => void bridge.invokeCommand('check')}>
          Check now
        </Button>
        <Button disabled={!connected} onClick={() => void bridge.invokeCommand('open')}>
          Open tab
        </Button>
      </div>
      {error ? (
        <div className="border-b border-destructive/40 bg-destructive/10 px-3 py-1.5 text-xs text-destructive">
          {error}
        </div>
      ) : null}
      <div className="min-h-0 flex-1 overflow-auto">
        {managers.map((manager) => (
          <div key={manager.id} className="flex items-center gap-2 border-b border-border px-3 py-2">
            <span
              className={`h-1.5 w-1.5 rounded-full ${manager.detected ? 'bg-foreground' : 'bg-muted-foreground'}`}
            />
            <div className="min-w-0 flex-1">
              <div className="text-[12.5px] font-medium">{manager.label}</div>
              <div className="text-[11px] text-muted-foreground">{managerSummary(manager)}</div>
            </div>
            <Toggle checked={manager.enabled} onChange={(next) => void toggle(manager, next)} label={manager.label} />
          </div>
        ))}
      </div>
      <div className="border-t border-border px-3 py-2 text-[11px] text-muted-foreground">
        No recent check. The tab checks automatically when opened.
      </div>
    </div>
  )
}
