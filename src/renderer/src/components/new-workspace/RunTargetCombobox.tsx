import React, { useCallback, useMemo, useState } from 'react'
import { Popover, PopoverContent } from '@/components/ui/popover'
import { cn } from '@/lib/utils'
import { translate } from '@/i18n/i18n'
import type {
  NeedsSetupProjectHostOption,
  ProjectHostSetupOption
} from '@/lib/project-host-setup-options'
import { isWithinComboboxRoot, useTypeAheadCombobox } from './use-type-ahead-combobox'
import { COMBOBOX_POPOVER_SURFACE } from './type-ahead-combobox-styles'
import {
  ConnectHostButton,
  HostRowIcon,
  NeedsSetupHostIcon,
  RunTargetRow,
  SetLocationButton
} from './RunTargetComboboxRow'
import {
  buildRunTargetRows,
  getEphemeralVmLabel,
  getRecipeDetail,
  type EphemeralVmRecipeOption
} from './run-target-options'
import { RecipesSubmenuRow } from './RunTargetSubmenus'
import RunTargetField from './RunTargetField'

type RunTargetComboboxProps = {
  hostOptions: readonly ProjectHostSetupOption[]
  hostValue: string | null
  onHostChange?: (setupId: string) => void
  recipes: EphemeralVmRecipeOption[]
  recipeValue: string | null
  onRecipeChange?: (recipeId: string | null) => void
  onConnectHost?: (option: NeedsSetupProjectHostOption) => Promise<void> | void
  onSetLocation?: (option: NeedsSetupProjectHostOption) => void
}

const ROOT_ATTRIBUTE = 'data-run-target-combobox-root'

/**
 * Run-target picker, built to match the project picker: the field *is* the
 * search, exactly one row is armed and Enter takes it, and hovering arms.
 *
 * Two things the project picker doesn't have: disconnected hosts carry an
 * inline Connect action that must not select the row, and the VM recipes row
 * opens a nested list rather than committing.
 */
export default function RunTargetCombobox({
  hostOptions,
  hostValue,
  onHostChange,
  recipes,
  recipeValue,
  onRecipeChange,
  onConnectHost,
  onSetLocation
}: RunTargetComboboxProps): React.JSX.Element {
  const [submenu, setSubmenu] = useState<'recipes' | null>(null)
  // Track in-flight connects per host so one stalling connect never blocks the others.
  const [connectingHostIds, setConnectingHostIds] = useState<ReadonlySet<string>>(() => new Set())

  const deriveRowKeys = useCallback(
    (query: string): string[] =>
      buildRunTargetRows({ hostOptions, recipes, query }).rows.map((row) => row.key),
    [hostOptions, recipes]
  )
  const combobox = useTypeAheadCombobox(deriveRowKeys)
  const { query, setQuery, open, setOpen, armedKey, arm, moveArm, inputRef, listId, setListNode } =
    combobox

  const { rows, matchedRecipes } = useMemo(
    () => buildRunTargetRows({ hostOptions, recipes, query }),
    [hostOptions, query, recipes]
  )
  const readyHostOptions = useMemo(
    () => hostOptions.filter((option) => option.kind === 'ready'),
    [hostOptions]
  )
  const selectedHost =
    readyHostOptions.find((option) => option.id === hostValue) ?? readyHostOptions[0] ?? null
  const selectedRecipe = recipes.find((recipe) => recipe.id === recipeValue) ?? null
  const armedRow = rows.find((row) => row.key === armedKey) ?? rows[0] ?? null
  // Only a committed selection paints the field; typing replaces it.
  const committed = query.length === 0 && (selectedRecipe !== null || selectedHost !== null)

  // Closing also drops any open submenu, which the shared hook doesn't know about.
  const close = useCallback((): void => {
    combobox.close()
    setSubmenu(null)
  }, [combobox])

  const selectHost = useCallback(
    (setupId: string): void => {
      onHostChange?.(setupId)
      onRecipeChange?.(null)
      close()
    },
    [close, onHostChange, onRecipeChange]
  )

  const selectRecipe = useCallback(
    (recipeId: string): void => {
      onRecipeChange?.(recipeId)
      close()
    },
    [close, onRecipeChange]
  )

  const connectHost = useCallback(
    async (option: NeedsSetupProjectHostOption): Promise<void> => {
      if (!option.connectAction || !onConnectHost || connectingHostIds.has(option.hostId)) {
        return
      }
      setConnectingHostIds((current) => new Set(current).add(option.hostId))
      try {
        await onConnectHost(option)
      } finally {
        // Always clear when the connect settles (success, failure, or timeout)
        // so the row's spinner stops.
        setConnectingHostIds((current) => {
          if (!current.has(option.hostId)) {
            return current
          }
          const next = new Set(current)
          next.delete(option.hostId)
          return next
        })
      }
    },
    [connectingHostIds, onConnectHost]
  )

  // Why: reached from the row body, its inline button, and Enter — keep one path
  // so all three close the picker before handing off to the nested dialog.
  const setLocation = useCallback(
    (option: NeedsSetupProjectHostOption): void => {
      if (!option.canSetLocation || !onSetLocation) {
        return
      }
      close()
      onSetLocation(option)
    },
    [close, onSetLocation]
  )

  /** Commits a row, or opens its submenu when the row is a submenu row. */
  const activate = useCallback(
    (key: string | null): void => {
      const row = rows.find((candidate) => candidate.key === key)
      if (!row) {
        return
      }
      if (row.kind === 'ready') {
        selectHost(row.option.id)
        return
      }
      if (row.kind === 'needs-setup') {
        // Not ready: setting the location is the only way forward from the row itself.
        setLocation(row.option)
        return
      }
      setSubmenu('recipes')
    },
    [rows, selectHost, setLocation]
  )

  const handleKeyDown = useCallback(
    (event: React.KeyboardEvent<HTMLInputElement>): void => {
      if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
        event.preventDefault()
        setOpen(true)
        moveArm(event.key === 'ArrowDown' ? 1 : -1)
        setSubmenu(null)
        return
      }
      if ((event.key === 'Enter' || event.key === 'ArrowRight') && open) {
        event.preventDefault()
        activate(armedRow?.key ?? null)
        return
      }
      if (event.key === 'Escape' && (open || query.length > 0)) {
        event.preventDefault()
        event.stopPropagation()
        // A submenu closes first, so Escape backs out one layer at a time.
        if (submenu !== null) {
          setSubmenu(null)
          return
        }
        close()
      }
    },
    [activate, armedRow, close, moveArm, open, query, setOpen, submenu]
  )

  const handleOpenChange = useCallback(
    (next: boolean): void => {
      if (next) {
        setOpen(true)
        return
      }
      close()
    },
    [close, setOpen]
  )

  const fieldLabel = selectedRecipe
    ? `${getEphemeralVmLabel()} / ${selectedRecipe.name}`
    : (selectedHost?.label ?? '')
  const fieldDetail = selectedRecipe ? getRecipeDetail(selectedRecipe) : (selectedHost?.path ?? '')

  return (
    <Popover open={open} onOpenChange={handleOpenChange}>
      <RunTargetField
        query={query}
        onQueryChange={(value) => {
          setQuery(value)
          setOpen(true)
          setSubmenu(null)
        }}
        open={open}
        onOpenRequest={() => setOpen(true)}
        onToggle={() => setOpen(!open)}
        committed={committed}
        isRecipe={selectedRecipe !== null}
        hostId={selectedHost?.hostId ?? null}
        label={fieldLabel}
        detail={fieldDetail}
        listId={listId}
        hasArmedRow={armedRow !== null}
        inputRef={inputRef}
        onKeyDown={handleKeyDown}
      />
      <PopoverContent
        align="start"
        sideOffset={4}
        className={cn(
          'flex w-[var(--radix-popover-trigger-width)] min-w-[18rem] flex-col p-0',
          COMBOBOX_POPOVER_SURFACE
        )}
        onOpenAutoFocus={(event) => event.preventDefault()}
        onCloseAutoFocus={(event) => event.preventDefault()}
        // The field lives in the anchor, not the content, so Radix would see a
        // focus/pointer event "outside" and dismiss the instant you tab in.
        onFocusOutside={(event) => {
          if (isWithinComboboxRoot(event.target, ROOT_ATTRIBUTE)) {
            event.preventDefault()
          }
        }}
        onInteractOutside={(event) => {
          if (isWithinComboboxRoot(event.target, ROOT_ATTRIBUTE)) {
            event.preventDefault()
          }
        }}
      >
        <div
          id={listId}
          role="listbox"
          aria-label={translate(
            'auto.components.new.workspace.RunTargetCombobox.listLabel',
            'Run targets'
          )}
          className="flex min-h-0 flex-col"
        >
          <div
            ref={setListNode}
            role="presentation"
            className="max-h-72 min-h-0 flex-1 overflow-y-auto p-1 scrollbar-sleek"
          >
            {rows.length === 0 ? (
              <p className="flex h-8 items-center justify-center px-2 text-sm text-muted-foreground">
                {translate(
                  'auto.components.NewWorkspaceComposerCard.noRunTargets',
                  'No run targets are ready for this project.'
                )}
              </p>
            ) : null}
            {rows.map((row) => {
              const isArmed = armedRow?.key === row.key
              const optionId = isArmed ? `${listId}-armed` : undefined
              if (row.kind === 'ready') {
                return (
                  <RunTargetRow
                    key={row.key}
                    icon={<HostRowIcon hostId={row.option.hostId} />}
                    label={row.option.label}
                    detail={row.option.path}
                    armed={isArmed}
                    current={selectedRecipe === null && row.option.id === selectedHost?.id}
                    optionId={optionId}
                    onArm={() => {
                      arm(row.key)
                      setSubmenu(null)
                    }}
                    onCommit={() => selectHost(row.option.id)}
                  />
                )
              }
              if (row.kind === 'needs-setup') {
                const connecting = connectingHostIds.has(row.option.hostId)
                const hasConnect = Boolean(row.option.connectAction && onConnectHost)
                const hasSetLocation = Boolean(row.option.canSetLocation && onSetLocation)
                return (
                  <RunTargetRow
                    key={row.key}
                    icon={
                      <NeedsSetupHostIcon
                        hostId={row.option.hostId}
                        connecting={connecting}
                        attention={row.option.attention}
                      />
                    }
                    label={row.option.label}
                    // Why: Connect / Set project location already say the next
                    // step, so the detail line would only repeat that.
                    detail={hasConnect || hasSetLocation ? '' : row.option.detail}
                    armed={isArmed}
                    current={false}
                    dimmed
                    optionId={optionId}
                    onArm={() => {
                      arm(row.key)
                      setSubmenu(null)
                    }}
                    onCommit={() => setLocation(row.option)}
                    trailing={
                      hasConnect ? (
                        <ConnectHostButton
                          connecting={connecting}
                          onConnect={() => void connectHost(row.option)}
                        />
                      ) : hasSetLocation ? (
                        <SetLocationButton
                          hostLabel={row.option.label}
                          onSetLocation={() => setLocation(row.option)}
                        />
                      ) : undefined
                    }
                  />
                )
              }
              // Recipes submenu row.
              return (
                <RecipesSubmenuRow
                  key={row.key}
                  open={submenu === 'recipes'}
                  onOpenChange={(next) => setSubmenu(next ? 'recipes' : null)}
                  armed={isArmed}
                  optionId={optionId}
                  recipes={matchedRecipes}
                  selectedRecipeId={selectedRecipe?.id ?? null}
                  onArm={() => {
                    arm(row.key)
                    setSubmenu('recipes')
                  }}
                  onSelectRecipe={selectRecipe}
                />
              )
            })}
          </div>
        </div>
      </PopoverContent>
    </Popover>
  )
}
