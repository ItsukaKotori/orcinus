import type { PersistedUIState } from '../../../../../shared/persisted-ui-state-types'
import type { TaskResumeState, TaskViewPresetId } from '../../../../../shared/ui-chrome-types'
import type { FeatureInteractionState } from '../../../../../shared/feature-interactions'
import { normalizeFeatureInteractions } from '../../../../../shared/feature-interactions'
import type { UISlice } from './ui-slice-contract'
import {
  sanitizeAcknowledgedAgentsByPaneKey,
  sanitizeActivityClearedAtByPaneKey,
  sanitizePaneKeyTimestampRecord
} from './ui-slice-hydration-sanitizers'

const VALID_TASK_PRESETS = new Set<TaskViewPresetId>([
  'all',
  'issues',
  'review',
  'my-issues',
  'my-prs',
  'prs'
])
export function sanitizeTaskResumeState(value: unknown): TaskResumeState | undefined {
  if (!value || typeof value !== 'object') {
    return undefined
  }
  const input = value as Record<string, unknown>
  const next: TaskResumeState = {}
  if (input.githubMode === 'items' || input.githubMode === 'project') {
    next.githubMode = input.githubMode
  }
  if (input.githubItemsPreset === null) {
    next.githubItemsPreset = null
  } else if (
    typeof input.githubItemsPreset === 'string' &&
    VALID_TASK_PRESETS.has(input.githubItemsPreset as TaskViewPresetId)
  ) {
    next.githubItemsPreset = input.githubItemsPreset as TaskViewPresetId
  }
  if (typeof input.githubItemsQuery === 'string') {
    next.githubItemsQuery = input.githubItemsQuery
  }
  return Object.keys(next).length > 0 ? next : undefined
}

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
