import { useState } from 'react'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { translate } from '@/i18n/i18n'

export type NewBranchDialogProps = {
  open: boolean
  /** start-point 分支的显示名（提示"从哪来"）。 */
  startPointLabel: string
  onCancel: () => void
  onCreate: (name: string) => void
}

/**
 * 「New Branch from…」的名字输入框（增量 3）。形状照 ProjectGroupNameDialog / discard-dialog：
 * 受控 Input + 空名禁用创建。真正的合法性校验在提交时交给后端 `check-ref-format`
 * （`git_branch_create` 的 NameTaken / 非法名会以 i18n 文案提示，故此处只做非空 gate）。
 */
export function NewBranchDialog({
  open,
  startPointLabel,
  onCancel,
  onCreate,
}: NewBranchDialogProps): React.JSX.Element {
  const [name, setName] = useState('')
  const trimmed = name.trim()
  const submit = (): void => {
    if (trimmed) onCreate(trimmed)
  }

  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        if (!next) {
          setName('')
          onCancel()
        }
      }}
    >
      <DialogContent className="max-w-sm">
        <DialogHeader>
          <DialogTitle className="text-sm">
            {translate('auto.components.git-branch.newBranchDialogTitle', 'New Branch')}
          </DialogTitle>
          <DialogDescription className="text-xs">
            {translate(
              'auto.components.git-branch.newBranchFromStartPoint',
              'Create a new branch from {{ref}}',
              { ref: startPointLabel }
            )}
          </DialogDescription>
        </DialogHeader>
        <div className="space-y-3">
          <Input
            autoFocus
            data-testid="new-branch-name-input"
            value={name}
            onChange={(event) => setName(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === 'Enter') {
                event.preventDefault()
                submit()
              }
            }}
            placeholder={translate(
              'auto.components.git-branch.newBranchNamePlaceholder',
              'New branch name'
            )}
          />
          <DialogFooter>
            <Button
              type="button"
              variant="outline"
              onClick={() => {
                setName('')
                onCancel()
              }}
            >
              {translate('auto.components.git-branch.checkoutCancel', 'Cancel')}
            </Button>
            <Button type="button" disabled={!trimmed} onClick={submit}>
              {translate('auto.components.git-branch.newBranchCreate', 'Create branch')}
            </Button>
          </DialogFooter>
        </div>
      </DialogContent>
    </Dialog>
  )
}
