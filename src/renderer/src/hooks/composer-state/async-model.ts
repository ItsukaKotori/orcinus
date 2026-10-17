import type * as React from 'react'
import type { RefObject } from 'react'
import type { OrcaHooks, SetupAgentStartupPolicy } from '../../../../shared/orca-yaml-hook-types'
import type { GlobalSettings } from '../../../../shared/global-settings-types'
import type { WorkspaceCreateErrorDisplay } from '@/lib/workspace-create-error-format'
import type { IssueCommandReadResult } from '@/runtime/runtime-hooks-client'
import type { SmartGitHubPrStartPointSelection } from './source-selection-decisions'

export type ComposerAsyncModel = {
  yamlHooks: OrcaHooks | null
  setYamlHooks: React.Dispatch<React.SetStateAction<OrcaHooks | null>>
  checkedHooksContextKey: string | null
  setCheckedHooksContextKey: React.Dispatch<React.SetStateAction<string | null>>
  loadedIssueCommand: { contextKey: string; result: IssueCommandReadResult } | null
  setLoadedIssueCommand: React.Dispatch<
    React.SetStateAction<{ contextKey: string; result: IssueCommandReadResult } | null>
  >
  currentIssueCommand: IssueCommandReadResult | null
  issueCommandTemplate: string
  hasLoadedIssueCommand: boolean
  setupDecision: 'skip' | 'run' | null
  setSetupDecision: React.Dispatch<React.SetStateAction<'skip' | 'run' | null>>
  setupAgentStartupPolicy: SetupAgentStartupPolicy
  setSetupAgentStartupPolicy: React.Dispatch<React.SetStateAction<SetupAgentStartupPolicy>>
  setupAgentStartupPolicyRef: RefObject<SetupAgentStartupPolicy>
  setupAgentStartupPolicySaveRef: RefObject<{
    repoId: string
    policy: SetupAgentStartupPolicy
    promise: Promise<boolean>
  } | null>
  setupAgentStartupPolicyDraftRef: RefObject<{
    repoId: string
    policy: SetupAgentStartupPolicy
  } | null>
  creating: boolean
  setCreating: React.Dispatch<React.SetStateAction<boolean>>
  createError: WorkspaceCreateErrorDisplay | null
  setCreateError: React.Dispatch<React.SetStateAction<WorkspaceCreateErrorDisplay | null>>
  createMultiple: boolean
  setCreateMultiple: React.Dispatch<React.SetStateAction<boolean>>
  advancedOpen: boolean
  setAdvancedOpen: React.Dispatch<React.SetStateAction<boolean>>
  sparseEnabled: boolean
  setSparseEnabled: React.Dispatch<React.SetStateAction<boolean>>
  sparseDirectories: string
  setSparseDirectories: React.Dispatch<React.SetStateAction<string>>
  sparseSelectedPresetId: string | null
  setSparseSelectedPresetId: React.Dispatch<React.SetStateAction<string | null>>
  lastAutoNameRef: RefObject<string>
  nameRef: RefObject<string>
  branchAutoNameRef: RefObject<string>
  lastAutoNoteRef: RefObject<string>
  noteRef: RefObject<string>
  smartGitHubPrStartPointSelectionRef: RefObject<SmartGitHubPrStartPointSelection | null>
  composerRef: RefObject<HTMLDivElement | null>
  promptTextareaRef: RefObject<HTMLTextAreaElement | null>
  promptCaretFrameRef: RefObject<number | null>
  nameInputRef: RefObject<HTMLInputElement | null>
  agentPromptRef: RefObject<string>
  connectionIdRef: RefObject<string | null>
  selectedRepoConnectionIdRef: RefObject<string | null>
  selectedRepoPath: string | undefined
  selectedRepoPathRef: RefObject<string | undefined>
  selectedRepoSettingsRef: RefObject<Pick<GlobalSettings, 'activeRuntimeEnvironmentId'> | null>
}
