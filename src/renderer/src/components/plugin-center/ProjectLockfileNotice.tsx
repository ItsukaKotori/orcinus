import { useEffect, useState } from 'react'
import { AlertTriangle, ChevronRight } from 'lucide-react'
import type {
  PluginProjectLockEntry,
  PluginProjectLockState,
  PluginProjectLockStatus
} from '../../../../preload/api-types'
import type { PluginInstallSource } from '../../../../shared/plugins/plugin-install-lockfile'
import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle
} from '@/components/ui/dialog'
import { useAppStore } from '@/store'
import { translate } from '@/i18n/i18n'

const LOCK_STATUS_PRESENTATION: Record<
  PluginProjectLockStatus,
  { label: string; className: string }
> = {
  installed: { label: '已安装', className: 'text-muted-foreground' },
  missing: { label: '缺失', className: 'text-amber-600 dark:text-amber-400' },
  'hash-mismatch': { label: '哈希不符', className: 'text-amber-600 dark:text-amber-400' },
  'source-unavailable': { label: '来源不可用', className: 'text-destructive' }
}

function lockStatusPresentation(status: PluginProjectLockStatus): {
  label: string
  className: string
} {
  return LOCK_STATUS_PRESENTATION[status]
}

function describeInstallSource(source: PluginInstallSource): string {
  switch (source.kind) {
    case 'local-path':
      return `本地路径 ${source.path}`
    case 'git':
      return source.ref ? `Git ${source.url}#${source.ref}` : `Git ${source.url}`
    case 'marketplace':
      return `市场 ${source.marketplace.url}`
    case 'bundled':
      return `内置 ${source.bundleId}`
  }
}

function isInstallable(entry: PluginProjectLockEntry): boolean {
  return entry.status === 'missing' || entry.status === 'hash-mismatch'
}

type ProjectLockfileNoticeProps = {
  /** Called after a batch install so the host surface can reload plugin entries. */
  onChanged?: () => void
}

/**
 * Compact banner shown on the plugin center installed tab when the active
 * project's lockfile has entries that are not ready. The dialog lists every
 * entry with its classified status and offers the one-click batch install.
 */
export function ProjectLockfileNotice({
  onChanged
}: ProjectLockfileNoticeProps): React.JSX.Element | null {
  const repoId = useAppStore((s) => s.activeRepoId)
  const [lockState, setLockState] = useState<PluginProjectLockState | null>(null)
  const [dialogOpen, setDialogOpen] = useState(false)
  const [installing, setInstalling] = useState(false)

  useEffect(() => {
    if (!repoId) {
      setLockState(null)
      return
    }
    let cancelled = false
    window.api.plugins
      .getProjectLockState({ repoId })
      .then((next) => {
        if (!cancelled) {
          setLockState(next)
        }
      })
      .catch((cause: unknown) => {
        console.warn('[plugin-center] failed to load project lockfile state:', cause)
        if (!cancelled) {
          setLockState(null)
        }
      })
    return () => {
      cancelled = true
    }
  }, [repoId])

  const unreadyEntries = lockState?.entries.filter((entry) => entry.status !== 'installed') ?? []
  if (!repoId || !lockState || unreadyEntries.length === 0) {
    return null
  }

  const installableCount = lockState.entries.filter(isInstallable).length

  const handleInstall = async (): Promise<void> => {
    setInstalling(true)
    try {
      await window.api.plugins.installMissingFromLockfile({ repoId })
    } catch (cause: unknown) {
      console.warn('[plugin-center] failed to install missing lockfile entries:', cause)
    }
    try {
      onChanged?.()
      setLockState(await window.api.plugins.getProjectLockState({ repoId }))
    } catch (cause: unknown) {
      console.warn('[plugin-center] failed to refresh project lockfile state:', cause)
    } finally {
      setInstalling(false)
    }
  }

  return (
    <>
      <button
        type="button"
        onClick={() => setDialogOpen(true)}
        className="flex w-full shrink-0 items-center gap-2 rounded-lg border border-amber-500/40 bg-amber-500/10 px-4 py-2.5 text-left text-[13px] text-foreground transition-colors hover:bg-amber-500/15"
      >
        <AlertTriangle className="size-4 shrink-0 text-amber-600 dark:text-amber-400" />
        <span>
          {translate(
            'auto.components.pluginCenter.ProjectLockfileNotice.summary',
            '{{missing}} 项未就绪',
            { missing: unreadyEntries.length }
          )}
        </span>
        <ChevronRight className="ml-auto size-4 shrink-0 text-muted-foreground" />
      </button>
      <Dialog open={dialogOpen} onOpenChange={setDialogOpen}>
        <DialogContent className="sm:max-w-xl">
          <DialogHeader>
            <DialogTitle>
              {translate(
                'auto.components.pluginCenter.ProjectLockfileNotice.title',
                '当前项目 lockfile 未就绪条目'
              )}
            </DialogTitle>
            <DialogDescription>
              {translate(
                'auto.components.pluginCenter.ProjectLockfileNotice.description',
                '以下条目与 lockfile 记录不一致，可一键重新安装缺失或哈希不符的插件。'
              )}
            </DialogDescription>
          </DialogHeader>
          <ul className="max-h-[50vh] overflow-y-auto scrollbar-sleek">
            {lockState.entries.map((entry) => {
              const presentation = lockStatusPresentation(entry.status)
              return (
                <li
                  key={entry.pluginKey}
                  className="flex items-start justify-between gap-3 border-b border-border py-2.5 last:border-b-0"
                >
                  <div className="min-w-0">
                    <p className="truncate text-sm font-medium text-foreground">
                      {entry.pluginKey}{' '}
                      <span className="font-normal text-muted-foreground">v{entry.version}</span>
                    </p>
                    <p className="truncate text-xs text-muted-foreground">
                      {describeInstallSource(entry.source)}
                    </p>
                    {entry.detail ? (
                      <p className="mt-0.5 text-xs text-muted-foreground">{entry.detail}</p>
                    ) : null}
                  </div>
                  <span className={`shrink-0 text-xs ${presentation.className}`}>
                    {presentation.label}
                  </span>
                </li>
              )
            })}
          </ul>
          <DialogFooter>
            <Button variant="outline" onClick={() => setDialogOpen(false)}>
              {translate('auto.components.pluginCenter.ProjectLockfileNotice.close', '关闭')}
            </Button>
            <Button
              disabled={installing || installableCount === 0}
              onClick={() => void handleInstall()}
            >
              {installing
                ? translate(
                    'auto.components.pluginCenter.ProjectLockfileNotice.installing',
                    '正在安装…'
                  )
                : translate(
                    'auto.components.pluginCenter.ProjectLockfileNotice.installMissing',
                    '安装缺失'
                  )}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </>
  )
}
