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

export type RenameDialogProps = {
  open: boolean
  /** 现有分支短名，作为输入初值。 */
  currentName: string
  onCancel: () => void
  onRename: (newName: string) => void
}

/**
 * 「Rename…」的新名输入框（增量 4）。形状照 NewBranchDialog：受控 Input + 空名禁用。
 * 初值预填当前名。合法性（`check-ref-format`、占用、已存在）交给后端 `git_branch_rename`，
 * 其拒绝以 outcome.status 的 i18n 文案提示，故此处只做非空 gate。
 */
export function RenameDialog({
  open,
  currentName,
  onCancel,
  onRename,
}: RenameDialogProps): React.JSX.Element {
  const [name, setName] = useState(currentName)
  const trimmed = name.trim()
  const submit = (): void => {
    if (trimmed) onRename(trimmed)
  }

  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        if (!next) onCancel()
      }}
    >
      <DialogContent className="max-w-sm">
        <DialogHeader>
          <DialogTitle className="text-sm">
            {translate('auto.components.git-branch.renameDialogTitle', 'Rename Branch')}
          </DialogTitle>
          <DialogDescription className="text-xs">
            {translate(
              'auto.components.git-branch.renameFrom',
              'Rename {{ref}} to',
              { ref: currentName }
            )}
          </DialogDescription>
        </DialogHeader>
        <div className="space-y-3">
          <Input
            autoFocus
            data-testid="rename-branch-name-input"
            value={name}
            onChange={(event) => setName(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === 'Enter') {
                event.preventDefault()
                submit()
              }
            }}
            placeholder={translate(
              'auto.components.git-branch.renameNamePlaceholder',
              'New branch name'
            )}
          />
          <DialogFooter>
            <Button type="button" variant="outline" onClick={onCancel}>
              {translate('auto.components.git-branch.checkoutCancel', 'Cancel')}
            </Button>
            <Button type="button" disabled={!trimmed} onClick={submit}>
              {translate('auto.components.git-branch.renameCreate', 'Rename branch')}
            </Button>
          </DialogFooter>
        </div>
      </DialogContent>
    </Dialog>
  )
}
