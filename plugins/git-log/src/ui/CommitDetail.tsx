import type { GitBranchChangeEntry } from '@shared/git-diff-compare-types'
import type { GitHistoryItem } from '@shared/git-history-types'
import { shortHash } from './format'

type Props = { item: GitHistoryItem | null; files: GitBranchChangeEntry[] }

export function CommitDetail({ item, files }: Props): React.JSX.Element {
  return (
    <div data-testid="commit-detail" className="h-48 shrink-0 overflow-auto border-t border-border p-3 text-xs">
      {!item ? (
        <div className="text-muted-foreground">Select a commit.</div>
      ) : (
        <div className="space-y-2">
          <div className="font-mono text-muted-foreground">{shortHash(item.id)}</div>
          <div className="font-medium text-foreground">{item.subject}</div>
          <div className="text-muted-foreground">{item.author} &lt;{item.authorEmail}&gt;</div>
          <div className="text-muted-foreground">{item.statistics ? `+${item.statistics.insertions} −${item.statistics.deletions} in ${item.statistics.files} files` : ''}</div>
          <ul className="mt-1 space-y-0.5">
            {files.map((f) => (
              <li key={f.path} className="flex items-center gap-2">
                <span className="w-4 text-center text-muted-foreground">{f.status[0]!.toUpperCase()}</span>
                <span className="truncate">{f.path}</span>
              </li>
            ))}
          </ul>
        </div>
      )}
    </div>
  )
}
