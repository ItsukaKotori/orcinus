// @vitest-environment happy-dom

import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { RemoteFileBrowser } from './RemoteFileBrowser'

const browserMock = vi.hoisted(() => ({
  pathFlavor: 'posix' as 'posix' | 'win32',
  entries: [
    { name: 'src', isDirectory: true },
    { name: 'README.md', isDirectory: false }
  ] as Array<{ name: string; isDirectory: boolean }>,
  calls: [] as Array<{ environmentId: string; dirPath: string }>
}))

vi.mock('@/runtime/runtime-server-directory-browser', () => ({
  browseRuntimeServerDirectory: async (environmentId: string, dirPath: string) => {
    browserMock.calls.push({ environmentId, dirPath })
    return {
      entries: browserMock.entries,
      resolvedPath:
        dirPath === '~'
          ? browserMock.pathFlavor === 'win32'
            ? 'C:/Users/alice'
            : '/home/alice'
          : dirPath,
      pathFlavor: browserMock.pathFlavor
    }
  }
}))

async function flushPromises(count = 4): Promise<void> {
  for (let index = 0; index < count; index += 1) {
    await Promise.resolve()
  }
}

async function renderRemoteFileBrowser(): Promise<{
  container: HTMLDivElement
  input: HTMLInputElement
  root: Root
}> {
  const container = document.createElement('div')
  document.body.appendChild(container)
  const root = createRoot(container)

  await act(async () => {
    root.render(<RemoteFileBrowser runtimeEnvironmentId="env-1" onSelect={vi.fn()} onCancel={vi.fn()} />)
    await flushPromises()
  })

  const input = container.querySelector('input')
  if (!input) {
    throw new Error('Remote file browser input was not rendered')
  }

  return { container, input, root }
}

async function changeInput(input: HTMLInputElement, value: string): Promise<void> {
  await act(async () => {
    const valueSetter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')?.set
    valueSetter?.call(input, value)
    input.dispatchEvent(new Event('input', { bubbles: true }))
  })
}

async function advancePathResolveDebounce(): Promise<void> {
  await act(async () => {
    vi.advanceTimersByTime(300)
    await flushPromises()
  })
}

describe('RemoteFileBrowser paste-sized input', () => {
  beforeEach(() => {
    ;(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true
    vi.useFakeTimers()
    browserMock.pathFlavor = 'posix'
    browserMock.entries = [
      { name: 'src', isDirectory: true },
      { name: 'README.md', isDirectory: false }
    ]
    browserMock.calls.length = 0
  })

  afterEach(() => {
    vi.useRealTimers()
    document.body.replaceChildren()
    vi.restoreAllMocks()
  })

  it('still debounces and resolves ordinary path-mode input', async () => {
    const { input, root } = await renderRemoteFileBrowser()
    browserMock.calls.length = 0

    await changeInput(input, 'src/')
    expect(browserMock.calls).toHaveLength(0)

    await advancePathResolveDebounce()

    expect(browserMock.calls).toContainEqual({
      environmentId: 'env-1',
      dirPath: '/home/alice/src'
    })

    await act(async () => {
      root.unmount()
    })
  })

  it('navigates a typed Windows drive root and renders its breadcrumb', async () => {
    browserMock.pathFlavor = 'win32'
    const { container, input, root } = await renderRemoteFileBrowser()
    browserMock.calls.length = 0

    await changeInput(input, 'M:\\')
    await advancePathResolveDebounce()

    expect(browserMock.calls).toContainEqual({ environmentId: 'env-1', dirPath: 'M:\\' })
    await act(async () => {
      input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }))
      await flushPromises()
    })
    expect(
      [...container.querySelectorAll('button')].some((button) => button.textContent === 'M:')
    ).toBe(true)

    await act(async () => {
      root.unmount()
    })
  })

  it('keeps a drive-shaped POSIX directory as an ordinary filtered row', async () => {
    browserMock.entries = [{ name: 'M:\\', isDirectory: true }]
    const { container, input, root } = await renderRemoteFileBrowser()
    browserMock.calls.length = 0

    await changeInput(input, 'M:\\')
    await advancePathResolveDebounce()

    expect(browserMock.calls).toHaveLength(0)
    expect(container.textContent).toContain('M:\\')

    await act(async () => {
      root.unmount()
    })
  })

  it('does not parse or remotely resolve oversized slash-containing paste text', async () => {
    const { container, input, root } = await renderRemoteFileBrowser()
    const pastedSecretPathList = 'C:/Users/alice/project/secret-token-value.txt\n'.repeat(2_000)
    browserMock.calls.length = 0

    await changeInput(input, pastedSecretPathList)
    await advancePathResolveDebounce()

    expect(browserMock.calls).toHaveLength(0)
    expect(container.textContent).toContain('No matches for this long input')
    expect(container.textContent).not.toContain('secret-token-value')

    await act(async () => {
      root.unmount()
    })
  })
})
