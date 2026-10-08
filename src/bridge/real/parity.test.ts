import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { PtyManagementApi } from '../../shared/preload-api/api/pty-management-api'
import type { PreloadApi } from '../../shared/preload-api/api-types'
import type { Repo } from '../../shared/repo-types'
import type { Worktree } from '../../shared/worktree/types'
import { createMockAdeApi } from '../create-api'
import { UnimplementedBridgeError } from '../unimplemented-fallback'
import { createAgentStatusRealApi } from './agent-status'
import { createAppRealApi } from './app'
import { createFsRealApi } from './fs'
import { createGhRealApi } from './gh'
import { createHostedReviewRealApi } from './hosted-review'
import { createNotificationsRealApi } from './notifications'
import { createOnboardingRealApi } from './onboarding'
import { createPlatformRealApi } from './platform'
import { createPreflightRealApi } from './preflight'
import { createProjectsRealApi } from './projects'
import { createPtyRealApi } from './pty'
import { createReposRealApi } from './repos'
import { createSessionRealApi } from './session'
import { createSettingsRealApi } from './settings'
import { createUiRealApi } from './ui'
import { createWorktreesRealApi } from './worktrees'

// Channel: pty 数据面（pty-stream）的下行载体；parity 只查方法处置面、从不
// spawn，故仅要求该导出存在，避免 mock 缺导出的访问期报错。
vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn(), Channel: class {} }))
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
  pty_list_sessions: [],
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

/**
 * pty 扁平契约方法的处置全集（Task 12 审查 Minor-3 的机械护栏）。键集由
 * `satisfies Record<keyof …>` 锁定为契约全集：往 `PtyApi` 新增契约方法而不在此
 * 处置，`pnpm typecheck` 直接报缺失键——「新增必须处置」是门禁失败而非 review
 * 记忆。`management` 是嵌套子对象，不适配 surfaceCases 的「函数性」断言，由
 * 下方专用用例以同样方式锁定。
 */
type PtyMethodDisposition = 'explicit' | 'missing'

const ptySurfaceDispositions = {
  // §2.1 控制面
  spawn: 'explicit',
  resize: 'explicit',
  signal: 'explicit',
  clearBuffer: 'explicit',
  kill: 'explicit',
  getCwd: 'explicit',
  getSize: 'explicit',
  hasPty: 'explicit',
  listSessions: 'explicit',
  // §2.1 数据面
  write: 'explicit',
  writeAccepted: 'explicit',
  onData: 'explicit',
  getPtyDataListenerCount: 'explicit',
  // §2.1 事件映射
  onExit: 'explicit',
  onSpawned: 'explicit',
  // §2.2 web-stub 同形缺省
  getForegroundProcess: 'explicit',
  confirmForegroundProcess: 'explicit',
  hasChildProcesses: 'explicit',
  inspectProcess: 'explicit',
  getMainBufferSnapshot: 'explicit',
  getAuthoritativeBufferSnapshotCapabilities: 'explicit',
  reportRendererDeliveryState: 'explicit',
  getRendererDeliveryDebugSnapshot: 'explicit',
  // §2.3 noop 订阅 / no-op 方法
  ackData: 'explicit',
  ackColdRestore: 'explicit',
  claimViewport: 'explicit',
  reportGeometry: 'explicit',
  onDeliveryResyncRequest: 'explicit',
  respondDeliveryResync: 'explicit',
  rendererDispatcherReady: 'explicit',
  setActiveRendererPty: 'explicit',
  setRendererPtyVisible: 'explicit',
  setHiddenRendererPty: 'explicit',
  setPtyDeliveryInterest: 'explicit',
  publishTerminalViewAttributes: 'explicit',
  onWriteUnavailable: 'explicit',
  onReplay: 'explicit',
  onModelRestoreNeeded: 'explicit',
  onSideEffect: 'explicit',
  getSideEffectSnapshot: 'explicit',
  onSerializeBufferRequest: 'explicit',
  onClearBufferRequest: 'explicit',
  sendSerializedBuffer: 'explicit',
  declarePendingPaneSerializer: 'explicit',
  settlePaneSerializer: 'explicit',
  clearPendingPaneSerializer: 'explicit',
  reportRendererSerializerReady: 'explicit',
  resetRendererDeliveryDebug: 'explicit'
} satisfies Record<Exclude<keyof PreloadApi['pty'], 'management'>, PtyMethodDisposition>

const ptyManagementSurface = Object.keys({
  listSessions: true,
  killAll: true,
  killOne: true,
  restart: true,
  macTccAttribution: true
} satisfies Record<keyof PtyManagementApi, boolean>)

const ptySurfaceMethods = (disposition: PtyMethodDisposition): string[] =>
  Object.entries(ptySurfaceDispositions)
    .filter(([, value]) => value === disposition)
    .map(([method]) => method)

/**
 * gh 扁平契约方法的处置全集（Task 8）。`satisfies Record<keyof …>` 把契约键集
 * 锁死：`GithubPullRequestApi`/`GithubWorkItemApi` 新增方法而不在此处置时
 * `pnpm typecheck` 直接失败。2D.1 只接线只读方法，其余（创建/变更/项目查询）
 * 保持 rejecting fallback。
 */
type GhMethodDisposition = 'explicit' | 'missing'

const ghSurfaceDispositions = {
  diagnoseAuth: 'explicit',
  rateLimit: 'explicit',
  repoSlug: 'explicit',
  repoUpstream: 'explicit',
  prForBranch: 'explicit',
  refreshPRNow: 'explicit',
  prChecks: 'explicit',
  prCheckDetails: 'explicit',
  onPRRefreshEvent: 'explicit',
  viewer: 'missing',
  enqueuePRRefresh: 'missing',
  reportVisiblePRRefreshCandidates: 'missing',
  prFileContents: 'missing',
  rerunPRChecks: 'missing',
  prComments: 'missing',
  setPRCommentReaction: 'missing',
  resolveReviewThread: 'missing',
  setPRFileViewed: 'missing',
  updatePRTitle: 'missing',
  mergePR: 'missing',
  setPRAutoMerge: 'missing',
  updatePRState: 'missing',
  markPRReadyForReview: 'missing',
  requestPRReviewers: 'missing',
  removePRReviewers: 'missing',
  addPRReviewCommentReply: 'missing',
  addPRReviewComment: 'missing',
  checkOrcaStarred: 'missing',
  starOrca: 'missing',
  issue: 'missing',
  workItem: 'missing',
  workItemByOwnerRepo: 'missing',
  workItemDetails: 'missing',
  notifyWorkItemMutated: 'missing',
  listIssues: 'missing',
  createIssue: 'missing',
  countWorkItems: 'missing',
  listWorkItems: 'missing',
  updateIssue: 'missing',
  addIssueComment: 'missing',
  listLabels: 'missing',
  listAssignableUsers: 'missing',
  onWorkItemMutated: 'missing',
  listAccessibleProjects: 'missing',
  resolveProjectRef: 'missing',
  listProjectViews: 'missing',
  getProjectViewTable: 'missing',
  projectWorkItemDetailsBySlug: 'missing',
  updateProjectItemField: 'missing',
  clearProjectItemField: 'missing',
  updateIssueBySlug: 'missing',
  updatePullRequestBySlug: 'missing',
  addIssueCommentBySlug: 'missing',
  updateIssueCommentBySlug: 'missing',
  deleteIssueCommentBySlug: 'missing',
  listLabelsBySlug: 'missing',
  listAssignableUsersBySlug: 'missing',
  listIssueTypesBySlug: 'missing',
  updateIssueTypeBySlug: 'missing'
} satisfies Record<keyof PreloadApi['gh'], GhMethodDisposition>

const ghSurfaceMethods = (disposition: GhMethodDisposition): string[] =>
  Object.entries(ghSurfaceDispositions)
    .filter(([, value]) => value === disposition)
    .map(([method]) => method)

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
      'create',
      'getBaseRefDefault',
      'searchBaseRefs',
      'searchBaseRefDetails',
      'onChanged',
      'onCloneProgress'
    ],
    missing: [
      'clone',
      'cloneRemote',
      'createRemote',
      'addRemote',
      'cloneAbort',
      'getGitUsername',
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
      'listLineage',
      'create',
      'remove',
      'forgetLocal',
      'forceDeletePreservedBranch',
      'updateMeta',
      'persistSortOrder',
      'onChanged',
      'onHeadIdentitiesChanged',
      'onBaseStatus',
      'onRemoteBranchConflict',
      'onCreateProgress',
      'onGitStatusMetadataChanged'
    ],
    missing: [
      'adoptProvisionedRoot',
      'prefetchCreateBase',
      'resolvePrBase',
      'resolveMrBase',
      'listLineageForHost',
      'updateLineage',
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
      'consumePendingOpenSettings',
      'consumePendingMarkdownFileOpens',
      'getZoomLevel',
      'setZoomLevel',
      'syncTrafficLights',
      'setMarkdownEditorFocused',
      'setRichMarkdownContextMenuTarget',
      'notifyWindowRevealed',
      ...uiNoopSubscriptions
    ],
    missing: [
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
      'setTerminalInputFocused',
      'setFloatingFocus',
      'setShortcutRecorderFocused',
      'minimize',
      'maximize',
      'isMaximized',
      'requestClose',
      'popupMenu',
      'confirmWindowClose'
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
  {
    domain: 'preflight',
    explicit: ['check', 'refreshAgents'],
    missing: []
  },
  {
    domain: 'pty',
    explicit: ptySurfaceMethods('explicit'),
    missing: ptySurfaceMethods('missing')
  },
  { domain: 'onboarding', explicit: ['get', 'update'], missing: [] },
  { domain: 'platform', explicit: ['get'], missing: [] },
  {
    domain: 'agentStatus',
    explicit: ['onSet', 'onClear', 'getSnapshot'],
    missing: [
      'inferInterrupt',
      'inferQuestionAnswered',
      'getMigrationUnsupportedSnapshot',
      'onMigrationUnsupported',
      'onMigrationUnsupportedClear',
      'onLegacyWorkerTerminalRecovery',
      'drop',
      'dropPersisted',
      'dropPersistedBatch',
      'reconcileEndedProcess',
      'dropByTabPrefix',
      'retirePaneAuthority',
      'restorePaneAuthority',
      'transferPaneAuthority'
    ]
  },
  {
    domain: 'notifications',
    explicit: [
      'getDesktopAwayState',
      'dispatch',
      'dismiss',
      'openSystemSettings',
      'getPermissionStatus',
      'probeDelivery',
      'playSound'
    ],
    missing: []
  },
  {
    domain: 'gh',
    explicit: ghSurfaceMethods('explicit'),
    missing: ghSurfaceMethods('missing')
  },
  {
    domain: 'hostedReview',
    explicit: ['forBranch'],
    missing: ['getCreationEligibility', 'create', 'createStacked']
  }
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
        if (/^on[A-Z]/.test(method)) {
          // Why: the fallback answers subscription-shaped methods with a no-op
          // unsubscriber (no host events on this host); renderers push the return
          // value into cleanup lists, so a rejection here would crash effects.
          expect(typeof real[method](() => {}), `${domain}.${method} no-op subscription`).toBe(
            'function'
          )
          continue
        }
        await expect(Promise.resolve(real[method]())).rejects.toBeInstanceOf(
          UnimplementedBridgeError
        )
      }
      warn.mockRestore()
    }
  )

  it('pty.management implements the full management sub-surface', () => {
    // `management` 嵌套子对象同样以 `satisfies Record<keyof …>` 锁键集（见
    // `ptyManagementSurface`）：契约新增而 real 未接，typecheck 失败。
    const management = createPtyRealApi().management
    for (const method of ptyManagementSurface) {
      expect(
        Object.prototype.hasOwnProperty.call(management, method),
        `pty.management.${method} must be explicitly implemented in real mode`
      ).toBe(true)
      expect(
        typeof management[method as keyof PtyManagementApi],
        `pty.management.${method} must be a function`
      ).toBe('function')
    }
  })
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
  },
  {
    // No `command`: listSessions rides the `{args}` envelope, which the shared
    // `toHaveBeenCalledWith(command)` single-arg form cannot express — the exact
    // envelope is asserted verbatim in pty.test.ts (invoke passthrough).
    name: 'pty.listSessions',
    real: () => createPtyRealApi().listSessions(),
    mock: () => createMockAdeApi().pty.listSessions(),
    assertShape: (value) => {
      expect(Array.isArray(value)).toBe(true)
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

  it('pty.onSpawned hands back an unsubscriber in both modes', async () => {
    const unlisten = vi.fn()
    listenMock.mockResolvedValueOnce(unlisten)
    const realStop = createPtyRealApi().onSpawned(() => {})
    const mockStop = createMockAdeApi().pty.onSpawned(() => {})
    expect(typeof realStop).toBe('function')
    expect(typeof mockStop).toBe('function')
    expect(listenMock).toHaveBeenCalledWith('pty:spawned', expect.any(Function))
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
    name: 'worktrees.adoptProvisionedRoot',
    real: () => createWorktreesRealApi().adoptProvisionedRoot({} as never),
    mock: () => createMockAdeApi().worktrees.adoptProvisionedRoot({} as never)
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

// Why a dedicated key-set lock: `PreloadApi` flattens the workspace-session
// contract, so `session` is already the method sub-surface — but like
// pty.management it gets its own explicit lock (withMethodFallback would
// otherwise fabricate any forgotten method instead of failing the gate).
const sessionSubApiSurface = [
  'get',
  'set',
  'patch',
  'flush',
  'readTerminalScrollback',
  'setSync'
] as const satisfies readonly (keyof PreloadApi['session'])[]

describe('mock/real parity: session sub-surface', () => {
  it('session implements the full renderer session sub-surface', () => {
    const real = createSessionRealApi().session
    for (const method of sessionSubApiSurface) {
      expect(
        Object.prototype.hasOwnProperty.call(real, method),
        `session.${method} must be explicitly implemented in real mode`
      ).toBe(true)
      expect(typeof real[method], `session.${method} must be a function`).toBe('function')
    }
  })

  it('preflight implements resolveAgentProviderSession in real mode', () => {
    const real = createPreflightRealApi() as unknown as Record<string, unknown>
    expect(Object.prototype.hasOwnProperty.call(real, 'resolveAgentProviderSession')).toBe(true)
    expect(typeof real.resolveAgentProviderSession).toBe('function')
    const mock = createMockAdeApi().preflight as unknown as Record<string, unknown>
    expect(typeof mock.resolveAgentProviderSession).toBe('function')
  })
})

function realApiFor(domain: keyof PreloadApi): unknown {
  switch (domain) {
    case 'repos':
      return createReposRealApi()
    case 'agentStatus':
      return createAgentStatusRealApi()
    case 'session':
      return createSessionRealApi()
    case 'fs':
      return createFsRealApi()
    case 'onboarding':
      return createOnboardingRealApi()
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
    case 'preflight':
      return createPreflightRealApi()
    case 'pty':
      return createPtyRealApi()
    case 'platform':
      return createPlatformRealApi()
    case 'notifications':
      return createNotificationsRealApi()
    case 'gh':
      return createGhRealApi()
    case 'hostedReview':
      return createHostedReviewRealApi()
    default:
      throw new Error(`no real factory for ${domain}`)
  }
}
