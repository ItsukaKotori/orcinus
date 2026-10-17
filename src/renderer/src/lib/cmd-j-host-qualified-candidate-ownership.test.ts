import { describe, expect, it } from 'vitest'
import type { Tab } from '../../../shared/tab-types'
import type { TerminalTab } from '../../../shared/terminal-tab-types'
import type { Worktree } from '../../../shared/worktree/types'
import { buildSearchableWorkspaceTabs, searchWorkspaceTabs } from './workspace-tab-palette-search'
import { buildOpenTabSearchEntries } from '../components/tab-bar/open-tab-search-entries'

const SHARED_WORKTREE_ID = 'repo-shared::/workspace'
const RUNTIME_HOST_ID = 'runtime:paired-host'

function makeWorktree(overrides: Partial<Worktree>): Worktree {
  return {
    id: SHARED_WORKTREE_ID,
    repoId: 'repo-shared',
    path: '/workspace',
    head: 'abc123',
    branch: 'refs/heads/main',
    isBare: false,
    isMainWorktree: false,
    displayName: 'Workspace',
    comment: '',
    linkedIssue: null,
    linkedPR: null,
    linkedLinearIssue: null,
    isArchived: false,
    isUnread: false,
    isPinned: false,
    sortOrder: 0,
    lastActivityAt: 0,
    ...overrides
  }
}

function makeTab(overrides: Partial<Tab>): Tab {
  return {
    id: 'tab',
    entityId: 'entity',
    groupId: 'group',
    worktreeId: SHARED_WORKTREE_ID,
    contentType: 'terminal',
    label: 'Tab',
    customLabel: null,
    color: null,
    sortOrder: 0,
    createdAt: 1,
    ...overrides
  }
}


function pairedWorktrees(): Worktree[] {
  return [
    makeWorktree({ hostId: 'local', displayName: 'Local workspace' }),
    makeWorktree({
      hostId: 'ssh:private-target',
      runtimeOwnerEnvironmentId: 'paired-host',
      displayName: 'Paired SSH workspace'
    })
  ]
}

describe('Cmd-J host-qualified candidate ownership', () => {
  it('keeps generic workspace rows isolated by execution host', () => {
    const terminalTabs: TerminalTab[] = [
      {
        id: 'local-terminal',
        ptyId: 'local-pty',
        worktreeId: SHARED_WORKTREE_ID,
        title: 'Local shell',
        customTitle: null,
        color: null,
        sortOrder: 0,
        createdAt: 1
      },
      {
        id: 'remote-terminal',
        ptyId: 'remote-pty',
        worktreeId: SHARED_WORKTREE_ID,
        title: 'Remote shell',
        customTitle: null,
        color: null,
        sortOrder: 1,
        createdAt: 1
      }
    ]
    const entries = buildSearchableWorkspaceTabs({
      worktrees: pairedWorktrees(),
      repoMap: new Map(),
      worktreeOrder: new Map(),
      unifiedTabsByWorktree: {
        [SHARED_WORKTREE_ID]: [
          makeTab({
            id: 'local-terminal',
            entityId: 'local-terminal',
            executionHostId: 'local',
            label: 'Local shell'
          }),
          makeTab({
            id: 'remote-terminal',
            entityId: 'remote-terminal',
            executionHostId: RUNTIME_HOST_ID,
            label: 'Remote shell'
          })
        ]
      },
      tabsByWorktree: { [SHARED_WORKTREE_ID]: terminalTabs },
      openFiles: [],
      agentStatusByPaneKey: {},
      retainedAgentsByPaneKey: {},
      sleepingAgentSessionsByPaneKey: {},
      activeGroupIdByWorktree: {},
      groupsByWorktree: {},
      activeWorktreeId: null,
      activeTabType: 'terminal',
      activeTabId: null,
      activeTabIdByWorktree: {},
      activeFileId: null,
      activeFileIdByWorktree: {},
      activeTabTypeByWorktree: {},
      generatedTitlesEnabled: true
    })

    expect(
      searchWorkspaceTabs(entries, 'shell')
        .map((result) => [result.tabId, result.executionHostId])
        .sort(([left], [right]) => String(left).localeCompare(String(right)))
    ).toEqual([
      ['local-terminal', 'local'],
      ['remote-terminal', RUNTIME_HOST_ID]
    ])
  })

  it('omits editor rows whose bare file id cannot be activated safely', () => {
    const entries = buildSearchableWorkspaceTabs({
      worktrees: pairedWorktrees(),
      repoMap: new Map(),
      worktreeOrder: new Map(),
      unifiedTabsByWorktree: {
        [SHARED_WORKTREE_ID]: [
          makeTab({
            id: 'shared-editor',
            entityId: 'shared-file',
            contentType: 'editor',
            executionHostId: 'local'
          }),
          makeTab({
            id: 'shared-editor',
            entityId: 'shared-file',
            contentType: 'editor',
            executionHostId: RUNTIME_HOST_ID
          })
        ]
      },
      tabsByWorktree: {},
      openFiles: [
        {
          id: 'shared-file',
          filePath: '/local/local-atlas.ts',
          relativePath: 'local/local-atlas.ts',
          worktreeId: SHARED_WORKTREE_ID,
          language: 'typescript',
          isDirty: false,
          mode: 'edit'
        },
        {
          id: 'shared-file',
          filePath: '/remote/remote-atlas.ts',
          relativePath: 'remote/remote-atlas.ts',
          worktreeId: SHARED_WORKTREE_ID,
          language: 'typescript',
          isDirty: false,
          runtimeEnvironmentId: 'paired-host',
          mode: 'edit'
        }
      ],
      agentStatusByPaneKey: {},
      retainedAgentsByPaneKey: {},
      sleepingAgentSessionsByPaneKey: {},
      activeGroupIdByWorktree: {},
      groupsByWorktree: {},
      activeWorktreeId: null,
      activeTabType: 'terminal',
      activeTabId: null,
      activeTabIdByWorktree: {},
      activeFileId: null,
      activeFileIdByWorktree: {},
      activeTabTypeByWorktree: {},
      generatedTitlesEnabled: true
    })

    expect(searchWorkspaceTabs(entries, 'local-atlas')).toEqual([])
    expect(searchWorkspaceTabs(entries, 'remote-atlas')).toEqual([])
  })

  it('keeps hidden same-id hosts in the legacy ownership ambiguity set', () => {
    const ownershipWorktrees = pairedWorktrees()
    const unifiedTabsByWorktree = {
      [SHARED_WORKTREE_ID]: [
        makeTab({ id: 'legacy-terminal', entityId: 'legacy-terminal', executionHostId: undefined })
      ]
    }
    const entries = buildOpenTabSearchEntries(
      {
        activeFileId: null,
        activeFileIdByWorktree: {},
        activeGroupIdByWorktree: {},
        activeTabId: null,
        activeTabIdByWorktree: {},
        activeTabType: 'terminal',
        activeTabTypeByWorktree: {},
        activeWorktreeId: null,
        executionHostId: 'local',
        generatedTitlesEnabled: true,
        groupsByWorktree: {},
        openFiles: [],
        folderWorkspaces: [],
        repo: null,
        tabsByWorktree: {
          [SHARED_WORKTREE_ID]: [
            {
              id: 'legacy-terminal',
              ptyId: 'legacy-pty',
              worktreeId: SHARED_WORKTREE_ID,
              title: 'Legacy shell',
              customTitle: null,
              color: null,
              sortOrder: 0,
              createdAt: 1
            }
          ]
        },
        unifiedTabsByWorktree,
        worktree: ownershipWorktrees[0],
        worktreesByRepo: { repo: ownershipWorktrees }
      },
      {
        agentStatusByPaneKey: {},
        paneForegroundAgentByPaneKey: {},
        retainedAgentsByPaneKey: {},
        sleepingAgentSessionsByPaneKey: {},
        terminalLayoutsByTabId: {}
      }
    )

    expect(entries).toEqual({ workspaceTabs: [] })
  })
})
