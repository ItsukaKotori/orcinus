
import { getActiveOptionId, type ActiveOption } from './tab-create-entry-active-option'

type TabEntrySelectionArgs = {
  activeOptions: ActiveOption[]
  menuOpen: boolean
  pinnedOptionId: string | null
  query: string
}

export function useNetworkSafeTabEntrySelection({
  activeOptions,
  menuOpen,
  pinnedOptionId,
  query
}: TabEntrySelectionArgs): {
  activeSelectedIndex: number | null
  selectedActiveOption: ActiveOption | undefined
} {
  const pinnedOptionIndex = pinnedOptionId
    ? activeOptions.findIndex((option) => getActiveOptionId(option) === pinnedOptionId)
    : -1
  // Why: a still-scanning index can promote a file match later, so keep the
  // ranking keyed by query+open state; there is no network row to defer to anymore.
  const rankingKey = `${menuOpen}:${query}`
  void rankingKey
  const activeSelectedIndex = pinnedOptionIndex >= 0 ? pinnedOptionIndex : activeOptions.length === 0 ? null : 0
  return {
    activeSelectedIndex,
    selectedActiveOption:
      activeSelectedIndex === null ? undefined : activeOptions[activeSelectedIndex]
  }
}
