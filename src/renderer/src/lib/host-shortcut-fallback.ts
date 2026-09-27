import { useAppStore } from '@/store'

/**
 * Host menu accelerators (Electron) used to deliver these shortcuts as `ui:*`
 * events. A has no menu layer yet, so the renderer owns the default accelerators
 * for the workspace-level shortcuts until the Tauri menu lands (Phase 4).
 */
function isMacPlatform(): boolean {
  return navigator.userAgent.includes('Mac')
}

export function registerHostShortcutFallback(): () => void {
  // Why: bridge harnesses install a minimal window stub; only the real renderer
  // owns addEventListener. Registration must stay a no-op there.
  if (typeof window === 'undefined' || typeof window.addEventListener !== 'function') {
    return () => {}
  }
  const onKeyDown = (event: KeyboardEvent): void => {
    if (event.defaultPrevented || event.repeat || event.altKey || event.shiftKey) {
      return
    }
    const primary = isMacPlatform() ? event.metaKey : event.ctrlKey
    if (!primary) {
      return
    }
    const store = useAppStore.getState()
    switch (event.key.toLowerCase()) {
      case 'p': {
        if (store.activeView === 'terminal' && store.activeWorktreeId !== null) {
          event.preventDefault()
          store.openModal('quick-open')
        }
        return
      }
      case 'j': {
        event.preventDefault()
        if (store.activeModal === 'worktree-palette') {
          store.closeModal()
        } else {
          store.openModal('worktree-palette')
        }
        return
      }
      case ',': {
        event.preventDefault()
        store.openSettingsPage()
        return
      }
      default:
        return
    }
  }
  window.addEventListener('keydown', onKeyDown)
  return () => window.removeEventListener('keydown', onKeyDown)
}
