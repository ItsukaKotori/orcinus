import type { GitBranchChangeEntry } from '../../../../shared/git-diff-compare-types'
import type { GitHistoryItem } from '../../../../shared/git-history-types'
import { formatAbsoluteDate, shortHash } from './format'

type Props = {
  item: GitHistoryItem | null
  files: GitBranchChangeEntry[]
  onOpenFile?: (item: GitHistoryItem, entry: GitBranchChangeEntry) => void
}

export function CommitDetail({ item, files, onOpenFile }: Props): React.JSX.Element {
  return (
    <div data-testid="commit-detail" className="h-48 shrink-0 overflow-auto border-t border-border p-3 text-xs">
      {!item ? (
        <div className="text-muted-foreground">Select a commit.</div>
      ) : (
        <div className="space-y-2">
          <div className="font-mono text-muted-foreground">{shortHash(item.displayId ?? item.id)}</div>
          <div className="font-medium text-foreground">{item.subject}</div>
          <div className="text-muted-foreground">{item.author} &lt;{item.authorEmail}&gt; · {formatAbsoluteDate(item.timestamp)}</div>
          {item.message && item.message !== item.subject ? (
            <pre className="whitespace-pre-wrap font-sans text-foreground">{item.message}</pre>
          ) : null}
          <div className="text-muted-foreground">{item.statistics ? `+${item.statistics.insertions} −${item.statistics.deletions} in ${item.statistics.files} files` : ''}</div>
          <ul className="mt-1 space-y-0.5">
            {files.map((f) => (
              <li key={f.path} className="flex items-center gap-2">
                <span className="w-4 text-center text-muted-foreground">{f.status[0]!.toUpperCase()}</span>
                {onOpenFile && item ? (
                  <button
                    type="button"
                    className="truncate text-left hover:underline"
                    onClick={() => onOpenFile(item, f)}
                    title={`Open diff: ${f.path}`}
                  >
                    {f.path}
                  </button>
                ) : (
                  <span className="truncate">{f.path}</span>
                )}
              </li>
            ))}
          </ul>
        </div>
      )}
    </div>
  )
}
