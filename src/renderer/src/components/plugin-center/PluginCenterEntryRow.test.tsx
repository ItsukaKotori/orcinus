// @vitest-environment happy-dom
import { render, screen } from '@testing-library/react'
import { describe, expect, it } from 'vitest'
import { PluginCenterEntryRow } from './PluginCenterEntryRow'
import { MOCK_GLOBAL_PLUGINS, MOCK_PROJECT_PLUGINS } from '../../../../bridge/mock/fixtures'

describe('PluginCenterEntryRow', () => {
  it('shows scope badge and plugin identity', () => {
    render(<PluginCenterEntryRow entry={MOCK_GLOBAL_PLUGINS[0]} onToggle={() => {}} onOpenLogs={() => {}} />)
    expect(screen.getByText('Database Manager')).toBeTruthy()
    expect(screen.getByText('全局')).toBeTruthy()
  })

  it('reads the real project scope field for the badge', () => {
    render(<PluginCenterEntryRow entry={MOCK_PROJECT_PLUGINS[0]} onToggle={() => {}} onOpenLogs={() => {}} />)
    expect(screen.getByText('Repo Notes')).toBeTruthy()
    expect(screen.getByText('当前项目')).toBeTruthy()
  })
})
