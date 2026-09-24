import { invoke } from '@tauri-apps/api/core'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { UnimplementedBridgeError } from '../unimplemented-fallback'
import { invokeCommand, toRendererError } from './invoke'

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }))
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn() }))

const invokeMock = vi.mocked(invoke)

beforeEach(() => {
  invokeMock.mockReset()
})

describe('invokeCommand', () => {
  it('calls a no-arg command without a payload', async () => {
    invokeMock.mockResolvedValueOnce(['repo'])
    await expect(invokeCommand('repos_list')).resolves.toEqual(['repo'])
    expect(invokeMock).toHaveBeenCalledWith('repos_list')
  })

  it('wraps method args in the { args } envelope', async () => {
    invokeMock.mockResolvedValueOnce({ ok: true })
    await invokeCommand('repos_add', { args: { path: '/tmp/x' } })
    expect(invokeMock).toHaveBeenCalledWith('repos_add', { args: { path: '/tmp/x' } })
  })

  it('maps a {message} rejection to a normal Error', async () => {
    invokeMock.mockRejectedValueOnce({ message: 'Repo not found: r1' })
    await expect(invokeCommand('repos_update', { args: {} })).rejects.toThrow('Repo not found: r1')
  })
})

describe('toRendererError', () => {
  it('passes Error instances through untouched', () => {
    const error = new Error('boom')
    expect(toRendererError(error)).toBe(error)
  })

  it('passes UnimplementedBridgeError through untouched', () => {
    const error = new UnimplementedBridgeError('repos.clone')
    expect(toRendererError(error)).toBe(error)
  })

  it('wraps a non-object rejection in an Error', () => {
    expect(toRendererError('transport down')).toEqual(new Error('transport down'))
  })

  it('wraps an object without a string message in an Error', () => {
    expect(toRendererError({ message: 42 }).message).toBe('[object Object]')
  })
})
