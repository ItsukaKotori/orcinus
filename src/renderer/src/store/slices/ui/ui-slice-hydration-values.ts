import type { PersistedUIState } from '../../../../../shared/persisted-ui-state-types'
import type { FeatureInteractionState } from '../../../../../shared/feature-interactions'
import { normalizeFeatureInteractions } from '../../../../../shared/feature-interactions'
import type { UISlice } from './ui-slice-contract'
import {
  sanitizeAcknowledgedAgentsByPaneKey,
  sanitizeActivityClearedAtByPaneKey,
  sanitizePaneKeyTimestampRecord
} from './ui-slice-hydration-sanitizers'

export function mergeFeatureInteractionState(
  current: FeatureInteractionState,
  incoming: PersistedUIState['featureInteractions']
): FeatureInteractionState {
  const currentNormalized = normalizeFeatureInteractions(current)
  const incomingNormalized = normalizeFeatureInteractions(incoming)
  const merged: FeatureInteractionState = { ...currentNormalized }
  for (const [id, incomingRecord] of Object.entries(incomingNormalized)) {
    const currentRecord = currentNormalized[id as keyof FeatureInteractionState]
    merged[id as keyof FeatureInteractionState] = currentRecord
      ? {
          firstInteractedAt: Math.min(
            currentRecord.firstInteractedAt,
            incomingRecord.firstInteractedAt
          ),
          interactionCount: Math.max(
            currentRecord.interactionCount,
            incomingRecord.interactionCount
          )
        }
      : incomingRecord
  }
  return merged
}

/** Stale acks/marks are inert (paneKey reuse beats them via stateStartedAt); the sanitizers only bound growth past HYDRATE_MAX_AGE_MS. */
export function hydrateAgentReadState(
  ui: PersistedUIState
): Pick<
  UISlice,
  'acknowledgedAgentsByPaneKey' | 'activityClearedAtByPaneKey' | 'manuallyUnreadTurnsByPaneKey'
> {
  return {
    acknowledgedAgentsByPaneKey: sanitizeAcknowledgedAgentsByPaneKey(
      ui.acknowledgedAgentsByPaneKey
    ),
    activityClearedAtByPaneKey: sanitizeActivityClearedAtByPaneKey(ui.activityClearedAtByPaneKey),
    manuallyUnreadTurnsByPaneKey: sanitizePaneKeyTimestampRecord(ui.manuallyUnreadTurnsByPaneKey)
  }
}
