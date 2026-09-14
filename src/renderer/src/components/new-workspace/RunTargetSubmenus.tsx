import React from 'react'
import { Cloud } from 'lucide-react'
import { Popover, PopoverAnchor, PopoverContent } from '@/components/ui/popover'
import { cn } from '@/lib/utils'
import { translate } from '@/i18n/i18n'
import { RunTargetRow } from './RunTargetComboboxRow'
import {
  getEphemeralVmLabel,
  getRecipeDetail,
  type EphemeralVmRecipeOption
} from './run-target-options'
import { COMBOBOX_POPOVER_SURFACE } from './type-ahead-combobox-styles'

const SUBMENU_CONTENT = cn('w-72 p-1', COMBOBOX_POPOVER_SURFACE)

/** The "Per-Workspace Environment" row and its nested recipe list. */
export function RecipesSubmenuRow({
  open,
  onOpenChange,
  armed,
  optionId,
  recipes,
  selectedRecipeId,
  onArm,
  onSelectRecipe
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
  armed: boolean
  optionId: string | undefined
  recipes: readonly EphemeralVmRecipeOption[]
  selectedRecipeId: string | null
  onArm: () => void
  onSelectRecipe: (recipeId: string) => void
}): React.JSX.Element {
  const [hoveredKey, setHoveredKey] = React.useState<string | null>(null)
  return (
    <Popover open={open} onOpenChange={onOpenChange}>
      <PopoverAnchor asChild>
        <div>
          <RunTargetRow
            icon={<Cloud className="size-3.5 shrink-0 text-muted-foreground" />}
            label={getEphemeralVmLabel()}
            detail={translate(
              'auto.components.NewWorkspaceComposerCard.perWorkspaceEnvHint',
              'Provision an on-demand environment from a recipe'
            )}
            armed={armed}
            current={selectedRecipeId !== null}
            optionId={optionId}
            submenu
            onArm={onArm}
            onCommit={() => onOpenChange(true)}
          />
        </div>
      </PopoverAnchor>
      <PopoverContent
        side="right"
        align="start"
        sideOffset={6}
        className={SUBMENU_CONTENT}
        onOpenAutoFocus={(event) => event.preventDefault()}
      >
        {/* Why: submenu rows track their own hover — without it they were the
            only rows in either picker that never highlighted under the pointer. */}
        <div
          role="listbox"
          aria-label={getEphemeralVmLabel()}
          onMouseLeave={() => setHoveredKey(null)}
        >
          {recipes.map((recipe) => (
            <RunTargetRow
              key={recipe.id}
              icon={<Cloud className="size-3.5 shrink-0 text-muted-foreground" />}
              label={recipe.name}
              detail={getRecipeDetail(recipe)}
              armed={hoveredKey === recipe.id}
              current={recipe.id === selectedRecipeId}
              optionId={undefined}
              onArm={() => setHoveredKey(recipe.id)}
              onCommit={() => onSelectRecipe(recipe.id)}
            />
          ))}
        </div>
      </PopoverContent>
    </Popover>
  )
}
