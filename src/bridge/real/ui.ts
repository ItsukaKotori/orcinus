import type { PreloadApi } from '../../shared/preload-api/api-types'
import { withMethodFallback } from '../unimplemented-fallback'
import { invokeCommand, subscribeToEvent } from './invoke'

const noopUnsubscribe = (): void => {}
const noopSubscription = (): (() => void) => noopUnsubscribe

/**
 * Events the A host never emits (tray/menu shortcuts, browser chrome, mobile
 * transport). The renderer probes or subscribes to them unconditionally, so
 * each must still hand back a working unsubscribe handle (spec §5.4).
 */
export const UI_NOOP_SUBSCRIPTION_METHODS = [
  'onActivateWorktree',
  'onAppMenuPaste',
  'onAppMenuSelectionAction',
  'onBrowserHistoryNavigate',
  'onCloseActiveTab',
  'onCloseFloatingItem',
  'onCloseSessionTab',
  'onCloseTerminal',
  'onCreateTerminal',
  'onCtrlTabKeyDown',
  'onCtrlTabKeyUp',
  'onDeleteCurrentWorkspace',
  'onEditableContextPaste',
  'onFileDrop',
  'onFindInBrowserPage',
  'onFocusBrowserAddressBar',
  'onFocusEditorTab',
  'onFocusTerminal',
  'onFullscreenChanged',
  'onHardReloadBrowserPage',
  'onJumpToTabIndex',
  'onJumpToWorktreeIndex',
  'onMaximizeChanged',
  'onMobileMarkdownRequest',
  'onMoveSessionTab',
  'onNewBrowserTab',
  'onNewMarkdownTab',
  'onNewTerminalTab',
  'onOpenDiffFromMobile',
  'onOpenFileFromMobile',
  'onOpenMarkdownFiles',
  'onOpenNewWorkspace',
  'onOpenQuickOpen',
  'onOpenSettings',
  'onOpenTasks',
  'onReloadBrowserPage',
  'onRenameTerminal',
  'onRequestTabClose',
  'onRequestTabCreate',
  'onRequestTabSetProfile',
  'onRequestTerminalCreate',
  'onRequestTerminalTabMount',
  'onResumeSleepingAgents',
  'onRichMarkdownContextCommand',
  'onScrollBrowserPage',
  'onSelectFloatingIndex',
  'onSessionTabCloseRequest',
  'onSleepWorktree',
  'onSplitTerminal',
  'onSwitchRecentTab',
  'onSwitchTab',
  'onSwitchTabAcrossAllTypes',
  'onSwitchTerminalTab',
  'onSystemResumed',
  'onTerminalShortcutCaptured',
  'onTerminalTabCloseRequest',
  'onTerminalZoom',
  'onToggleFloatingTerminal',
  'onToggleLeftSidebar',
  'onToggleQuickCommandsMenu',
  'onToggleRightSidebar',
  'onToggleStatusBar',
  'onToggleWorktreePalette',
  'onWindowCloseRequested',
  'onWorktreeHistoryNavigate',
  'onZoomBrowserPage'
] as const satisfies readonly (keyof PreloadApi['ui'])[]

type UiNoopSubscriptionMethod = (typeof UI_NOOP_SUBSCRIPTION_METHODS)[number]

function createNoopSubscriptions(): Pick<PreloadApi['ui'], UiNoopSubscriptionMethod> {
  // SAFETY: every listed method takes a callback and returns an unsubscribe
  // handle; the runtime map is exactly the Pick built by the list above.
  return Object.fromEntries(
    UI_NOOP_SUBSCRIPTION_METHODS.map((method) => [method, noopSubscription])
  ) as unknown as Pick<PreloadApi['ui'], UiNoopSubscriptionMethod>
}

/**
 * Real `ui` adapter (spec §5.4). State reads/writes and the interaction
 * counter map onto `ui_*` commands; the host broadcasts `ui:stateChanged`, and
 * every event it cannot emit yet is an explicit no-op subscription so
 * renderer `typeof` probes and cleanup handles stay honest.
 */
export function createUiRealApi(): PreloadApi['ui'] {
  return withMethodFallback<PreloadApi['ui']>('ui', {
    get: () => invokeCommand('ui_get'),
    set: (args) => invokeCommand('ui_set', { args }),
    setWithAck: (args) => invokeCommand('ui_set_with_ack', { args }),
    recordFeatureInteraction: (id) =>
      invokeCommand('ui_record_feature_interaction', { args: { id } }),
    onStateChanged: (callback) => subscribeToEvent('ui:stateChanged', callback),
    ...createNoopSubscriptions()
  })
}
