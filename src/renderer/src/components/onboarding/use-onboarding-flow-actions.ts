import { useCallback, useRef, type Dispatch, type SetStateAction } from 'react'
import { toast } from 'sonner'
import { applyDocumentTheme } from '@/lib/document-theme'

import { translate } from '@/i18n/i18n'
import { ONBOARDING_FINAL_STEP } from '../../../../shared/constants'
import type { GlobalSettings } from '../../../../shared/global-settings-types'
import type { OnboardingState } from '../../../../shared/onboarding-state-types'
import type { TuiAgent } from '../../../../shared/tui-agent'

import { persistStep, type PersistCurrentStepResult } from './use-onboarding-flow-persistence'
import { STEPS, type StepNumber } from './use-onboarding-flow-types'
import {
  prepareSkippedOnboardingPreferences,
  resolveStepIndex,
  type OnboardingStepSkipOptions
} from './onboarding-flow-state'

type CloseWith = (
  outcome: 'completed' | 'dismissed',
  lastStepReached: StepNumber,
  completedPath?: 'add_project_modal'
) => Promise<boolean>

type OnboardingFlowActionsArgs = {
  busyLabel: string | null
  setBusyLabel: Dispatch<SetStateAction<string | null>>
  setError: Dispatch<SetStateAction<string | null>>
  currentStep: (typeof STEPS)[number]
  settings: GlobalSettings | null
  persistCurrentStep: () => Promise<PersistCurrentStepResult>
  closeWith: CloseWith
  openModal: (modal: 'add-repo') => void
  getNextStepIndex: (index: number) => number
  onOnboardingChange: (state: OnboardingState) => void
  stepIndex: number
  setStepIndex: Dispatch<SetStateAction<number>>
  selectedAgent: TuiAgent | null
  themeStepEntryThemeRef: { current: GlobalSettings['theme'] | null }
  setTheme: Dispatch<SetStateAction<GlobalSettings['theme']>>
  updateSettings: (updates: Partial<GlobalSettings>) => Promise<void> | void
  skipOptions: OnboardingStepSkipOptions
}

export function useOnboardingFlowActions({
  busyLabel,
  setBusyLabel,
  setError,
  currentStep,
  settings,
  persistCurrentStep,
  closeWith,
  openModal,
  getNextStepIndex,
  onOnboardingChange,
  stepIndex,
  setStepIndex,
  selectedAgent,
  themeStepEntryThemeRef,
  setTheme,
  updateSettings,
  skipOptions
}: OnboardingFlowActionsArgs) {
  // Why: sync latch; busyLabel state commits too late to stop a ~30ms Cmd+Enter auto-repeat from re-entering next() and skipping a step.
  const nextInFlightRef = useRef(false)
  const next = useCallback(
    async (_advancedVia: 'button' | 'keyboard' = 'button') => {
      if (nextInFlightRef.current || busyLabel) {
        return
      }
      nextInFlightRef.current = true
      try {
        const result = await persistCurrentStep()
        if (result.ok) {
          if (currentStep.id === 'notifications') {
            setBusyLabel(
              translate(
                'components.onboarding.flow.actions.openingAddProject',
                'Opening Add Project...'
              )
            )
            const closed = await closeWith('completed', ONBOARDING_FINAL_STEP, 'add_project_modal')
            if (closed) {
              openModal('add-repo')
            }
            return
          }
          const nextIndex = getNextStepIndex(stepIndex)
          const skippedThroughStepNumber = STEPS[nextIndex].stepNumber - 1
          if (skippedThroughStepNumber > currentStep.stepNumber) {
            // Why: skipped optional pages must still persist progress at the next visible page.
            try {
              onOnboardingChange(await persistStep(skippedThroughStepNumber))
            } catch (err) {
              toast.error(
                translate(
                  'auto.components.onboarding.use.onboarding.flow.52acfbef51',
                  'Could not save progress'
                ),
                {
                  description: err instanceof Error ? err.message : String(err)
                }
              )
            }
          }
          setStepIndex(nextIndex)
        }
      } finally {
        setBusyLabel(null)
        nextInFlightRef.current = false
      }
    },
    [
      busyLabel,
      closeWith,
      currentStep.id,
      currentStep.stepNumber,
      getNextStepIndex,
      onOnboardingChange,
      openModal,
      persistCurrentStep,
      stepIndex,
      setBusyLabel,
      setStepIndex
    ]
  )

  const skipToRepo = useCallback(async () => {
    if (busyLabel) {
      return
    }
    setError(null)
    if (currentStep.id === 'notifications') {
      return
    }
    const preferencesSaved = await prepareSkippedOnboardingPreferences({
      currentStepId: currentStep.id,
      themeBeforePreview: themeStepEntryThemeRef.current,
      settingsTheme: settings?.theme,
      selectedAgent,
      setTheme,
      applyTheme: applyDocumentTheme,
      updateSettings,
      setError
    })
    if (!preferencesSaved) {
      return
    }
    setBusyLabel(
      translate('components.onboarding.flow.actions.openingAddProject', 'Opening Add Project...')
    )
    try {
      const closed = await closeWith('completed', ONBOARDING_FINAL_STEP, 'add_project_modal')
      if (!closed) {
        return
      }
      // Why: repo picker now lives in the Add Project dialog, so skipping optional setup closes onboarding and hands off to it.
      openModal('add-repo')
    } finally {
      setBusyLabel(null)
    }
  }, [
    busyLabel,
    closeWith,
    currentStep.id,
    openModal,
    selectedAgent,
    settings,
    updateSettings,
    setBusyLabel,
    setError,
    setTheme,
    themeStepEntryThemeRef
  ])

  const dismissOnboarding = useCallback(
    async (_advancedVia: 'button' | 'keyboard' = 'button'): Promise<boolean> => {
      if (busyLabel) {
        return false
      }
      setError(null)
      return closeWith('dismissed', currentStep.stepNumber)
    },
    [busyLabel, closeWith, currentStep.stepNumber, setError]
  )

  const back = useCallback(() => {
    setStepIndex((index) => resolveStepIndex(index - 1, skipOptions, 'backward'))
  }, [setStepIndex, skipOptions])

  const jumpToStep = useCallback(
    (idx: number) => {
      setStepIndex(resolveStepIndex(idx, skipOptions, idx < stepIndex ? 'backward' : 'forward'))
    },
    [setStepIndex, skipOptions, stepIndex]
  )

  return { next, skipToRepo, dismissOnboarding, back, jumpToStep }
}
