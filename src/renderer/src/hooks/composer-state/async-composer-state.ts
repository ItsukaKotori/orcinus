import type { ComposerModel } from './composer-model'

type ComposerAsyncStateInput = Pick<
  ComposerModel,
  | 'agentPrompt'
  | 'connectionId'
  | 'decisions'
  | 'draftLinkedWorkItemSeed'
  | 'enableIssueAutomation'
  | 'initialLinkedWorkItemSeed'
  | 'initialName'
  | 'name'
  | 'newWorkspaceDraft'
  | 'note'
  | 'persistDraft'
  | 'selectedRepo'
  | 'selectedRepoConnectionId'
  | 'selectedRepoHookContextKey'
  | 'selectedRepoIsGit'
  | 'selectedRepoSettings'
  | 'setName'
>

import { useState, useRef, useEffect } from 'react'
import type { OrcaHooks, SetupAgentStartupPolicy } from '../../../../shared/orca-yaml-hook-types'
import type { IssueCommandReadResult } from '@/runtime/runtime-hooks-client'
import type { WorkspaceCreateErrorDisplay } from '@/lib/workspace-create-error-format'
import { getRepoSetupAgentStartupPolicy } from './setup-policy-decisions'
import type { SmartGitHubPrStartPointSelection } from './source-selection-decisions'

export function useComposerAsyncState(input: ComposerAsyncStateInput) {
  const {
    agentPrompt,
    connectionId,
    decisions,
    draftLinkedWorkItemSeed,
    enableIssueAutomation,
    initialLinkedWorkItemSeed,
    initialName,
    name,
    newWorkspaceDraft,
    note,
    persistDraft,
    selectedRepo,
    selectedRepoConnectionId,
    selectedRepoHookContextKey,
    selectedRepoIsGit,
    selectedRepoSettings
  } = input
  const { getInitialAutoManagedWorkspaceName } = decisions

  const [yamlHooks, setYamlHooks] = useState<OrcaHooks | null>(null)

  const [checkedHooksContextKey, setCheckedHooksContextKey] = useState<string | null>(null)

  const [loadedIssueCommand, setLoadedIssueCommand] = useState<{
    contextKey: string
    result: IssueCommandReadResult
  } | null>(null)

  const currentIssueCommand =
    loadedIssueCommand?.contextKey === selectedRepoHookContextKey ? loadedIssueCommand.result : null

  const issueCommandTemplate = currentIssueCommand?.effectiveContent ?? ''

  const hasLoadedIssueCommand =
    !selectedRepoIsGit || !enableIssueAutomation || currentIssueCommand !== null

  const [setupDecision, setSetupDecision] = useState<'run' | 'skip' | null>(null)

  const [setupAgentStartupPolicy, setSetupAgentStartupPolicy] = useState<SetupAgentStartupPolicy>(
    () => getRepoSetupAgentStartupPolicy(selectedRepo)
  )

  const setupAgentStartupPolicyRef = useRef(setupAgentStartupPolicy)

  useEffect(() => {
    setupAgentStartupPolicyRef.current = setupAgentStartupPolicy
  }, [setupAgentStartupPolicy])

  const setupAgentStartupPolicySaveRef = useRef<{
    repoId: string
    policy: SetupAgentStartupPolicy
    promise: Promise<boolean>
  } | null>(null)

  const setupAgentStartupPolicyDraftRef = useRef<{
    repoId: string
    policy: SetupAgentStartupPolicy
  } | null>(null)

  const [creating, setCreating] = useState(false)

  const [createError, setCreateError] = useState<WorkspaceCreateErrorDisplay | null>(null)

  // Why: when checked, a successful create keeps the modal open and resets identity fields so the user can queue another worktree.
  const [createMultiple, setCreateMultiple] = useState(false)

  const [advancedOpen, setAdvancedOpen] = useState(
    persistDraft ? Boolean((newWorkspaceDraft?.note ?? '').trim()) : false
  )

  const [sparseEnabled, setSparseEnabled] = useState(false)

  const [sparseDirectories, setSparseDirectories] = useState('')

  const [sparseSelectedPresetId, setSparseSelectedPresetId] = useState<string | null>(null)

  const lastAutoNameRef = useRef<string>(undefined!)
  lastAutoNameRef.current ??= getInitialAutoManagedWorkspaceName({
    draftName: persistDraft ? newWorkspaceDraft?.name : null,
    draftLinkedWorkItem: persistDraft ? draftLinkedWorkItemSeed : null,
    initialName,
    initialLinkedWorkItem: initialLinkedWorkItemSeed
  })

  const nameRef = useRef<string>(name)

  const branchAutoNameRef = useRef<string>('')

  // Why: the note we auto-prefilled from a Start-from PR pick, so a later PR change can replace it without clobbering user-typed text.
  const lastAutoNoteRef = useRef<string>('')

  // Why: let handleBaseBranchPrSelect read the latest note without adding it to deps (would rebuild the callback on every keystroke).
  const noteRef = useRef<string>(note)

  useEffect(() => {
    nameRef.current = name
    noteRef.current = note
  }, [name, note])

  const smartGitHubPrStartPointSelectionRef = useRef<SmartGitHubPrStartPointSelection | null>(null)

  const composerRef = useRef<HTMLDivElement | null>(null)

  const promptTextareaRef = useRef<HTMLTextAreaElement | null>(null)

  const promptCaretFrameRef = useRef<number | null>(null)

  const nameInputRef = useRef<HTMLInputElement | null>(null)

  // Why: mirror agentPrompt into a ref so the once-mounted file-drop listener reads it fresh without re-subscribing, which would reorder composerDropStack.
  const agentPromptRef = useRef(agentPrompt)

  const connectionIdRef = useRef(connectionId)

  const selectedRepoConnectionIdRef = useRef(selectedRepoConnectionId)

  const selectedRepoPath = selectedRepo?.path

  const selectedRepoPathRef = useRef<string | undefined>(selectedRepoPath)

  const selectedRepoSettingsRef = useRef(selectedRepoSettings)

  useEffect(() => {
    agentPromptRef.current = agentPrompt
    connectionIdRef.current = connectionId
    selectedRepoConnectionIdRef.current = selectedRepoConnectionId
    selectedRepoPathRef.current = selectedRepoPath
    selectedRepoSettingsRef.current = selectedRepoSettings
  }, [agentPrompt, connectionId, selectedRepoConnectionId, selectedRepoPath, selectedRepoSettings])

  return {
    yamlHooks,
    setYamlHooks,
    checkedHooksContextKey,
    setCheckedHooksContextKey,
    loadedIssueCommand,
    setLoadedIssueCommand,
    currentIssueCommand,
    issueCommandTemplate,
    hasLoadedIssueCommand,
    setupDecision,
    setSetupDecision,
    setupAgentStartupPolicy,
    setSetupAgentStartupPolicy,
    setupAgentStartupPolicyRef,
    setupAgentStartupPolicySaveRef,
    setupAgentStartupPolicyDraftRef,
    creating,
    setCreating,
    createError,
    setCreateError,
    createMultiple,
    setCreateMultiple,
    advancedOpen,
    setAdvancedOpen,
    sparseEnabled,
    setSparseEnabled,
    sparseDirectories,
    setSparseDirectories,
    sparseSelectedPresetId,
    setSparseSelectedPresetId,
    lastAutoNameRef,
    nameRef,
    branchAutoNameRef,
    lastAutoNoteRef,
    noteRef,
    smartGitHubPrStartPointSelectionRef,
    composerRef,
    promptTextareaRef,
    promptCaretFrameRef,
    nameInputRef,
    agentPromptRef,
    connectionIdRef,
    selectedRepoConnectionIdRef,
    selectedRepoPath,
    selectedRepoPathRef,
    selectedRepoSettingsRef
  }
}
