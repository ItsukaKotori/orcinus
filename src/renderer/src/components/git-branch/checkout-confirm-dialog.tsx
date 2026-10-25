import { useRef } from 'react'
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Button } from '@/components/ui/button'
import { translate } from '@/i18n/i18n'
import { focusDiscardDialogConfirmButton } from '../right-sidebar/source-control/commit/discard-dialog'
import type { GitRefEntry } from '../../../../shared/git-ref-types'

/** 脏改动最多列几条路径（spec §13.7「前 K 个路径」；K 取 5）。 */
const MAX_DIRTY_PATHS = 5

export type CheckoutConfirmInput = {
  target: GitRefEntry
  dirtyCount: number
  dirtyPaths: string[]
  sessionCount: number
}

/**
 * 产品决策 1：干净且无终端/会话时不弹框，直接检出；只有「有东西要警告」时才拦一道。
 */
export function needsCheckoutConfirmation(input: {
  dirtyCount: number
  sessionCount: number
}): boolean {
  return input.dirtyCount > 0 || input.sessionCount > 0
}

/**
 * Checkout 前确认框（spec §13.7 三段）：目标 / 脏改动 / 活跃终端会话。
 * 形状照 `discard-dialog.tsx`：`DialogContent` + `onConfirm` / `onCancel` 回调，无内部确认态。
 */
export function CheckoutConfirmDialog({
  input,
  open,
  onConfirm,
  onCancel,
}: {
  input: CheckoutConfirmInput
  open: boolean
  onConfirm: () => void
  onCancel: () => void
}): React.JSX.Element {
  const confirmRef = useRef<HTMLButtonElement>(null)
  const shownPaths = input.dirtyPaths.slice(0, MAX_DIRTY_PATHS)
  // 远程目标的本地分支短名 = 去掉 `<remote>/` 前缀（origin/dev → dev）。
  const localName = input.target.remote
    ? input.target.name.slice(input.target.remote.length + 1)
    : input.target.name

  return (
    <Dialog
      open={open}
      onOpenChange={(nextOpen) => {
        if (!nextOpen) onCancel()
      }}
    >
      <DialogContent
        className="max-w-md"
        onOpenAutoFocus={(event) => focusDiscardDialogConfirmButton(event, confirmRef.current)}
      >
        <DialogHeader>
          <DialogTitle className="text-sm">
            {translate('auto.components.git-branch.checkoutConfirmTitle', 'Checkout branch?')}
          </DialogTitle>
        </DialogHeader>

        {/* 段 1：目标（恒出现） */}
        <div className="space-y-1 text-xs">
          <div>
            {translate('auto.components.git-branch.checkoutTarget', 'Will check out: {{branch}}', {
              branch: input.target.name,
            })}
          </div>
          {input.target.category === 'remote' && (
            <div className="text-muted-foreground">
              {translate(
                'auto.components.git-branch.checkoutCreatesTracking',
                'and create local branch {{name}} tracking {{remote}}',
                { name: localName, remote: input.target.name }
              )}
            </div>
          )}
        </div>

        {/* 段 2：脏改动（有则出现，否则整段不出现） */}
        {input.dirtyCount > 0 && (
          <div className="rounded-md border border-border/70 bg-muted/35 px-3 py-2 text-xs text-muted-foreground">
            <div>
              {translate('auto.components.git-branch.checkoutDirtyCount', '{{count}} uncommitted change(s)', {
                count: input.dirtyCount,
              })}
            </div>
            <ul className="mt-1 list-disc pl-4">
              {shownPaths.map((path) => (
                <li key={path} className="break-all">
                  {path}
                </li>
              ))}
            </ul>
            <div className="mt-1">
              {translate(
                'auto.components.git-branch.checkoutWillCarry',
                'git will carry them to the target branch; if any file would be overwritten, git will refuse this checkout.'
              )}
            </div>
          </div>
        )}

        {/* 段 3：活跃终端/会话（有则出现，仅告知，不阻断；终端+agent 会话合并计数） */}
        {input.sessionCount > 0 && (
          <div className="text-xs text-muted-foreground">
            {translate(
              'auto.components.git-branch.checkoutSessionCount',
              'This workspace has {{count}} running terminal(s)/session(s)',
              { count: input.sessionCount }
            )}
          </div>
        )}

        <DialogFooter>
          <Button type="button" variant="outline" onClick={onCancel}>
            {translate('auto.components.git-branch.checkoutCancel', 'Cancel')}
          </Button>
          <Button ref={confirmRef} type="button" autoFocus onClick={onConfirm}>
            {translate('auto.components.git-branch.checkout', 'Checkout')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
