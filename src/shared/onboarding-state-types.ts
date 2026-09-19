export type OnboardingOutcome = 'completed' | 'dismissed'

export type OnboardingChecklistState = {
  addedRepo: boolean
  choseAgent: boolean
  ranFirstAgent: boolean
  ranSecondAgentOnSameTask: boolean
  triedCmdJ: boolean
  shapedSidebar: boolean
  reviewedDiff: boolean
  openedPr: boolean
  addedFolder: boolean
  openedFile: boolean
  ranAgentOnFile: boolean
  // Why: UI state flag (panel visibility), not a task the user performed.
  dismissed: boolean
}

export type OnboardingState = {
  // Why: step meanings change when pages are removed; version marker prevents migration re-running on new progress.
  flowVersion: number
  closedAt: number | null
  outcome: OnboardingOutcome | null
  // Sentinel -1 = not started; 1..5 = highest finished wizard step. number (not union) because callers clamp via Math.max/min.
  lastCompletedStep: number
  checklist: OnboardingChecklistState
}
