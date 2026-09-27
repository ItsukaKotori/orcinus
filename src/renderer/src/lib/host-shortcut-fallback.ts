import { useAppStore } from '@/store'

/**
 * Host menu accelerators (Electron) used to deliver these shortcuts as `ui:*`
 * events. A has no menu layer yet, so the renderer owns the default accelerators
 * for the workspace-level shortcuts until the Tauri menu lands (Phase 4).
 *
 * Matching prefers `event.code` because `event.key` degrades to `Process` while
 * a CJK input source is active on macOS.
 */
type ShortcutAction = 'quick-open' | 'worktree-palette' | 'settings'

function shortcutForEvent(event: KeyboardEvent): ShortcutAction | null {
  switch (event.code) {
    case 'KeyP':
      return 'quick-open'
    case 'KeyJ':
      return 'worktree-palette'
    case 'Comma':
      return 'settings'
    default:
      break
  }
  switch (event.key.toLowerCase()) {
    case 'p':
      return 'quick-open'
    case 'j':
      return 'worktree-palette'
    case ',':
      return 'settings'
    default:
      return null
  }
}

function isMacPlatform(): boolean {
  return navigator.userAgent.includes('Mac')
}

function runShortcut(action: ShortcutAction): boolean {
  const store = useAppStore.getState()
  switch (action) {
    case 'quick-open': {
      if (store.activeView !== 'terminal' || store.activeWorktreeId === null) {
        return false
      }
      store.openModal('quick-open')
      return true
    }
    case 'worktree-palette': {
      if (store.activeModal === 'worktree-palette') {
        store.closeModal()
      } else {
        store.openModal('worktree-palette')
      }
      return true
    }
    case 'settings': {
      store.openSettingsPage()
      return true
    }
  }
}

export function registerHostShortcutFallback(): () => void {
  // Why: bridge harnesses install a minimal window stub; only the real renderer
  // owns addEventListener. Registration must stay a no-op there.
  if (typeof window === 'undefined' || typeof window.addEventListener !== 'function') {
    return () => {}
  }
  const onKeyDown = (event: KeyboardEvent): void => {
    if (event.repeat || event.altKey || event.shiftKey) {
      return
    }
    const primary = isMacPlatform() ? event.metaKey : event.ctrlKey
    if (!primary) {
      return
    }
    const action = shortcutForEvent(event)
    if (!action) {
      return
    }
    if (!runShortcut(action)) {
      return
    }
    event.preventDefault()
    if (import.meta.env.DEV) {
      console.info(`[ade] host shortcut fallback: ${action}`)
    }
  }
  window.addEventListener('keydown', onKeyDown)
  if (import.meta.env.DEV) {
    console.info('[ade] host shortcut fallback registered')
  }
  return () => window.removeEventListener('keydown', onKeyDown)
}
