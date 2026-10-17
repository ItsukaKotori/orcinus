import React from 'react'
import { Plus } from 'lucide-react'
import { CommandItem } from '@/components/ui/command'
import { CREATE_WORKTREE_ITEM_ID } from '@/lib/worktree-palette-create-action'
import { translate } from '@/i18n/i18n'

export function PaletteCreateWorktreeRow({
  className,
  createWorktreeName,
  onSelect
}: {
  className: string
  createWorktreeName: string
  onSelect: () => void
}): React.JSX.Element {
  return (
    <CommandItem
      value={CREATE_WORKTREE_ITEM_ID}
      onSelect={onSelect}
      className={className}
    >
      <div className="flex h-5 w-5 shrink-0 items-center justify-center rounded-full border border-dashed border-border/60 bg-muted/25 text-muted-foreground/70">
        <Plus size={13} aria-hidden="true" />
      </div>
      <div className="min-w-0 flex-1">
        <div className="text-[14px] font-semibold tracking-[-0.01em] text-foreground">
          {translate(
            'auto.components.WorktreeJumpPalette.95be6587d3',
            'Create worktree "{{value0}}"',
            { value0: createWorktreeName }
          )}
        </div>
      </div>
    </CommandItem>
  )
}
