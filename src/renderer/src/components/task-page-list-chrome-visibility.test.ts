import { describe, expect, it } from 'vitest'

import {
  shouldHideTaskPageListChrome,
  type TaskPageListChromeVisibilityState
} from './task-page-list-chrome-visibility'

const baseState: TaskPageListChromeVisibilityState = {
  taskSource: 'github',
  hasGitHubDetail: false
}

describe('shouldHideTaskPageListChrome', () => {
  it('hides chrome for the active provider detail context', () => {
    expect(
      shouldHideTaskPageListChrome({
        ...baseState,
        taskSource: 'github',
        hasGitHubDetail: true
      })
    ).toBe(true)
  })

  it('keeps chrome visible when no detail state is active', () => {
    expect(
      shouldHideTaskPageListChrome({
        ...baseState,
        taskSource: 'github'
      })
    ).toBe(false)
  })
})
