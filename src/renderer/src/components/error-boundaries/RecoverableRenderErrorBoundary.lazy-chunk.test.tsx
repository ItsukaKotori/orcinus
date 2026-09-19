// @vitest-environment happy-dom

import { Suspense, act, type ReactElement, type ReactNode } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { lazyWithRetry } from '@/lib/lazy-with-retry'
import { RecoverableRenderErrorBoundary } from './RecoverableRenderErrorBoundary'

const RELOAD_GUARD_KEY = 'orca:lazy-chunk-reload-attempted'
const LANDED_RELOAD_GUARD_VALUE = 'doc-before-the-reload'

globalThis.IS_REACT_ACT_ENVIRONMENT = true

function createContainer(): { container: HTMLDivElement; root: Root } {
  const container = document.createElement('div')
  document.body.appendChild(container)
  return { container, root: createRoot(container) }
}

function BoundaryHarness({ children }: { children: ReactNode }): ReactElement {
  return (
    <RecoverableRenderErrorBoundary boundaryId="page.automations">
      <Suspense fallback={<div>Loading...</div>}>{children}</Suspense>
    </RecoverableRenderErrorBoundary>
  )
}

async function flushReactWork(): Promise<void> {
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 0))
  })
}

describe('RecoverableRenderErrorBoundary lazy chunk containment', () => {
  let root: Root | null = null
  let container: HTMLDivElement | null = null
  let consoleError: ReturnType<typeof vi.spyOn>

  beforeEach(() => {
    window.sessionStorage.clear()
    consoleError = vi.spyOn(console, 'error').mockImplementation(() => undefined)
  })

  afterEach(() => {
    if (root) {
      act(() => root?.unmount())
    }
    container?.remove()
    root = null
    container = null
    window.sessionStorage.clear()
    consoleError.mockRestore()
  })

  it('renders the fallback after guarded dynamic import exhaustion', async () => {
    window.sessionStorage.setItem(RELOAD_GUARD_KEY, LANDED_RELOAD_GUARD_VALUE)
    const LazyRejectingImport = lazyWithRetry(
      () =>
        Promise.reject(
          new TypeError('Failed to fetch dynamically imported module: file://redacted/chunk.js')
        ),
      { retries: 0 }
    )
    ;({ container, root } = createContainer())

    await act(async () => {
      root?.render(
        <BoundaryHarness>
          <LazyRejectingImport />
        </BoundaryHarness>
      )
    })
    await flushReactWork()
    await flushReactWork()

    expect(container?.querySelector('[role="alert"]')).not.toBeNull()
  })

  it('contains ordinary render errors behind the fallback', async () => {
    function BrokenSurface(): ReactElement {
      throw new Error('ordinary render failure')
    }
    ;({ container, root } = createContainer())

    await act(async () => {
      root?.render(
        <BoundaryHarness>
          <BrokenSurface />
        </BoundaryHarness>
      )
    })

    expect(container?.querySelector('[role="alert"]')).not.toBeNull()
  })
})
