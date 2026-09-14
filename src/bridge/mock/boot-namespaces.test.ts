import { describe, expect, it } from 'vitest'
import { createCliApi } from './cli-api'
import { createOnboardingApi } from './onboarding-api'
import { createReposApi } from './repos-api'
import { createRuntimeEnvironmentsApi } from './runtime-environments-api'
import { createRemoteWorkspaceApi, createSessionApi } from './workspace-session-api'

describe('Phase 0 boot namespace mocks', () => {
  it('session: startup hydration read and persistence patch resolve', async () => {
    const session = createSessionApi()
    await expect(session.get()).resolves.toBeDefined()
    await expect(session.patch({})).resolves.toBeUndefined()
  })

  it('remoteWorkspace: client id, upload, and change subscription are benign', async () => {
    const remoteWorkspace = createRemoteWorkspaceApi()
    await expect(remoteWorkspace.clientId()).resolves.toBe('mock-client-id')
    await expect(
      remoteWorkspace.setForConnectedTargets({
        expectedRevisionsByTargetId: {},
        expectedHostObservationTokensByTargetId: {}
      })
    ).resolves.toEqual([])
    const unsubscribe = remoteWorkspace.onChanged(() => {})
    expect(typeof unsubscribe).toBe('function')
    unsubscribe()
  })

  it('onboarding: get resolves the default state', async () => {
    const onboarding = createOnboardingApi()
    await expect(onboarding.get()).resolves.toBeDefined()
  })

  it('cli: getInstallStatus resolves a not-installed status', async () => {
    const cli = createCliApi()
    await expect(cli.getInstallStatus()).resolves.toMatchObject({ state: 'not_installed' })
  })

  it('runtimeEnvironments: list, snapshots, and status subscription are benign', async () => {
    const runtimeEnvironments = createRuntimeEnvironmentsApi()
    await expect(runtimeEnvironments.list()).resolves.toEqual([])
    await expect(runtimeEnvironments.getStatusSnapshots()).resolves.toEqual([])
    const unsubscribe = runtimeEnvironments.onStatusChanged(() => {})
    expect(typeof unsubscribe).toBe('function')
    unsubscribe()
  })

  it('repos: list resolves empty and onChanged subscribes synchronously', async () => {
    const repos = createReposApi()
    await expect(repos.list()).resolves.toEqual([])
    const unsubscribe = repos.onChanged(() => {})
    expect(typeof unsubscribe).toBe('function')
    unsubscribe()
  })
})
