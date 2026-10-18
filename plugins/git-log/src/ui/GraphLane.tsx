import type { GraphRow } from '../graph/git-graph-model'
import { graphColorVar } from '../graph/git-graph-model'

const CELL = 14
export function GraphLane({ row }: { row: GraphRow }): React.JSX.Element {
  const { viewModel } = row
  const x = (i: number) => i * CELL + CELL / 2
  const cy = CELL / 2
  const lines: React.JSX.Element[] = []
  const selfX = x(row.laneIndex)

  viewModel.inputSwimlanes.forEach((node, i) => {
    lines.push(<line key={`in-${node.id}-${i}`} x1={x(i)} y1={0} x2={selfX} y2={cy} stroke={graphColorVar(node.color)} strokeWidth={1.5} />)
  })
  viewModel.outputSwimlanes.forEach((node, i) => {
    const isSelf = node.id === viewModel.historyItem.parentIds[0]
    lines.push(<line key={`out-${node.id}-${i}`} x1={selfX} y1={cy} x2={x(i)} y2={CELL} stroke={graphColorVar(node.color)} strokeWidth={isSelf ? 1.8 : 1.2} />)
  })
  return (
    <svg width={(viewModel.inputSwimlanes.length + 1) * CELL} height={CELL} className="shrink-0">
      {lines}
      <circle cx={selfX} cy={cy} r={3.5} fill={viewModel.kind === 'HEAD' ? graphColorVar('git-graph-ref') : 'var(--muted-foreground)'} stroke="var(--background)" strokeWidth={1} />
    </svg>
  )
}
