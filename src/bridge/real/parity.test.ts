import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { PreloadApi } from '../../shared/preload-api/api-types'
import type { Repo } from '../../shared/repo-types'
import type { Worktree } from '../../shared/worktree/types'
import { createMockAdeApi } from '../create-api'
import { UnimplementedBridgeError } from '../unimplemented-fallback'
import { createAppRealApi } from './app'
import { createFsRealApi } from './fs'
import { createPlatformRealApi } from './platform'
import { createProjectsRealApi } from './projects'
import { createReposRealApi } from './repos'
import { createSettingsRealApi } from './settings'
import { createUiRealApi } from './ui'
import { createWorktreesRealApi } from './worktrees'

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }))
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn() }))

const invokeMock = vi.mocked(invoke)
const listenMock = vi.mocked(listen)

const repoFixture: Repo = {
  id: 'r1',
  path: '/repo',
  displayName: 'Repo',
  badgeColor: '#737373',
  addedAt: 1_000,
  kind: 'git'
}

const worktreeFixture = {
  id: 'r1::/repo',
  repoId: 'r1',
  path: '/repo',
  head: 'abc',
  branch: 'refs/heads/main',
  isBare: false,
  isMainWorktree: true,
  displayName: 'main',
  comment: '',
  linkedIssue: null,
  linkedPR: null,
  linkedLinearIssue: null,
  isArchived: false,
  isUnread: false,
  isPinned: false,
  sortOrder: 0,
  lastActivityAt: 0,
  workspaceStatus: 'in-progress',
  displayNameMode: 'automatic'
} as Worktree

const fixtures: Record<string, unknown> = {
  repos_list: [repoFixture],
  worktrees_list_all: [worktreeFixture],
  settings_get: { theme: 'dark' },
  ui_get: { activeView: 'terminal' },
  app_get_identity: {
    name: 'Orcinus',
    isDev: false,
    devLabel: null,
    devBranch: null,
    devWorktreeName: null,
    devRepoRoot: null,
    dockBadgeLabel: null
  }
}

const bootstrapScope = globalThis as { __ADE_BOOTSTRAP__?: unknown }

beforeEach(() => {
  invokeMock.mockReset()
  invokeMock.mockImplementation(async (command: string) => fixtures[command] ?? null)
  listenMock.mockReset()
  bootstrapScope.__ADE_BOOTSTRAP__ = {
    settings: { theme: 'dark' },
    platform: {
      platform: 'darwin',
      osRelease: 'Darwin 25.0.0',
      arch: 'aarch64',
      shell: '/bin/zsh',
      displayServer: null
    },
    schemaVersion: 1
  }
})

afterEach(() => {
  delete bootstrapScope.__ADE_BOOTSTRAP__
})

type SurfaceCase = {
  domain: keyof PreloadApi
  /** Methods the renderer probes with `typeof` or calls/subscribes unconditionally. */
  explicit: string[]
  /** Methods the A host does not back; they must stay on the rejecting fallback. */
  missing: string[]
}

const uiNoopSubscriptions = [
  'onActivateWorktree',
  'onAppMenuPaste',
  'onAppMenuSelectionAction',
  'onBrowserHistoryNavigate',
  'onCloseActiveTab',
  'onCloseFloatingItem',
  'onCloseSessionTab',
  'onCloseTerminal',
  'onCreateTerminal',
  'onCtrlTabKeyDown',
  'onCtrlTabKeyUp',
  'onDeleteCurrentWorkspace',
  'onEditableContextPaste',
  'onFileDrop',
  'onFindInBrowserPage',
  'onFocusBrowserAddressBar',
  'onFocusEditorTab',
  'onFocusTerminal',
  'onFullscreenChanged',
  'onHardReloadBrowserPage',
  'onJumpToTabIndex',
  'onJumpToWorktreeIndex',
  'onMaximizeChanged',
  'onMobileMarkdownRequest',
  'onMoveSessionTab',
  'onNewBrowserTab',
  'onNewMarkdownTab',
  'onNewTerminalTab',
  'onOpenDiffFromMobile',
  'onOpenFileFromMobile',
  'onOpenMarkdownFiles',
  'onOpenNewWorkspace',
  'onOpenQuickOpen',
  'onOpenSettings',
  'onOpenTasks',
  'onReloadBrowserPage',
  'onRenameTerminal',
  'onRequestTabClose',
  'onRequestTabCreate',
  'onRequestTabSetProfile',
  'onRequestTerminalCreate',
  'onRequestTerminalTabMount',
  'onResumeSleepingAgents',
  'onRichMarkdownContextCommand',
  'onScrollBrowserPage',
  'onSelectFloatingIndex',
  'onSessionTabCloseRequest',
  'onSleepWorktree',
  'onSplitTerminal',
  'onSwitchRecentTab',
  'onSwitchTab',
  'onSwitchTabAcrossAllTypes',
  'onSwitchTerminalTab',
  'onSystemResumed',
  'onTerminalShortcutCaptured',
  'onTerminalTabCloseRequest',
  'onTerminalZoom',
  'onToggleFloatingTerminal',
  'onToggleLeftSidebar',
  'onToggleQuickCommandsMenu',
  'onToggleRightSidebar',
  'onToggleStatusBar',
  'onToggleWorktreePalette',
  'onWindowCloseRequested',
  'onWorktreeHistoryNavigate',
  'onZoomBrowserPage'
]

const surfaceCases: SurfaceCase[] = [
  {
    domain: 'repos',
    explicit: [
      'list',
      'add',
      'update',
      'remove',
      'reorderForHost',
      'pickFolder',
      'pickFolders',
      'pickDirectory',
      'isGitAvailable',
      'getDefaultCreateProjectParent',
      'onChanged',
      'onCloneProgress'
    ],
    missing: [
      'clone',
      'cloneRemote',
      'createRemote',
      'addRemote',
      'create',
      'cloneAbort',
      'getGitUsername',
      'getBaseRefDefault',
      'searchBaseRefs',
      'searchBaseRefDetails',
      'reorder',
      'removeForHost'
    ]
  },
  {
    domain: 'fs',
    explicit: [
      'readDir',
      'readFile',
      'writeFile',
      'createFile',
      'createDir',
      'rename',
      'copy',
      'deletePath',
      'stat',
      'pathExists',
      'pathsExist',
      'listFiles',
      'cancelListFiles',
      'search',
      'watchWorktree',
      'unwatchWorktree',
      'listMarkdownDocuments',
      'authorizeExternalPath',
      'onFsChanged',
      'onLocalLogTailChanged'
    ],
    missing: [
      'downloadFile',
      'downloadFolder',
      'saveDownloadedFile',
      'startDownloadedFile',
      'appendDownloadedFileChunk',
      'finishDownloadedFile',
      'cancelDownloadedFile',
      'readLocalLogTail',
      'startLocalLogTail',
      'stopLocalLogTail',
      'importExternalPaths',
      'stageExternalPathsForRuntimeUpload',
      'resolveDroppedPathsForAgent',
      'runPythonCell'
    ]
  },
  {
    domain: 'projects',
    explicit: ['list', 'listHostSetups', 'update'],
    missing: ['createHostSetup', 'setupExistingFolder', 'updateHostSetup', 'deleteHostSetup']
  },
  {
    domain: 'worktrees',
    explicit: [
      'list',
      'listAll',
      'listDetected',
      'listKnownForExecutionHost',
      'forgetRemovedForExecutionHost',
      'cancelListDetected',
      'onChanged',
      'onHeadIdentitiesChanged',
      'onBaseStatus',
      'onRemoteBranchConflict',
      'onCreateProgress',
      'onGitStatusMetadataChanged'
    ],
    missing: [
      'create',
      'remove',
      'adoptProvisionedRoot',
      'prefetchCreateBase',
      'resolvePrBase',
      'resolveMrBase',
      'forgetLocal',
      'forceDeletePreservedBranch',
      'updateMeta',
      'listLineage',
      'listLineageForHost',
      'updateLineage',
      'persistSortOrder',
      'getBranchRenameFailureOutput',
      'listRetiredNames'
    ]
  },
  {
    domain: 'settings',
    explicit: ['get', 'getSync', 'set', 'onChanged'],
    missing: [
      'setActiveRuntimeEnvironmentPreference',
      'updatePRBotAuthorOverride',
      'listFonts',
      'previewGhosttyImport',
      'previewWarpThemeImport'
    ]
  },
  {
    domain: 'ui',
    explicit: [
      'get',
      'set',
      'setWithAck',
      'recordFeatureInteraction',
      'onStateChanged',
      ...uiNoopSubscriptions
    ],
    missing: [
      'consumePendingOpenSettings',
      'consumePendingMarkdownFileOpens',
      'onOpenCrashReport',
      'onExportPdfRequested',
      'replyTabCreate',
      'replyTabSetProfile',
      'replyTabClose',
      'replyTerminalCreate',
      'respondSessionTabClose',
      'respondMobileMarkdownRequest',
      'respondTerminalTabClose',
      'readClipboardText',
      'readSelectionClipboardText',
      'saveClipboardImageAsTempFile',
      'readClipboardImageThumbnail',
      'writeClipboardText',
      'writeTerminalClipboardText',
      'writeSelectionClipboardText',
      'writeClipboardImage',
      'performNativePaste',
      'performNativeSelectionAction',
      'writeClipboardFile',
      'getZoomLevel',
      'setZoomLevel',
      'syncTrafficLights',
      'setMarkdownEditorFocused',
      'setRichMarkdownContextMenuTarget',
      'setTerminalInputFocused',
      'setFloatingFocus',
      'setShortcutRecorderFocused',
      'minimize',
      'maximize',
      'isMaximized',
      'requestClose',
      'popupMenu',
      'confirmWindowClose',
      'notifyWindowRevealed'
    ]
  },
  {
    domain: 'app',
    explicit: [
      'getIdentity',
      'relaunch',
      'restart',
      'reload',
      'stageBeforeUnloadSync',
      'awaitBeforeUnloadCheckpoint',
      'awaitFirstWindowStartupServices',
      'awaitGitEnvironmentStartupBarrier',
      'prepareTerminalStartupRestoration',
      'recoverLegacyWorkerTerminalsForRendererStartup',
      'startupDiagnostic',
      'getKeyboardInputSourceId',
      'getMacCapturedDigitRowChords',
      'getKeyboardLayoutSnapshot',
      'onKeyboardLayoutChanged',
      'setUnreadDockBadgeCount',
      'getFloatingTerminalCwd',
      'getFloatingMarkdownDirectory',
      'pickFloatingMarkdownDocument',
      'pickFloatingWorkspaceDirectory',
      'writeTerminalRenderDesyncEvidence'
    ],
    missing: []
  },
  { domain: 'platform', explicit: ['get'], missing: [] }
]

describe('mock/real parity: method surface', () => {
  it.each(surfaceCases)('$domain explicitly implements every renderer-probed method', ({
    domain,
    explicit
  }) => {
    const mock = createMockAdeApi()[domain] as unknown as Record<string, unknown>
    const real = realApiFor(domain) as unknown as Record<string, unknown>
    for (const method of explicit) {
      expect(
        Object.prototype.hasOwnProperty.call(real, method),
        `${domain}.${method} must be explicitly implemented in real mode`
      ).toBe(true)
      expect(typeof real[method], `${domain}.${method} must be a function`).toBe('function')
      // Optional contract methods may be absent from the mock inventory; when
      // the mock does define one, both modes must agree it is a function.
      if (mock[method] !== undefined) {
        expect(typeof mock[method], `${domain}.${method} in mock mode`).toBe('function')
      }
    }
  })

  it.each(surfaceCases.filter((surfaceCase) => surfaceCase.missing.length > 0))(
    '$domain rejects methods the host does not back with UnimplementedBridgeError',
    async ({ domain, explicit, missing }) => {
      const warn = vi.spyOn(console, 'warn').mockImplementation(() => {})
      const real = realApiFor(domain) as unknown as Record<string, (...args: unknown[]) => unknown>
      for (const method of missing) {
        expect(explicit).not.toContain(method)
        expect(
          Object.prototype.hasOwnProperty.call(real, method),
          `${domain}.${method} must stay on the fallback`
        ).toBe(false)
        await expect(Promise.resolve(real[method]())).rejects.toBeInstanceOf(
          UnimplementedBridgeError
        )
      }
      warn.mockRestore()
    }
  )
})

type ShapeCase = {
  name: string
  command?: string
  real: () => Promise<unknown>
  mock: () => Promise<unknown>
  assertShape: (value: unknown) => void
}

const shapeCases: ShapeCase[] = [
  {
    name: 'repos.list',
    command: 'repos_list',
    real: () => createReposRealApi().list(),
    mock: () => createMockAdeApi().repos.list(),
    assertShape: (value) => {
      expect(Array.isArray(value)).toBe(true)
    }
  },
  {
    name: 'projects.list',
    command: 'repos_list',
    real: () => createProjectsRealApi().list(),
    mock: () => createMockAdeApi().projects.list(),
    assertShape: (value) => {
      expect(Array.isArray(value)).toBe(true)
    }
  },
  {
    name: 'worktrees.listAll',
    command: 'worktrees_list_all',
    real: () => createWorktreesRealApi().listAll(),
    mock: () => createMockAdeApi().worktrees.listAll(),
    assertShape: (value) => {
      expect(Array.isArray(value)).toBe(true)
      for (const worktree of value as Worktree[]) {
        expect(worktree).toEqual(
          expect.objectContaining({
            id: expect.any(String),
            repoId: expect.any(String),
            path: expect.any(String),
            head: expect.any(String),
            branch: expect.any(String),
            displayName: expect.any(String),
            isMainWorktree: expect.any(Boolean)
          })
        )
      }
    }
  },
  {
    name: 'settings.get',
    command: 'settings_get',
    real: () => createSettingsRealApi().get(),
    mock: () => createMockAdeApi().settings.get(),
    assertShape: (value) => {
      expect(value).toEqual(expect.objectContaining({ theme: expect.any(String) }))
    }
  },
  {
    name: 'ui.get',
    command: 'ui_get',
    real: () => createUiRealApi().get(),
    mock: () => createMockAdeApi().ui.get(),
    assertShape: (value) => {
      expect(value).toEqual(expect.objectContaining({ activeView: expect.any(String) }))
    }
  },
  {
    name: 'app.getIdentity',
    command: 'app_get_identity',
    real: () => createAppRealApi().getIdentity(),
    mock: () => createMockAdeApi().app.getIdentity(),
    assertShape: (value) => {
      expect(value).toEqual(
        expect.objectContaining({ name: expect.any(String), isDev: expect.any(Boolean) })
      )
    }
  },
  {
    name: 'platform.get',
    real: async () => createPlatformRealApi().get(),
    mock: async () => createMockAdeApi().platform.get(),
    assertShape: (value) => {
      expect(value).toEqual(expect.objectContaining({ platform: expect.any(String) }))
    }
  }
]

describe('mock/real parity: read shapes and envelopes', () => {
  it.each(shapeCases)('$name answers the same contract shape in both modes', async ({
    command,
    real,
    mock,
    assertShape
  }) => {
    invokeMock.mockClear()
    assertShape(await real())
    if (command) {
      expect(invokeMock).toHaveBeenCalledWith(command)
    }
    invokeMock.mockClear()
    assertShape(await mock())
    expect(invokeMock).not.toHaveBeenCalled()
  })
})

describe('mock/real parity: event subscriptions', () => {
  it('repos.onChanged hands back an unsubscriber in both modes', async () => {
    const unlisten = vi.fn()
    listenMock.mockResolvedValueOnce(unlisten)
    const realStop = createReposRealApi().onChanged(() => {})
    const mockStop = createMockAdeApi().repos.onChanged(() => {})
    expect(typeof realStop).toBe('function')
    expect(typeof mockStop).toBe('function')
    expect(listenMock).toHaveBeenCalledWith('repos:changed', expect.any(Function))
    realStop()
    await Promise.resolve()
    expect(unlisten).toHaveBeenCalledTimes(1)
    mockStop()
  })
})

type UnimplementedCase = {
  name: string
  real: () => Promise<unknown>
  mock: () => Promise<unknown>
}

const unimplementedCases: UnimplementedCase[] = [
  {
    name: 'repos.clone',
    real: () => createReposRealApi().clone({ url: 'https://example.com/x.git', destination: '/x' }),
    mock: () =>
      createMockAdeApi().repos.clone({ url: 'https://example.com/x.git', destination: '/x' })
  },
  {
    name: 'projects.createHostSetup',
    real: () => createProjectsRealApi().createHostSetup({} as never),
    mock: () => createMockAdeApi().projects.createHostSetup({} as never)
  },
  {
    name: 'worktrees.create',
    real: () => createWorktreesRealApi().create({} as never),
    mock: () => createMockAdeApi().worktrees.create({} as never)
  },
  {
    name: 'fs.downloadFile',
    real: () => createFsRealApi().downloadFile({ filePath: '/a', connectionId: 'ssh:1' }),
    mock: () => createMockAdeApi().fs.downloadFile({ filePath: '/a', connectionId: 'ssh:1' })
  }
]

describe('mock/real parity: unimplemented methods', () => {
  it.each(unimplementedCases)('$name rejects UnimplementedBridgeError in both modes', async ({
    real,
    mock
  }) => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {})
    await expect(real()).rejects.toBeInstanceOf(UnimplementedBridgeError)
    invokeMock.mockClear()
    await expect(mock()).rejects.toBeInstanceOf(UnimplementedBridgeError)
    expect(invokeMock).not.toHaveBeenCalled()
    warn.mockRestore()
  })
})

function realApiFor(domain: keyof PreloadApi): unknown {
  switch (domain) {
    case 'repos':
      return createReposRealApi()
    case 'fs':
      return createFsRealApi()
    case 'projects':
      return createProjectsRealApi()
    case 'worktrees':
      return createWorktreesRealApi()
    case 'settings':
      return createSettingsRealApi()
    case 'ui':
      return createUiRealApi()
    case 'app':
      return createAppRealApi()
    case 'platform':
      return createPlatformRealApi()
    default:
      throw new Error(`no real factory for ${domain}`)
  }
}
