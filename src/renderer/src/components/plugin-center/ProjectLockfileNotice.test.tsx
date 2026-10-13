// @vitest-environment happy-dom

import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type {
  PluginProjectLockEntry,
  PluginProjectLockState
} from '../../../../preload/api-types'

const { storeState } = vi.hoisted(() => ({
  storeState: { activeRepoId: null as string | null }
}))

vi.mock('@/store', () => ({
  useAppStore: (selector: (store: typeof storeState) => unknown) => selector(storeState)
}))

import { ProjectLockfileNotice } from './ProjectLockfileNotice'

const hash = 'a'.repeat(64)

const entries: PluginProjectLockEntry[] = [
  {
    pluginKey: 'acme.installed',
    version: '1.0.0',
    source: { kind: 'bundled', bundleId: 'orca-samples.installed' },
    contentHash: hash,
    status: 'installed'
  },
  {
    pluginKey: 'acme.git',
    version: '1.1.0',
    source: { kind: 'git', url: 'https://example.com/acme.git', ref: 'v1.1.0' },
    contentHash: hash,
    status: 'missing'
  },
  {
    pluginKey: 'acme.local',
    version: '2.0.0',
    source: { kind: 'local-path', path: '/tmp/acme-local' },
    contentHash: hash,
    status: 'hash-mismatch',
    detail: 'content hash differs from the lockfile'
  },
  {
    pluginKey: 'acme.market',
    version: '3.0.0',
    source: {
      kind: 'marketplace',
      marketplace: {
        url: 'https://example.com/marketplace.git',
        ref: 'main',
        resolvedCommit: 'b'.repeat(40)
      },
      plugin: { url: 'https://example.com/acme-market.git', ref: 'main' }
    },
    contentHash: hash,
    status: 'source-unavailable',
    detail: 'remote unreachable'
  }
]

const readyState: PluginProjectLockState = {
  repoId: 'repo-1',
  lockfilePath: '/repo/.ade/plugins.lock.json',
  entries: entries.filter((entry) => entry.status === 'installed')
}

const unreadyState: PluginProjectLockState = {
  repoId: 'repo-1',
  lockfilePath: '/repo/.ade/plugins.lock.json',
  entries
}

const getProjectLockStateMock = vi.fn()
const installMissingFromLockfileMock = vi.fn()
const onChangedMock = vi.fn()

beforeEach(() => {
  storeState.activeRepoId = 'repo-1'
  getProjectLockStateMock.mockReset()
  installMissingFromLockfileMock.mockReset()
  onChangedMock.mockReset()
  getProjectLockStateMock.mockResolvedValue(unreadyState)
  installMissingFromLockfileMock.mockResolvedValue([{ ok: true }])
  Object.defineProperty(window, 'api', {
    configurable: true,
    value: {
      plugins: {
        getProjectLockState: getProjectLockStateMock,
        installMissingFromLockfile: installMissingFromLockfileMock
      }
    }
  })
})

afterEach(() => {
  cleanup()
  Reflect.deleteProperty(window, 'api')
  vi.restoreAllMocks()
})

async function renderNotice(): Promise<void> {
  render(<ProjectLockfileNotice onChanged={onChangedMock} />)
  await waitFor(() => expect(getProjectLockStateMock).toHaveBeenCalledWith({ repoId: 'repo-1' }))
}

describe('ProjectLockfileNotice', () => {
  it('stays hidden when every lockfile entry is installed', async () => {
    getProjectLockStateMock.mockResolvedValue(readyState)

    await renderNotice()

    expect(screen.queryByText(/项未就绪/)).toBeNull()
  })

  it('stays hidden and does not query without an active project', async () => {
    storeState.activeRepoId = null

    render(<ProjectLockfileNotice onChanged={onChangedMock} />)
    await Promise.resolve()

    expect(getProjectLockStateMock).not.toHaveBeenCalled()
    expect(screen.queryByText(/项未就绪/)).toBeNull()
  })

  it('shows the unready count banner when statuses are mixed', async () => {
    await renderNotice()

    expect(screen.getByText('3 项未就绪')).toBeTruthy()
    expect(getProjectLockStateMock).toHaveBeenCalledTimes(1)
  })

  it('renders every entry with its status, version, source, and detail in the dialog', async () => {
    await renderNotice()

    fireEvent.click(screen.getByText('3 项未就绪'))

    const dialog = await screen.findByRole('dialog')
    expect(dialog.textContent).toContain('acme.installed')
    expect(dialog.textContent).toContain('v1.0.0')
    expect(dialog.textContent).toContain('已安装')
    expect(dialog.textContent).toContain('acme.git')
    expect(dialog.textContent).toContain('Git https://example.com/acme.git#v1.1.0')
    expect(dialog.textContent).toContain('缺失')
    expect(dialog.textContent).toContain('哈希不符')
    expect(dialog.textContent).toContain('content hash differs from the lockfile')
    expect(dialog.textContent).toContain('来源不可用')
    expect(dialog.textContent).toContain('remote unreachable')
  })

  it('runs the batch install with the repoId, refreshes the lock state and notifies the host', async () => {
    getProjectLockStateMock
      .mockResolvedValueOnce(unreadyState)
      .mockResolvedValueOnce(readyState)

    await renderNotice()
    fireEvent.click(screen.getByText('3 项未就绪'))
    fireEvent.click(await screen.findByRole('button', { name: '安装缺失' }))

    await waitFor(() =>
      expect(installMissingFromLockfileMock).toHaveBeenCalledWith({ repoId: 'repo-1' })
    )
    await waitFor(() => expect(onChangedMock).toHaveBeenCalledTimes(1))
    await waitFor(() => expect(getProjectLockStateMock).toHaveBeenCalledTimes(2))
    await waitFor(() => expect(screen.queryByText(/项未就绪/)).toBeNull())
  })

  it('disables the batch install when no entry can be reinstalled', async () => {
    getProjectLockStateMock.mockResolvedValue({
      ...unreadyState,
      entries: entries.filter((entry) => entry.status === 'source-unavailable')
    })

    await renderNotice()
    fireEvent.click(screen.getByText('1 项未就绪'))

    const installButton = await screen.findByRole('button', { name: '安装缺失' })
    expect(installButton.hasAttribute('disabled')).toBe(true)
  })
})
