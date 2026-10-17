// @vitest-environment happy-dom

import { act, type ReactNode } from 'react'
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { useAppStore } from '@/store'
import type { FolderWorkspace } from '../../../../shared/folder-workspace-types'
import type { Repo } from '../../../../shared/repo-types'
import type { WorktreeMeta } from '../../../../shared/worktree/meta-types'
import type { WorktreeMetaUpdateOptions } from '@/store/slices/worktree-helpers'
import type { Worktree } from '../../../../shared/worktree/types'

// Why: Radix tooltips need a provider the dialog does not own, and the menu's
// portal needs real layout. Stand-ins keep these tests on provider selection.
vi.mock('@/components/ui/tooltip', () => ({
  Tooltip: ({ children }: { children?: ReactNode }) => <>{children}</>,
  TooltipContent: () => null,
  TooltipTrigger: ({ children }: { children?: ReactNode }) => <>{children}</>
}))

vi.mock('@/components/ui/dropdown-menu', async () => {
  const React = await import('react')
  const SelectContext = React.createContext<(value: string) => void>(() => {})
  const Passthrough = ({ children }: { children?: ReactNode }) => <>{children}</>
  return {
    DropdownMenu: Passthrough,
    DropdownMenuTrigger: Passthrough,
    DropdownMenuContent: Passthrough,
    DropdownMenuRadioGroup: ({
      value,
      onValueChange,
      children
    }: {
      value: string
      onValueChange: (value: string) => void
      children?: ReactNode
    }) => (
      <SelectContext.Provider value={onValueChange}>
        <div data-selected={value}>{children}</div>
      </SelectContext.Provider>
    ),
    DropdownMenuRadioItem: ({ value, children }: { value: string; children?: ReactNode }) => {
      const onSelect = React.useContext(SelectContext)
      return (
        <button type="button" role="menuitemradio" onClick={() => onSelect(value)}>
          {children}
        </button>
      )
    }
  }
})

import WorktreeMetaDialog from './WorktreeMetaDialog'

const REPO_ID = 'repo-1'
const WORKTREE_ID = 'repo-1::/repo/worktrees/feature'

const initialState = useAppStore.getInitialState()
const updateWorktreeMeta =
  vi.fn<
    (
      id: string,
      updates: Partial<WorktreeMeta>,
      options?: WorktreeMetaUpdateOptions
    ) => Promise<{ ok: true } | { ok: false; error: string }>
  >()
const openUrl = vi.fn<(url: string) => void>()

function makeRepo(id: string = REPO_ID, path: string = '/repo'): Repo {
  return { id, path, displayName: 'orca', badgeColor: '#999999', addedAt: 1 }
}

function makeWorktree(overrides: Partial<Worktree> = {}): Worktree {
  return {
    id: WORKTREE_ID,
    repoId: REPO_ID,
    path: '/repo/worktrees/feature',
    displayName: 'Feature work',
    branch: 'feature',
    head: 'abc123',
    isBare: false,
    isMainWorktree: false,
    comment: 'existing note',
    linkedIssue: null,
    linkedPR: null,
    linkedLinearIssue: null,
    isArchived: false,
    isUnread: false,
    isPinned: false,
    sortOrder: 0,
    lastActivityAt: 1,
    ...overrides
  }
}

function makeFolderWorkspace(overrides: Partial<FolderWorkspace> = {}): FolderWorkspace {
  return {
    id: 'fw-1',
    projectGroupId: 'pg-1',
    name: 'Docs folder',
    folderPath: '/repo/docs',
    linkedTask: {
      provider: 'github',
      type: 'issue',
      number: 901,
      title: 'Fix auth',
      url: 'https://github.com/acme/orca/issues/901'
    },
    comment: '',
    isArchived: false,
    isUnread: false,
    isPinned: false,
    sortOrder: 0,
    lastActivityAt: 1,
    createdAt: 1,
    updatedAt: 1,
    ...overrides
  }
}

function openDialog(
  options: {
    worktree?: Partial<Worktree>
    worktreeId?: string
    folderWorkspace?: Partial<FolderWorkspace>
    /** Extra owners of the same workspace ID, which the index reads as ambiguous. */
    otherRepos?: { repoId: string; worktree?: Partial<Worktree> }[]
    modalRepoId?: string
    modalExecutionHostId?: string
    modalCurrentReview?: number
    modalSuppressHostedReviewRefresh?: boolean
  } = {}
): void {
  const worktree = makeWorktree(options.worktree)
  const otherRepos = options.otherRepos ?? []
  useAppStore.setState({
    repos: [makeRepo(), ...otherRepos.map((other) => makeRepo(other.repoId, `/${other.repoId}`))],
    worktreesByRepo: {
      [REPO_ID]: [worktree],
      ...Object.fromEntries(
        otherRepos.map((other) => [
          other.repoId,
          [makeWorktree({ repoId: other.repoId, ...other.worktree })]
        ])
      )
    },
    ...(options.folderWorkspace
      ? { folderWorkspaces: [makeFolderWorkspace(options.folderWorkspace)] }
      : {}),
    activeModal: 'edit-meta',
    modalData: {
      worktreeId: options.worktreeId ?? worktree.id,
      ...(options.modalRepoId ? { repoId: options.modalRepoId } : {}),
      ...(options.modalExecutionHostId ? { executionHostId: options.modalExecutionHostId } : {}),
      ...(options.modalCurrentReview ? { currentReview: options.modalCurrentReview } : {}),
      ...(options.modalSuppressHostedReviewRefresh ? { suppressHostedReviewRefresh: true } : {}),
      currentDisplayName: worktree.displayName,
      currentComment: worktree.comment,
      focus: 'comment'
    },
    updateWorktreeMeta
  })
  render(<WorktreeMetaDialog />)
}

function saveButton(): HTMLButtonElement {
  return screen.getByRole('button', { name: 'Save' })
}

describe('WorktreeMetaDialog issue link row', () => {
  beforeEach(() => {
    useAppStore.setState(initialState, true)
    updateWorktreeMeta.mockReset()
    updateWorktreeMeta.mockResolvedValue({ ok: true })
    openUrl.mockReset()
    Object.defineProperty(window, 'api', {
      configurable: true,
      value: { shell: { openUrl } }
    })
  })

  afterEach(() => {
    cleanup()
    useAppStore.setState(initialState, true)
  })

  it('replaces a completed emoji shortcode in the display name', () => {
    openDialog()
    const displayNameInput = screen.getByRole('textbox', { name: 'Display Name' })

    fireEvent.change(displayNameInput, {
      target: { value: 'Feature :wink:', selectionStart: 14 }
    })

    expect((displayNameInput as HTMLInputElement).value).toBe('Feature 😉')
  })

  it('qualifies a save with the host selected by the opening row', async () => {
    openDialog({
      worktree: { hostId: 'ssh:build-box' },
      modalExecutionHostId: 'ssh:build-box'
    })

    fireEvent.change(screen.getByPlaceholderText('Notes about this worktree...'), {
      target: { value: 'remote note' }
    })
    await act(async () => {
      fireEvent.click(saveButton())
    })

    await waitFor(() =>
      expect(updateWorktreeMeta).toHaveBeenCalledWith(
        WORKTREE_ID,
        expect.objectContaining({ comment: 'remote note' }),
        { executionHostId: 'ssh:build-box' }
      )
    )
  })

  // updateWorktreeMeta stamps lastActivityAt on any comment write, which would
  // reorder the workspace under the time-decay sidebar sort.
  // A failed save refetches and reverts the optimistic write, so closing here
  // would report success for an edit that silently undid itself.
  it('leaves the issue link alone when only the comment is edited', async () => {
    openDialog({ worktree: { linkedIssue: 42 } })

    fireEvent.change(screen.getByPlaceholderText('Notes about this worktree...'), {
      target: { value: 'updated note' }
    })

    expect(screen.queryByText(/Saving unlinks/)).toBeNull()

    await act(async () => {
      fireEvent.click(saveButton())
    })

    await waitFor(() => expect(updateWorktreeMeta).toHaveBeenCalledTimes(1))
    const updates = updateWorktreeMeta.mock.calls[0]?.[1] ?? {}
    expect(Object.keys(updates)).not.toContain('linkedIssue')
    expect(updates.comment).toBe('updated note')
  })

  // Folder workspaces live outside worktreesByRepo, so the indexed lookup alone
  // leaves the row blank and the link it does hold looks lost.
  // A background `orca worktree set` must not move the baseline mid-edit: the
  // field would read as dirty and a comment-only save would write the stale seed.
  it('keeps the baseline frozen when the store changes while open', async () => {
    openDialog({ worktree: { linkedIssue: 42 } })

    act(() => {
      useAppStore.setState({
        worktreesByRepo: {
          [REPO_ID]: [makeWorktree({ linkedIssue: 99 })]
        }
      })
    })
    fireEvent.change(screen.getByPlaceholderText('Notes about this worktree...'), {
      target: { value: 'still working' }
    })
    await act(async () => {
      fireEvent.click(saveButton())
    })

    const updates = updateWorktreeMeta.mock.calls[0]?.[1] ?? {}
    expect(updates.comment).toBe('still working')
    expect(updates).not.toHaveProperty('linkedIssue')
  })

  // Same issue, different spelling: the link is unchanged, so its title and
  // SSH/runtime source context must survive the save.
  // The owner index reports a duplicated workspace ID as ambiguous rather than
  // guessing, so the opening row has to name its own bucket.
})
