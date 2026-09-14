// @vitest-environment happy-dom
import { render, screen } from '@testing-library/react'
import { describe, expect, it } from 'vitest'
import { PluginCenterEntryRow } from './PluginCenterEntryRow'
import { MOCK_GLOBAL_PLUGINS } from '../../../../bridge/mock/fixtures'

describe('PluginCenterEntryRow', () => {
  it('shows scope badge and plugin identity', () => {
    render(<PluginCenterEntryRow entry={MOCK_GLOBAL_PLUGINS[0]} onToggle={() => {}} onOpenLogs={() => {}} />)
    expect(screen.getByText('Database Manager')).toBeTruthy()
    expect(screen.getByText('全局')).toBeTruthy()
  })
})
