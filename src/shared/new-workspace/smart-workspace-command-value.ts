export type SmartWorkspaceCommandRowKind = 'use-name' | 'create-branch' | 'branch'

export type SmartWorkspaceCommandRow = {
  kind: SmartWorkspaceCommandRowKind
  value: string
}

export function resolveSmartWorkspaceCommandValue({
  currentValue,
  rows,
  isQueryStale
}: {
  currentValue: string
  rows: readonly SmartWorkspaceCommandRow[]
  isQueryStale: boolean
}): string {
  if (rows.length === 0) {
    return currentValue
  }

  // Why: freeze the arm while the live input is ahead of debounced search so the
  // highlight does not thrash to use-name / empty / first-row on every keystroke.
  if (isQueryStale) {
    const typedTextRow = rows.find((row) => row.kind === 'use-name' || row.kind === 'create-branch')
    if (typedTextRow) {
      return typedTextRow.value
    }
    return rows.some((row) => row.value === currentValue) ? currentValue : (rows[0]?.value ?? '')
  }

  return rows.some((row) => row.value === currentValue) ? currentValue : rows[0].value
}
