import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Button } from '@/components/ui/button'
import { translate } from '@/i18n/i18n'

export type DeleteConfirmDialogProps = {
  open: boolean
  branchName: string
  /** 未合入 HEAD 的提交数（来自后端 NotMergeable.unmergedCount）。 */
  unmergedCount: number
  onCancel: () => void
  onForceDelete: () => void
}

/**
 * Delete 的强制二次确认框（增量 4，§7.4）。只在 `git branch -d` 回 NotMergeable 时弹出：
 * 文案带分支名 + 未合并提交数，确认后走 `-D`。刻意**不加**「输入分支名」式摩擦（spec §7.4）。
 */
export function DeleteConfirmDialog({
  open,
  branchName,
  unmergedCount,
  onCancel,
  onForceDelete,
}: DeleteConfirmDialogProps): React.JSX.Element {
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
            {translate('auto.components.git-branch.deleteConfirmTitle', 'Delete branch')}
          </DialogTitle>
          <DialogDescription className="text-xs">
            {translate(
              'auto.components.git-branch.deleteConfirmUnmerged',
              'Branch "{{branch}}" has {{count}} unmerged commit(s). Force delete it?',
              { branch: branchName, count: unmergedCount }
            )}
          </DialogDescription>
        </DialogHeader>
        <DialogFooter>
          <Button type="button" variant="outline" onClick={onCancel}>
            {translate('auto.components.git-branch.checkoutCancel', 'Cancel')}
          </Button>
          <Button type="button" variant="destructive" onClick={onForceDelete}>
            {translate('auto.components.git-branch.deleteForce', 'Force delete')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
