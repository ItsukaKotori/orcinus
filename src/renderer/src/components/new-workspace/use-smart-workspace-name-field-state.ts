import { useRef, useState } from 'react'
import type { BaseRefSearchResult } from '../../../../shared/repo-types'
import type { SmartNameMode } from './smart-workspace-source-results'

export function useSmartWorkspaceNameFieldState(textOnly: boolean, value: string) {
  const [mode, setMode] = useState<SmartNameMode>(textOnly ? 'text' : 'smart')
  const [open, setOpen] = useState(false)
  const [debouncedQuery, setDebouncedQuery] = useState(value)
  const [branches, setBranches] = useState<BaseRefSearchResult[]>([])
  const [branchResultsSource, setBranchResultsSource] = useState<{
    repoId: string
    query: string
  } | null>(null)
  const [branchesLoading, setBranchesLoading] = useState(false)
  const [commandValue, setCommandValue] = useState('')
  const [emojiCommandValue, setEmojiCommandValue] = useState('')
  const [emojiCursor, setEmojiCursor] = useState<number | null>(null)
  const localInputRef = useRef<HTMLInputElement | null>(null)
  const focusedSelectedSourceKeyRef = useRef<string | null>(null)
  const tabsListRef = useRef<HTMLDivElement | null>(null)
  const localInputFocusFrameRef = useRef<number | null>(null)
  // Why: Electron makes programmatic .focus() look user-initiated, so wait for real interaction.
  const deferSourcePopoverUntilInteractionRef = useRef(true)

  return {
    mode,
    setMode,
    open,
    setOpen,
    debouncedQuery,
    setDebouncedQuery,
    branches,
    setBranches,
    branchResultsSource,
    setBranchResultsSource,
    branchesLoading,
    setBranchesLoading,
    commandValue,
    setCommandValue,
    emojiCommandValue,
    setEmojiCommandValue,
    emojiCursor,
    setEmojiCursor,
    localInputRef,
    focusedSelectedSourceKeyRef,
    tabsListRef,
    localInputFocusFrameRef,
    deferSourcePopoverUntilInteractionRef
  }
}
