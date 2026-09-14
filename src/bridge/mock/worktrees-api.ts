// Phase 0 mock; replaced by Tauri IPC per view.
import type { WorktreeApi } from '../../preload/api/worktree-api'
import type { DetectedWorktreeListResult, Worktree } from '../../shared/worktree/types'
import { EMPTY_RETIRED_NAME_REGISTRY } from '../../shared/worktree/retired-name-registry'
import { UnimplementedBridgeError } from '../unimplemented-fallback'

const MOCK_REPO_ID = 'mock-repo-1'
const MOCK_WORKSPACE_ROOT = 'C:\\Users\\ade\\orca\\workspaces\\ade'

const MOCK_WORKTREES: Worktree[] = [
  {
    id: `${MOCK_REPO_ID}::${MOCK_WORKSPACE_ROOT}\\calm-otter`,
    repoId: MOCK_REPO_ID,
    path: `${MOCK_WORKSPACE_ROOT}\\calm-otter`,
    head: '1111111111111111111111111111111111111111',
    branch: 'feature/plugin-center',
    isBare: false,
    isMainWorktree: false,
    displayName: 'calm-otter',
    comment: '',
    linkedIssue: null,
    linkedPR: null,
    linkedLinearIssue: null,
    isArchived: false,
    isUnread: false,
    isPinned: false,
    sortOrder: 0,
    lastActivityAt: Date.now() - 60_000
  },
  {
    id: `${MOCK_REPO_ID}::${MOCK_WORKSPACE_ROOT}\\brave-heron`,
    repoId: MOCK_REPO_ID,
    path: `${MOCK_WORKSPACE_ROOT}\\brave-heron`,
    head: '2222222222222222222222222222222222222222',
    branch: 'fix/settings-groups',
    isBare: false,
    isMainWorktree: false,
    displayName: 'brave-heron',
    comment: '',
    linkedIssue: null,
    linkedPR: null,
    linkedLinearIssue: null,
    isArchived: false,
    isUnread: false,
    isPinned: false,
    sortOrder: 1,
    lastActivityAt: Date.now() - 3_600_000
  }
]

const MOCK_DETECTED = MOCK_WORKTREES.map((worktree, index) => ({
  ...worktree,
  ownership: 'orca-managed' as const,
  selectedCheckout: index === 0,
  visible: true
}))

function unimplemented(path: string): never {
  throw new UnimplementedBridgeError(path)
}

export function createWorktreesApi(): WorktreeApi {
  return {
    list: async () => MOCK_WORKTREES,
    listRetiredNames: async () => EMPTY_RETIRED_NAME_REGISTRY,
    listDetected: async (args: { repoId: string }): Promise<DetectedWorktreeListResult> => ({
      repoId: args.repoId,
      authoritative: true,
      source: 'git',
      worktrees: MOCK_DETECTED
    }),
    listAll: async () => MOCK_WORKTREES,
    create: async () => unimplemented('worktrees.create'),
    adoptProvisionedRoot: async () => unimplemented('worktrees.adoptProvisionedRoot'),
    onCreateProgress: (_callback) => () => {},
    prefetchCreateBase: async () => {},
    resolvePrBase: async () => unimplemented('worktrees.resolvePrBase'),
    resolveMrBase: async () => unimplemented('worktrees.resolveMrBase'),
    remove: async () => unimplemented('worktrees.remove'),
    forgetLocal: async () => unimplemented('worktrees.forgetLocal'),
    forceDeletePreservedBranch: async () =>
      unimplemented('worktrees.forceDeletePreservedBranch'),
    updateMeta: async () => unimplemented('worktrees.updateMeta'),
    listLineage: async () => ({ lineage: {} }),
    updateLineage: async () => null,
    persistSortOrder: async () => {},
    getBranchRenameFailureOutput: async () => null,
    onChanged: (_callback) => () => {},
    onGitStatusMetadataChanged: (_callback) => () => {},
    onHeadIdentitiesChanged: (_callback) => () => {},
    onBaseStatus: (_callback) => () => {},
    onRemoteBranchConflict: (_callback) => () => {}
  }
}
