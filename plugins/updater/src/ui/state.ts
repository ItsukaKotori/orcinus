import type { PluginPanelPushMessage } from '../../../../src/shared/plugins/plugin-panel-bridge'
import type { UpdaterState } from '../shared/types'

export type UiState = {
  report: UpdaterState | null
  logs: string[]
}

export const INITIAL_UI_STATE: UiState = { report: null, logs: [] }

const MAX_LOG_LINES = 500

export function reducePush(state: UiState, message: PluginPanelPushMessage): UiState {
  if (message.topic === 'log') {
    const lines = (message.payload as { lines?: unknown } | null)?.lines
    if (!Array.isArray(lines)) {
      return state
    }
    const appended = lines.filter((line): line is string => typeof line === 'string')
    return { ...state, logs: [...state.logs, ...appended].slice(-MAX_LOG_LINES) }
  }
  if (message.topic === 'state') {
    return { ...state, report: message.payload as UpdaterState }
  }
  return state
}
