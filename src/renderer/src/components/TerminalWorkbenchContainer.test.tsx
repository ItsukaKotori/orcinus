// @vitest-environment happy-dom
import { cleanup, render } from '@testing-library/react'
import { afterEach, describe, expect, it } from 'vitest'
import { TerminalWorkbenchContainer } from './TerminalWorkbenchContainer'

function mountWorkbench(isVisible: boolean): HTMLElement {
  const { container } = render(
    <TerminalWorkbenchContainer isVisible={isVisible}>
      <span>workbench</span>
    </TerminalWorkbenchContainer>
  )
  const node = container.querySelector('[data-terminal-workbench-container]')
  if (!(node instanceof HTMLElement)) {
    throw new Error('workbench container not rendered')
  }
  return node
}

afterEach(cleanup)

describe('TerminalWorkbenchContainer', () => {
  it('parks with display:none when the workspace view is hidden', () => {
    expect(mountWorkbench(false).className).toContain('hidden')
  })

  it('renders normally on the workspace view', () => {
    const node = mountWorkbench(true)
    expect(node.className).not.toContain('hidden')
    expect(node.className).not.toContain('opacity-0')
    expect(node.hasAttribute('inert')).toBe(false)
  })

  it('stays unreachable by Tab and assistive tech while hidden', () => {
    const node = mountWorkbench(false)
    expect(node.hasAttribute('inert')).toBe(true)
    expect(node.getAttribute('aria-hidden')).toBe('true')
  })
})
