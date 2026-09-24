// @vitest-environment happy-dom
import { invoke } from '@tauri-apps/api/core'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { installAdeBridge } from './install'

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }))
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn() }))

const invokeMock = vi.mocked(invoke)

afterEach(() => {
  vi.unstubAllEnvs()
})

describe('installAdeBridge', () => {
  it('installs the mock bridge when the caller asks for it', async () => {
    installAdeBridge({ mode: 'mock' })
    await expect(window.api.repos.list()).resolves.toEqual([])
    expect(invokeMock).not.toHaveBeenCalled()
  })

  it('defaults to the real bridge', async () => {
    invokeMock.mockResolvedValueOnce([])
    installAdeBridge()
    await window.api.repos.list()
    expect(invokeMock).toHaveBeenCalledWith('repos_list')
  })
})
