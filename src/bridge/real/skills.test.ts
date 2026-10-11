import { invoke } from '@tauri-apps/api/core'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { SkillDiscoveryResult } from '../../shared/skills'
import { UnimplementedBridgeError } from '../unimplemented-fallback'
import { createSkillsRealApi } from './skills'

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }))

const invokeMock = vi.mocked(invoke)

const SCANNED_AT = '2026-10-11T12:00:00.000Z'
const EMPTY_COMMAND_RESULT = { skills: [], sources: [], scannedAt: SCANNED_AT }

beforeEach(() => {
  invokeMock.mockReset()
})

describe('skills real adapter', () => {
  it('routes discover to skills_discover and converts scannedAt to epoch millis', async () => {
    const skill = {
      id: '/home/user/.claude/skills/alpha/SKILL.md',
      name: 'Alpha',
      sourceKind: 'home'
    }
    const source = { id: 'home-claude', label: 'Claude home', exists: true }
    invokeMock.mockResolvedValueOnce({ skills: [skill], sources: [source], scannedAt: SCANNED_AT })

    const api = createSkillsRealApi()
    const result: SkillDiscoveryResult = await api.discover()

    expect(invokeMock).toHaveBeenLastCalledWith('skills_discover', { args: undefined })
    expect(result.scannedAt).toBe(Date.parse(SCANNED_AT))
    expect(result.skills).toEqual([skill])
    expect(result.sources).toEqual([source])
  })

  it('forwards refresh only and drops host-identifying target fields', async () => {
    invokeMock.mockResolvedValue(EMPTY_COMMAND_RESULT)
    const api = createSkillsRealApi()

    await api.discover({ refresh: true })
    expect(invokeMock).toHaveBeenLastCalledWith('skills_discover', { args: { refresh: true } })

    await api.discover({ refresh: false, runtime: 'wsl', wslDistro: 'Ubuntu' })
    expect(invokeMock).toHaveBeenLastCalledWith('skills_discover', { args: { refresh: false } })

    await api.discover({ runtime: 'host' })
    expect(invokeMock).toHaveBeenLastCalledWith('skills_discover', { args: undefined })
  })

  it('surfaces command failures as Error instances', async () => {
    invokeMock.mockRejectedValueOnce({ message: 'boom' })
    await expect(createSkillsRealApi().discover()).rejects.toThrow('boom')
  })

  it('keeps the remaining skills methods on the rejecting fallback', async () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {})
    const api = createSkillsRealApi()

    expect(Object.prototype.hasOwnProperty.call(api, 'freshnessInventory')).toBe(false)
    await expect(api.freshnessInventory()).rejects.toBeInstanceOf(UnimplementedBridgeError)
    warn.mockRestore()
  })
})
