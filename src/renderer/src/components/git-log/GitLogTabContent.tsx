import type { Tab } from '../../../../shared/tab-types'

/** Placeholder wired in T4; T5 renders the native GitLogView here. */
export function GitLogTabContent({ tab }: { tab: Tab }): React.JSX.Element {
  return (
    <div
      data-testid="git-log-tab"
      className="flex h-full w-full items-center justify-center text-sm text-muted-foreground"
    >
      Git Log — {tab.gitLog?.worktreeId ?? ''}
    </div>
  )
}

export default GitLogTabContent
