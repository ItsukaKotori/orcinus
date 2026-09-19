import { useCallback, useEffect } from 'react'
import type { CSSProperties } from 'react'
import {
  DEFAULT_TERMINAL_DIVIDER_DARK,
  isTerminalBackgroundLight,
  normalizeColor,
  resolveEffectiveTerminalAppearance,
  resolveOpaqueTerminalBackground
} from '@/lib/terminal-theme'
import { stripSshReconnectOwnedErrorLines } from './TerminalErrorToast'
import { mapPaneTerminalErrors, terminalErrorForPane } from './terminal-error-accumulation'
import { canContinueAgentSessionInNewSession } from './terminal-agent-session-continuation'
import { resolvePaneTitleAgent } from './terminal-pane-title-agent'
import type { TerminalPaneMobileController } from './use-terminal-pane-mobile-actions'
import { useAppStore } from '@/store'
import { makePaneKey } from '../../../../shared/stable-pane-id'
import { resolvePaneAgentSessionId } from './pane-agent-session-id'

export function useTerminalPaneProjection(controller: TerminalPaneMobileController) {
  const {
    contextMenu,
    contextMenuLeafId,
    expectedLayoutLeafIds,
    isActive,
    isVisible,
    managerRef,
    paneTitles,
    runtimePaneTitlesByPaneId,
    setTerminalError,
    setTerminalErrorsByPaneId,
    settings,
    shouldMeasureHiddenStartup,
    sshReconnectOwnsTerminalErrors,
    systemPrefersDark,
    tabAgentTypeByLeaf,
    terminalError,
    terminalErrorsByPaneId,
    terminalTab,
    unifiedTabLabel
  } = controller
  const effectiveAppearance = settings
    ? resolveEffectiveTerminalAppearance(settings, systemPrefersDark)
    : null
  const terminalBackground =
    settings?.terminalColorOverrides?.background ?? effectiveAppearance?.theme?.background
  const titleUsesLightSurface = isTerminalBackgroundLight(terminalBackground, {
    appSurface: effectiveAppearance?.mode,
    backgroundOpacity: settings?.terminalBackgroundOpacity
  })
  const paneTitleBackground =
    resolveOpaqueTerminalBackground(terminalBackground, {
      appSurface: effectiveAppearance?.mode,
      backgroundOpacity: settings?.terminalBackgroundOpacity
    }) ?? (titleUsesLightSurface ? '#ffffff' : '#000000')
  const terminalContentVisible = isVisible || shouldMeasureHiddenStartup
  const hiddenStartupStyle: CSSProperties = shouldMeasureHiddenStartup
    ? { opacity: 0, pointerEvents: 'none' }
    : {}
  const terminalContainerStyle: CSSProperties = {
    display: terminalContentVisible ? 'flex' : 'none',
    overflow: 'hidden',
    ...hiddenStartupStyle,
    ['--orca-terminal-divider-color' as string]:
      effectiveAppearance?.dividerColor ?? DEFAULT_TERMINAL_DIVIDER_DARK,
    ['--orca-terminal-divider-color-strong' as string]: normalizeColor(
      effectiveAppearance?.dividerColor,
      DEFAULT_TERMINAL_DIVIDER_DARK
    )
  }
  const activePane = managerRef.current?.getActivePane()
  const managedPanes = managerRef.current?.getPanes() ?? []
  const showSshReconnectOverlay = isActive && isVisible && sshReconnectOwnsTerminalErrors
  // Why: SSH reconnect owns its failures even while this tab is hidden; clear only those lines so
  // unrelated pane errors survive and no stale connect failure flashes after recovery.
  useEffect(() => {
    if (!sshReconnectOwnsTerminalErrors) {
      return
    }
    setTerminalError((current) =>
      current === null ? null : stripSshReconnectOwnedErrorLines(current)
    )
    setTerminalErrorsByPaneId((current) =>
      mapPaneTerminalErrors(current, stripSshReconnectOwnedErrorLines)
    )
    // oxlint-disable-next-line react-hooks/exhaustive-deps -- Preserve the pre-split dependency contract.
  }, [sshReconnectOwnsTerminalErrors])
  const visibleTerminalError = terminalErrorForPane(
    terminalError,
    terminalErrorsByPaneId,
    activePane?.id ?? null
  )
  const menuPaneHasCustomTitle =
    contextMenu.menuPaneId !== null && Boolean(paneTitles[contextMenu.menuPaneId])
  const menuAgentSessionId = useAppStore((state) =>
    contextMenu.open && contextMenuLeafId
      ? resolvePaneAgentSessionId(state, makePaneKey(controller.tabId, contextMenuLeafId))
      : null
  )
  const resolveAgentForLeaf = useCallback(
    (leafId: string | null): string | null => {
      const detectedAgent = leafId ? (tabAgentTypeByLeaf[leafId] ?? null) : null
      if (detectedAgent) {
        return detectedAgent
      }
      const panes = managerRef.current?.getPanes() ?? []
      // Tab titles can lag pane focus in split layouts, so let the title evidence
      // describe a leaf only when the tab has exactly that one leaf.
      const hasSingleKnownLeaf =
        expectedLayoutLeafIds.length === 1 && expectedLayoutLeafIds[0] === leafId
      return resolvePaneTitleAgent({
        leafId,
        panes,
        runtimePaneTitlesByPaneId,
        tabLabel: hasSingleKnownLeaf ? unifiedTabLabel : null,
        terminalTitle: hasSingleKnownLeaf ? terminalTab?.title : null
      })
    },
    // oxlint-disable-next-line react-hooks/exhaustive-deps -- managerRef is a stable ref container.
    [
      expectedLayoutLeafIds,
      runtimePaneTitlesByPaneId,
      tabAgentTypeByLeaf,
      terminalTab?.title,
      unifiedTabLabel
    ]
  )
  const activePaneCanContinueInNewSession = canContinueAgentSessionInNewSession(
    resolveAgentForLeaf(activePane?.leafId ?? null)
  )
  const contextMenuCanContinueInNewSession = canContinueAgentSessionInNewSession(
    resolveAgentForLeaf(contextMenuLeafId)
  )
  return {
    effectiveAppearance,
    terminalBackground,
    titleUsesLightSurface,
    paneTitleBackground,
    terminalContentVisible,
    hiddenStartupStyle,
    terminalContainerStyle,
    activePane,
    managedPanes,
    showSshReconnectOverlay,
    visibleTerminalError,
    menuPaneHasCustomTitle,
    menuAgentSessionId,
    resolveAgentForLeaf,
    activePaneCanContinueInNewSession,
    contextMenuCanContinueInNewSession
  }
}

export type TerminalPaneProjectionController = TerminalPaneMobileController &
  ReturnType<typeof useTerminalPaneProjection>
