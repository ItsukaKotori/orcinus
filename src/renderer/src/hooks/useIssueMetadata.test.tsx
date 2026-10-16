// @vitest-environment happy-dom

import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { useRepoLabels } from './useIssueMetadata'

const runtimeMocks = vi.hoisted(() => ({ callRuntimeRpc: vi.fn() }))
const githubMocks = vi.hoisted(() => ({ listLabels: vi.fn() }))

vi.mock('@/runtime/runtime-rpc-client', () => ({
  callRuntimeRpc: runtimeMocks.callRuntimeRpc,
  getActiveRuntimeTarget: (settings?: { activeRuntimeEnvironmentId?: string | null } | null) =>
    settings?.activeRuntimeEnvironmentId
      ? { kind: 'environment', environmentId: settings.activeRuntimeEnvironmentId }
      : { kind: 'local' }
}))

const roots: Root[] = []

function installWindowApi(): void {
  Object.defineProperty(window, 'api', {
    configurable: true,
    value: { gh: { listLabels: githubMocks.listLabels } }
  })
}

async function flushEffects(): Promise<void> {
  await act(async () => {
    await Promise.resolve()
    await Promise.resolve()
  })
}

function renderProbe(element: React.ReactNode): void {
  const container = document.createElement('div')
  document.body.appendChild(container)
  const root = createRoot(container)
  roots.push(root)
  act(() => {
    root.render(element)
  })
}

describe('useIssueMetadata hooks', () => {
  beforeEach(() => {
    runtimeMocks.callRuntimeRpc.mockReset()
    githubMocks.listLabels.mockReset()
    installWindowApi()
  })

  afterEach(() => {
    roots.splice(0).forEach((root) => {
      act(() => root.unmount())
    })
    document.body.replaceChildren()
  })

  it('routes repo-id-only folder metadata through local IPC', async () => {
    let labels: string[] = []
    githubMocks.listLabels.mockResolvedValue(['folder'])

    function LabelsProbe(): null {
      labels = useRepoLabels(null, 'folder-repo-id').data
      return null
    }

    renderProbe(<LabelsProbe />)
    await flushEffects()

    expect(labels).toEqual(['folder'])
    expect(githubMocks.listLabels).toHaveBeenCalledExactlyOnceWith({
      repoPath: '',
      repoId: 'folder-repo-id'
    })
    expect(runtimeMocks.callRuntimeRpc).not.toHaveBeenCalled()
  })

  it('prefers an explicit remote environment and repo id', async () => {
    let labels: string[] = []
    runtimeMocks.callRuntimeRpc.mockResolvedValue(['remote'])

    function LabelsProbe(): null {
      labels = useRepoLabels('/local/repo', 'remote-repo-id', {
        runtimeEnvironmentId: ' env-explicit ',
        activeRuntimeEnvironmentId: 'env-active'
      }).data
      return null
    }

    renderProbe(<LabelsProbe />)
    await flushEffects()

    expect(labels).toEqual(['remote'])
    expect(runtimeMocks.callRuntimeRpc).toHaveBeenCalledExactlyOnceWith(
      { kind: 'environment', environmentId: 'env-explicit' },
      'github.listLabels',
      { repo: 'remote-repo-id' },
      { timeoutMs: 15_000 }
    )
    expect(githubMocks.listLabels).not.toHaveBeenCalled()
  })

})
