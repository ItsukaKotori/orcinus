import type { WorktreeMeta } from '../../../../shared/worktree/meta-types'

export type WorktreeMetaSavedPayload = {
  worktreeId: string
  updates: Partial<WorktreeMeta>
}

/** What the user currently has typed in the dialog. */
export type WorktreeMetaDraft = {
  displayNameInput: string
  commentInput: string
}

/** The persisted state the dialog was seeded from. Captured once when the
 *  dialog opens: comparing a frozen draft against a live store would let a
 *  background write move the baseline and make an untouched field "dirty". */
export type WorktreeMetaSnapshot = {
  displayName: string
  comment: string
}

// Why: blanking the field means "fall back to the branch/folder name", and the
// empty string is how that intent is persisted. Emitting `undefined` instead
// put a present-but-undefined key into the store spread, wiping the live name
// and crashing the worktree palette (crash a1f81ea1).
function buildDisplayNameUpdate(
  draft: WorktreeMetaDraft,
  current: WorktreeMetaSnapshot
): Partial<WorktreeMeta> {
  const trimmed = draft.displayNameInput.trim()
  return trimmed === current.displayName ? {} : { displayName: trimmed }
}

// Why: persistence bumps lastActivityAt whenever a `comment` key is present, so
// re-emitting an unchanged note reorders the workspace under the time-decay
// sidebar sort on a save that only touched the display name.
function buildCommentUpdate(
  draft: WorktreeMetaDraft,
  current: WorktreeMetaSnapshot
): Partial<WorktreeMeta> {
  const trimmed = draft.commentInput.trim()
  return trimmed === current.comment ? {} : { comment: trimmed }
}

/** Pure save-payload builder for the worktree meta dialog: unchanged inputs
 *  emit nothing. No key is ever emitted holding `undefined` — persistence
 *  raw-spreads updates, so a present-but-undefined key erases the stored value. */
export function buildWorktreeMetaUpdates(
  draft: WorktreeMetaDraft,
  current: WorktreeMetaSnapshot
): Partial<WorktreeMeta> {
  return {
    ...buildCommentUpdate(draft, current),
    ...buildDisplayNameUpdate(draft, current)
  }
}
