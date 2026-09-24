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
  methods: string[]
}

describe('mock/real parity: method surface', () => {
  it.each<SurfaceCase>([
    {
      domain: 'repos',
      methods: ['list', 'add', 'update', 'remove', 'reorderForHost', 'pickFolder', 'onChanged']
    },
    { domain: 'fs', methods: ['readDir', 'readFile', 'writeFile', 'onFsChanged', 'downloadFile'] },
    { domain: 'projects', methods: ['list', 'listHostSetups', 'update'] },
    {
      domain: 'worktrees',
      methods: ['list', 'listAll', 'onChanged', 'create', 'listDetected']
    },
    { domain: 'settings', methods: ['get', 'getSync', 'set', 'onChanged'] },
    { domain: 'ui', methods: ['get', 'set', 'recordFeatureInteraction', 'onStateChanged'] },
    { domain: 'app', methods: ['getIdentity', 'relaunch'] },
    { domain: 'platform', methods: ['get'] }
  ])('$domain exposes every contract method in both modes', ({ domain, methods }) => {
    const mock = createMockAdeApi()[domain] as unknown as Record<string, unknown>
    const real = realApiFor(domain) as unknown as Record<string, unknown>
    for (const method of methods) {
      expect(typeof mock[method]).toBe('function')
      expect(typeof real[method]).toBe('function')
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
