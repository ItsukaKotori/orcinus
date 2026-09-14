import { useCallback, useEffect, useRef, useState } from 'react'
import { useShortcutLabel } from '@/hooks/useShortcutLabel'
import { useAppStore } from '../../store'
import { selectFloatingWorkspaceHasUnread } from '../../store/selectors'
import { CLOSE_ALL_CONTEXT_MENUS_EVENT } from './ProviderDetailsMenu'
import { observeStatusBarContainer } from './status-bar-container-observer'

export function useStatusBarController(floatingTerminalOpen: boolean) {
  const floatingTerminalShortcut = useShortcutLabel('floatingTerminal.toggle')
  const settings = useAppStore((s) => s.settings)
  const statusBarVisible = useAppStore((s) => s.statusBarVisible)
  const statusBarItems = useAppStore((s) => s.statusBarItems)
  const recordFeatureInteraction = useAppStore((s) => s.recordFeatureInteraction)
  // Why: reuse the floating-button's unread dot so activity shows for either trigger location (see FloatingTerminalToggleButton).
  const hasFloatingUnread = useAppStore(selectFloatingWorkspaceHasUnread)
  const floatingTerminalEnabled = settings?.floatingTerminalEnabled === true
  const floatingTerminalTriggerLocation =
    settings?.floatingTerminalTriggerLocation ?? 'floating-button'
  // Why: pet segment is driven purely by experimentalPet, not statusBarItems, to avoid double-toggling the surface (see design doc).
  const petEnabled = useAppStore((s) => s.settings?.experimentalPet === true)
  const toggleStatusBarItem = useAppStore((s) => s.toggleStatusBarItem)
  const containerRef = useRef<HTMLDivElement>(null)
  const [menuOpen, setMenuOpen] = useState(false)
  const [menuPoint, setMenuPoint] = useState({ x: 0, y: 0 })

  const [containerWidth, setContainerWidth] = useState(900)
  const resizeObserverRef = useRef<ResizeObserver | null>(null)

  useEffect(() => {
    const closeMenu = (): void => setMenuOpen(false)
    window.addEventListener(CLOSE_ALL_CONTEXT_MENUS_EVENT, closeMenu)
    return () => window.removeEventListener(CLOSE_ALL_CONTEXT_MENUS_EVENT, closeMenu)
  }, [])

  const containerRefCallback = useCallback((node: HTMLDivElement | null) => {
    if (resizeObserverRef.current) {
      resizeObserverRef.current.disconnect()
      resizeObserverRef.current = null
    }
    if (node) {
      containerRef.current = node
      resizeObserverRef.current = observeStatusBarContainer(node, setContainerWidth)
      setContainerWidth(node.getBoundingClientRect().width)
    }
  }, [])

  if (!statusBarVisible) {
    return null
  }

  const showResourceUsage = statusBarItems.includes('resource-usage')
  const showPorts = statusBarItems.includes('ports')
  const showFloatingTerminalToggle =
    floatingTerminalEnabled && floatingTerminalTriggerLocation === 'status-bar'

  const compact = containerWidth < 900
  const iconOnly = containerWidth < 500
  const floatingTerminalActionLabel = floatingTerminalOpen
    ? 'Minimize Floating Workspace'
    : 'Show Floating Workspace'
  const showFloatingWorkspaceAttentionDot = !floatingTerminalOpen && hasFloatingUnread

  return {
    compact,
    containerRefCallback,
    floatingTerminalActionLabel,
    floatingTerminalShortcut,
    iconOnly,
    menuOpen,
    menuPoint,
    petEnabled,
    recordFeatureInteraction,
    setMenuOpen,
    setMenuPoint,
    showFloatingTerminalToggle,
    showFloatingWorkspaceAttentionDot,
    showPorts,
    showResourceUsage,
    statusBarItems,
    toggleStatusBarItem
  }
}

export type StatusBarController = NonNullable<ReturnType<typeof useStatusBarController>>
