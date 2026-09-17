// Phase 0 mock; replaced by Tauri IPC per view.
import type { AppApi } from '../../preload/api/app-api'

const MOCK_HOME = 'C:\\Users\\ade'

export function createAppApi(): AppApi {
  return {
    getIdentity: async () => ({
      name: 'Ade',
      isDev: true,
      devLabel: null,
      devBranch: null,
      devWorktreeName: null,
      devRepoRoot: null,
      dockBadgeLabel: null
    }),
    relaunch: async () => {},
    restart: async () => {},
    reload: async () => {},
    stageBeforeUnloadSync: (_args) => {},
    awaitBeforeUnloadCheckpoint: async () => {},
    awaitFirstWindowStartupServices: async () => {},
    awaitGitEnvironmentStartupBarrier: async () => {},
    prepareTerminalStartupRestoration: async () => {},
    recoverLegacyWorkerTerminalsForRendererStartup: async () => {},
    startupDiagnostic: async (_event, _details) => {},
    getKeyboardInputSourceId: async () => null,
    getMacCapturedDigitRowChords: async () => [],
    getKeyboardLayoutSnapshot: async () => null,
    onKeyboardLayoutChanged: (_callback) => () => {},
    setUnreadDockBadgeCount: async (_count) => {},
    getFloatingTerminalCwd: async (_args) => MOCK_HOME,
    getFloatingMarkdownDirectory: async () => `${MOCK_HOME}\\orca`,
    pickFloatingMarkdownDocument: async () => null,
    pickFloatingWorkspaceDirectory: async () => null,
    writeTerminalRenderDesyncEvidence: async (_args) => ({
      directory: `${MOCK_HOME}\\orca`,
      pngPath: `${MOCK_HOME}\\orca\\mock-desync.png`,
      metadataPath: null
    })
  }
}
