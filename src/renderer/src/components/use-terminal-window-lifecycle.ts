import { useEffect } from 'react'
import { useAppStore } from '../store'
import { isIntentionalAppRestartInProgress } from '@/lib/updater-beforeunload'
import { preventUnloadAndScheduleShutdownCheckpointReset } from '@/lib/shutdown-checkpoint-guard'
import { setWindowCloseRequestHandler } from './window-close-request-coordinator'
import type { TerminalActivationController } from './use-terminal-activation-actions'

export function useTerminalWindowLifecycle(controller: TerminalActivationController): void {
  const { proceedToNativeWindowClose, queueEditorCloseRequests, windowCloseAfterDirtyRef } =
    controller
  useEffect(() => {
    const handler = (event: BeforeUnloadEvent): void => {
      if (isIntentionalAppRestartInProgress()) {
        return
      }
      const dirtyFiles = useAppStore.getState().openFiles.filter((file) => file.isDirty)
      if (dirtyFiles.length > 0) {
        preventUnloadAndScheduleShutdownCheckpointReset(event, window)
      }
    }
    window.addEventListener('beforeunload', handler)
    return () => window.removeEventListener('beforeunload', handler)
  }, [])

  useEffect(() => {
    setWindowCloseRequestHandler(({ isQuitting }) => {
      if (isIntentionalAppRestartInProgress()) {
        window.api.ui.confirmWindowClose()
        return
      }
      if (windowCloseAfterDirtyRef.current) {
        return
      }
      const dirtyFiles = useAppStore.getState().openFiles.filter((file) => file.isDirty)
      if (dirtyFiles.length > 0) {
        queueEditorCloseRequests(
          dirtyFiles.map((file) => file.id),
          { isQuitting }
        )
        return
      }
      proceedToNativeWindowClose(isQuitting)
    })
    return () => setWindowCloseRequestHandler(null)
    // oxlint-disable-next-line react-hooks/exhaustive-deps -- controller refs preserve their original stable identities.
  }, [proceedToNativeWindowClose, queueEditorCloseRequests])
}
