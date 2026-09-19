// @vitest-environment happy-dom

import { Suspense, act, type ReactElement, type ReactNode } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { lazyWithRetry } from '@/lib/lazy-with-retry'
import { RichMarkdownErrorBoundary } from './RichMarkdownErrorBoundary'

const RELOAD_GUARD_KEY = 'orca:lazy-chunk-reload-attempted'
const LANDED_RELOAD_GUARD_VALUE = 'doc-before-the-reload'

globalThis.IS_REACT_ACT_ENVIRONMENT = true

function createContainer(): { container: HTMLDivElement; root: Root } {
  const container = document.createElement('div')
  document.body.appendChild(container)
  return { container, root: createRoot(container) }
}

function BoundaryHarness({
  children,
  boundaryKey = 'pane-a'
}: {
  children: ReactNode
  boundaryKey?: string
}): ReactElement {
  return (
    <Suspense fallback={<div>Loading...</div>}>
      <RichMarkdownErrorBoundary key={boundaryKey} fileId="file-1">
        {children}
      </RichMarkdownErrorBoundary>
    </Suspense>
  )
}

async function flushReactWork(): Promise<void> {
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 0))
  })
}

const CORRUPT_CHUNK_PARSE_ERROR = "Unexpected token ':'"

describe('RichMarkdownErrorBoundary lazy chunk containment', () => {
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

  it('contains a corrupt lazy chunk behind the fallback instead of blacking out the renderer', async () => {
    window.sessionStorage.setItem(RELOAD_GUARD_KEY, LANDED_RELOAD_GUARD_VALUE)
    const LazyRejectingImport = lazyWithRetry(
      () => Promise.reject(new SyntaxError(CORRUPT_CHUNK_PARSE_ERROR)),
      { retries: 0, reloadKey: 'rich-markdown-editor' }
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

    expect(container?.textContent).toContain('rich markdown editor')
    expect(container?.querySelector('button')).not.toBeNull()
  })

  it('recovers through the Retry button after an ordinary render error', async () => {
    let shouldThrow = true
    function FlakyEditor(): ReactElement {
      if (shouldThrow) {
        throw new Error('ordinary render failure')
      }
      return <div>editor content</div>
    }
    ;({ container, root } = createContainer())

    await act(async () => {
      root?.render(
        <BoundaryHarness>
          <FlakyEditor />
        </BoundaryHarness>
      )
    })
    expect(container?.textContent).toContain('rich markdown editor')

    shouldThrow = false
    await act(async () => {
      container?.querySelector('button')?.click()
    })
    await flushReactWork()

    expect(container?.textContent).toContain('editor content')
  })
})
