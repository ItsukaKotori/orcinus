import type {
  SourceControlPrimaryActionDecision,
  SourceControlPrimaryActionDecisionInputs
} from './source-control-primary-action-decision-types'

export function resolveUnpublishedSourceControlPrimaryAction({
  hasCurrentBranch
}: Pick<SourceControlPrimaryActionDecisionInputs, 'hasCurrentBranch'>): SourceControlPrimaryActionDecision {
  if (!hasCurrentBranch) {
    return {
      kind: 'commit',
      labelIntent: 'commit',
      titleIntent: 'checkout_branch_before_publish',
      disabled: true
    }
  }

  return {
    kind: 'publish',
    labelIntent: 'publish',
    titleIntent: 'publish_branch',
    disabled: false
  }
}
