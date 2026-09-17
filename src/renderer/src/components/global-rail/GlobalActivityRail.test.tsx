// @vitest-environment happy-dom
import { cleanup, render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, beforeAll, describe, expect, it } from 'vitest'
import { GlobalActivityRail } from './GlobalActivityRail'
import { useAppStore } from '@/store'
import { installAdeBridge } from '../../../../bridge/install'

describe('GlobalActivityRail', () => {
  beforeAll(() => {
    installAdeBridge()
  })

  afterEach(cleanup)

  it('switches to plugin-center view on click', async () => {
    render(<GlobalActivityRail />)
    await userEvent.click(screen.getByRole('button', { name: '插件中心' }))
    expect(useAppStore.getState().activeView).toBe('plugin-center')
  })

  it('renders an entry per installed global plugin', async () => {
    await useAppStore.getState().loadPluginCenterEntries()
    render(<GlobalActivityRail />)
    expect(screen.getByRole('button', { name: 'Database Manager' })).toBeTruthy()
  })

  it('opens the settings surface from the bottom settings entry', async () => {
    render(<GlobalActivityRail />)

    await userEvent.click(screen.getByRole('button', { name: '设置' }))

    expect(useAppStore.getState().activeView).toBe('settings')
  })

  it('opens the settings surface from the bottom help entry', async () => {
    render(<GlobalActivityRail />)

    await userEvent.click(screen.getByRole('button', { name: '帮助' }))

    expect(useAppStore.getState().activeView).toBe('settings')
  })
})
