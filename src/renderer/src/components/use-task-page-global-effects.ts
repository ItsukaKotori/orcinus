import type { TaskPageGitHubIssueCreationModel } from './use-task-page-github-issue-creation'
import { useEffect } from 'react'
export function useTaskPageGlobalEffects(model: TaskPageGitHubIssueCreationModel) {
  const {
    closeTaskPage,
    activeModal,
    preflightStatusChecked,
    preflightStatusContextKey,
    refreshPreflightStatus,
    expectedPreflightContextKey,
    providerRuntimeContextKey,
    preflightStatusCurrent,
    tasksLoading,
    tasksRefreshing,
    tasksFiltering,
    dialogWorkItem,
    newIssueOpen
  } = model
  const githubTasksBusy = tasksLoading || tasksRefreshing || tasksFiltering
  useEffect(() => {
    // Why: when a modal is open, let it own Esc dismissal.
    if (dialogWorkItem || newIssueOpen || activeModal !== 'none') {
      return
    }
    const onKeyDown = (event: KeyboardEvent): void => {
      if (event.key !== 'Escape') {
        return
      }
      const target = event.target
      if (!(target instanceof HTMLElement)) {
        return
      }

      // Why: open menus/popovers/selects own Esc; capture-phase leave would steal it from Radix.
      if (
        document.querySelector(
          '[data-slot="dropdown-menu-content"], [data-slot="popover-content"], [data-slot="select-content"], [role="menu"]'
        )
      ) {
        return
      }

      // Why: Esc first blurs a focused input so it doesn't accidentally close the whole page; only closes once focus is outside an input.
      if (
        target instanceof HTMLInputElement ||
        target instanceof HTMLTextAreaElement ||
        target instanceof HTMLSelectElement ||
        target.isContentEditable
      ) {
        event.preventDefault()
        target.blur()
        return
      }
      event.preventDefault()
      closeTaskPage()
    }
    window.addEventListener('keydown', onKeyDown, {
      capture: true
    })
    return () =>
      window.removeEventListener('keydown', onKeyDown, {
        capture: true
      })
  }, [activeModal, closeTaskPage, dialogWorkItem, newIssueOpen])
  useEffect(() => {
    if (!preflightStatusCurrent || !preflightStatusChecked) {
      void refreshPreflightStatus()
    }
  }, [
    expectedPreflightContextKey,
    providerRuntimeContextKey,
    preflightStatusContextKey,
    preflightStatusChecked,
    preflightStatusCurrent,
    refreshPreflightStatus
  ])

  const nextModel = model as typeof model & {
    githubTasksBusy: typeof githubTasksBusy
  }
  nextModel.githubTasksBusy = githubTasksBusy
  return nextModel
}
export type TaskPageGlobalEffectsModel = ReturnType<typeof useTaskPageGlobalEffects>
