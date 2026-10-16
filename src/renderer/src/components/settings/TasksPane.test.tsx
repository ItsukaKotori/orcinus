// @vitest-environment happy-dom

import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { renderToStaticMarkup } from 'react-dom/server'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { GlobalSettings } from '../../../../shared/global-settings-types'
import type { TaskProvider } from '../../../../shared/task-providers'
import type { TaskProviderReadiness } from './task-source-setup-state'
import { TasksPane } from './TasksPane'

const mocks = vi.hoisted(() => ({
  readiness: {} as Record<TaskProvider, TaskProviderReadiness>,
  openSettingsTarget: vi.fn(),
  openSettingsPage: vi.fn(),
  refreshPreflightStatus: vi.fn()
}))

vi.mock('./use-task-source-provider-readiness', () => ({
  useTaskSourceProviderReadiness: () => mocks.readiness
}))

vi.mock('./use-integration-provider-status-refresh', () => ({
  useIntegrationProviderStatusRefresh: vi.fn()
}))

vi.mock('./TaskSourceSimpleSetup', () => ({
  CodeHostSetupSteps: (props: {
    providerLabel: string
    unavailable?: boolean
    onRetryConnection: () => void
  }) => (
    <div data-testid={`code-host-${props.providerLabel}`}>
      {props.unavailable ? (
        <>
          <span>Orca couldn&apos;t check this connection</span>
          <button type="button" onClick={props.onRetryConnection}>
            Try again
          </button>
        </>
      ) : (
        'Code host setup'
      )}
    </div>
  )
}))

vi.mock('@/store', () => ({
  useAppStore: (
    selector: (state: {
      openSettingsPage: () => void
      openSettingsTarget: (target: unknown) => void
      refreshPreflightStatus: () => void
      settingsSearchQuery: string
    }) => unknown
  ) =>
    selector({
      openSettingsPage: mocks.openSettingsPage,
      openSettingsTarget: mocks.openSettingsTarget,
      refreshPreflightStatus: mocks.refreshPreflightStatus,
      settingsSearchQuery: ''
    })
}))

const baseSettings = {
  visibleTaskProviders: ['github'],
  defaultTaskSource: 'github'
} as GlobalSettings

const INCOMPLETE_BANNER = 'Some visible providers still need setup'

function renderPane(): string {
  return renderToStaticMarkup(<TasksPane settings={baseSettings} updateSettings={vi.fn()} />)
}

let root: Root | null = null
let container: HTMLDivElement | null = null

async function renderInteractivePane(): Promise<HTMLDivElement> {
  container = document.createElement('div')
  document.body.appendChild(container)
  root = createRoot(container)
  await act(async () => {
    root?.render(<TasksPane settings={baseSettings} updateSettings={vi.fn()} />)
  })
  return container
}

describe('TasksPane', () => {
  beforeEach(() => {
    mocks.openSettingsPage.mockClear()
    mocks.openSettingsTarget.mockClear()
    mocks.readiness = {
      github: { connected: true, checking: false, visible: true },
      gitlab: { connected: false, checking: false, visible: false },
      linear: { connected: false, checking: false, visible: false },
      jira: { connected: false, checking: false, visible: false }
    }
  })

  afterEach(async () => {
    if (root) {
      await act(async () => {
        root?.unmount()
      })
    }
    root = null
    container?.remove()
    container = null
  })

  it('frames Task Sources as a guided setup hub, not visibility-only toggles', () => {
    const markup = renderPane()

    expect(markup).toContain('Task management setup')
    expect(markup).toContain('GitHub')
    expect(markup).toContain('Ready')
  })

  it('does not warn on a fresh install where nothing is connected yet', () => {
    // Settings ship with the provider visible, so untouched providers are the
    // default state; the card still says "Connect required" on its own.
    mocks.readiness.github = { connected: false, checking: false, visible: true }

    const markup = renderPane()

    expect(markup).not.toContain(INCOMPLETE_BANNER)
    expect(markup).toContain('Connect required')
  })

  it('hides the incomplete banner when the visible provider is ready', () => {
    expect(renderPane()).not.toContain(INCOMPLETE_BANNER)
  })

  it('does not warn or expand while connection checks are still in flight', () => {
    mocks.readiness.github = { connected: false, checking: true, visible: true }

    const markup = renderPane()

    expect(markup).not.toContain(INCOMPLETE_BANNER)
    expect(markup).not.toContain('Code host setup')
  })

  it('does not warn about an unconnected code host (single-step setup never stalls)', () => {
    mocks.readiness.github = { connected: false, checking: false, visible: true }

    expect(renderPane()).not.toContain(INCOMPLETE_BANNER)
  })

  it('shows a retry action instead of setup instructions when preflight is unavailable', async () => {
    mocks.readiness.github = {
      connected: false,
      checking: false,
      unavailable: true,
      visible: true
    }
    await renderInteractivePane()

    expect(container?.textContent).toContain('Orca couldn&apos;t check this connection'.replace('&apos;', "'"))
    const retry = Array.from(container?.querySelectorAll('button') ?? []).find(
      (button) => button.textContent === 'Try again'
    )
    expect(retry).toBeDefined()

    await act(async () => {
      retry?.click()
    })
    expect(mocks.refreshPreflightStatus).toHaveBeenCalledWith({ force: true })
  })

  it('deep-links credential management to the integrations pane', async () => {
    await renderInteractivePane()

    const integrationsButton = Array.from(container?.querySelectorAll('button') ?? []).find(
      (button) => button.textContent === 'Integrations'
    )
    expect(integrationsButton).toBeDefined()

    await act(async () => {
      integrationsButton?.click()
    })

    expect(mocks.openSettingsPage).toHaveBeenCalledOnce()
    expect(mocks.openSettingsTarget).toHaveBeenCalledWith({
      pane: 'integrations',
      repoId: null
    })
  })
})
