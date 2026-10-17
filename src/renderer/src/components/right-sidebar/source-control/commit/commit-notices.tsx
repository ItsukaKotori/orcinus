import React from 'react'
import {
  PullPolicyRemoteActionNotice,
  isPullPolicyRemoteActionError
} from './pull-policy-error-notice'
import { SourceControlRecoveryNotice } from './recovery-notice'
import type { CommitNoticesProps } from './commit-area-types'
import { translate } from '@/i18n/i18n'

export function CommitNotices({
  worktreeId,
  groupId,
  connectionId,
  repoId,
  launchPlatform,
  commitError,
  commitFailureSummary,
  commitFailureKindLabel,
  hasCommitFailureDetails,
  commitFailureRecoveryPrompt,
  pushRecovery,
  remoteActionError,
  generateError,
  sourceControlAiActionsVisible,
  isFixingCommitFailureWithAI,
  isFixingPushFailureWithAI,
  fixCommitFailureRecipe,
  fixPushFailureRecipe,
  onSaveLaunchActionDefault,
  onOpenSourceControlAiSettings,
  onFixCommitFailureWithAI,
  onFixPushFailureWithAI
}: CommitNoticesProps): React.JSX.Element {
  return (
    <>
      {commitError && commitFailureSummary ? (
        <SourceControlRecoveryNotice
          id="commit-area-error"
          recoveryKind="commit"
          title={translate(
            'auto.components.right.sidebar.SourceControl.011f9713fc',
            'Commit blocked'
          )}
          detailsTitle={translate(
            'auto.components.right.sidebar.SourceControl.a9bf7c171a',
            'Commit Failed'
          )}
          summary={commitFailureSummary}
          detailText={commitError}
          hasDetails={hasCommitFailureDetails}
          kindLabel={commitFailureKindLabel}
          prompt={commitFailureRecoveryPrompt}
          worktreeId={worktreeId}
          groupId={groupId}
          connectionId={connectionId}
          repoId={repoId}
          launchPlatform={launchPlatform}
          sourceControlAiActionsVisible={sourceControlAiActionsVisible}
          isLaunching={isFixingCommitFailureWithAI}
          recipe={fixCommitFailureRecipe}
          onSaveLaunchActionDefault={onSaveLaunchActionDefault}
          onOpenSourceControlAiSettings={onOpenSourceControlAiSettings}
          onFixWithAI={onFixCommitFailureWithAI}
        />
      ) : null}
      {pushRecovery ? (
        <SourceControlRecoveryNotice
          id="commit-area-push-error"
          recoveryKind="push"
          title={translate(
            'auto.components.right.sidebar.SourceControl.pushRecovery.011f9713fc',
            'Push blocked'
          )}
          detailsTitle={translate(
            'auto.components.right.sidebar.SourceControl.pushRecovery.a9bf7c171a',
            'Push Failed'
          )}
          summary={pushRecovery.summary}
          detailText={pushRecovery.detailText}
          hasDetails={pushRecovery.hasDetails}
          kindLabel={pushRecovery.kindLabel}
          prompt={pushRecovery.prompt}
          worktreeId={worktreeId}
          groupId={groupId}
          connectionId={connectionId}
          repoId={repoId}
          launchPlatform={launchPlatform}
          sourceControlAiActionsVisible={sourceControlAiActionsVisible}
          isLaunching={isFixingPushFailureWithAI}
          recipe={fixPushFailureRecipe}
          onSaveLaunchActionDefault={onSaveLaunchActionDefault}
          onOpenSourceControlAiSettings={onOpenSourceControlAiSettings}
          onFixWithAI={onFixPushFailureWithAI}
        />
      ) : null}
      {remoteActionError && isPullPolicyRemoteActionError(remoteActionError) ? (
        <PullPolicyRemoteActionNotice id="commit-area-remote-error" />
      ) : remoteActionError ? (
        <p
          id="commit-area-remote-error"
          role="alert"
          aria-live="polite"
          className="mt-1 text-[11px] text-destructive"
        >
          {remoteActionError}
        </p>
      ) : null}
      {generateError && (
        <p
          id="commit-area-generate-error"
          role="alert"
          aria-live="polite"
          className="mt-1 text-[11px] text-destructive"
        >
          {generateError}
        </p>
      )}
    </>
  )
}
