import { buildPaletteDocument, type PaletteDocument } from './palette-match/palette-document'
import type { PaletteComposedEvidence } from './palette-match/evidence-composer'
import { buildWorktreeCommentEvidence } from './worktree-palette-evidence'
import {
  resolveWorktreeBranchLabel,
  resolveWorktreeDisplayName
} from './worktree-default-display-name'
import type { Repo } from '../../../shared/repo-types'
import type { Worktree } from '../../../shared/worktree/types'
import {
  getPaletteWorktreeIdentity,
  resolvePaletteRepoForWorktree
} from './palette-repo-resolution'

export const WORKTREE_PALETTE_NAME_FIELD_ID = 'name'
export const WORKTREE_PALETTE_BRANCH_FIELD_ID = 'branch'
export const WORKTREE_PALETTE_REPO_FIELD_ID = 'repo'
export const WORKTREE_PALETTE_HOST_FIELD_ID = 'host'

export type WorktreePaletteDocumentSources = {
  repoMap: ReadonlyMap<string, Repo>
  repoMapByHostIdentity?: ReadonlyMap<string, Repo>
  hostLabelByWorktreeId?: ReadonlyMap<string, string>
}

function buildEvidence(worktree: Worktree): PaletteComposedEvidence[] {
  const comment = buildWorktreeCommentEvidence(worktree.comment ?? '')
  return [comment].filter((unit): unit is PaletteComposedEvidence => unit !== null)
}

export function buildWorktreePaletteDocument(
  worktree: Worktree,
  sources: WorktreePaletteDocumentSources
): PaletteDocument {
  const repo = resolvePaletteRepoForWorktree(
    worktree,
    sources.repoMap,
    sources.repoMapByHostIdentity
  )
  return buildPaletteDocument({
    id: worktree.id,
    visibleFields: [
      {
        id: WORKTREE_PALETTE_NAME_FIELD_ID,
        profile: 'structured-label',
        text: resolveWorktreeDisplayName(worktree),
        role: 'primary',
        destinationEligible: true
      },
      {
        id: WORKTREE_PALETTE_BRANCH_FIELD_ID,
        profile: 'structured-label',
        text: resolveWorktreeBranchLabel(worktree),
        role: 'secondary',
        destinationEligible: true
      },
      {
        id: WORKTREE_PALETTE_REPO_FIELD_ID,
        profile: 'structured-label',
        text: repo?.displayName ?? '',
        role: 'secondary',
        destinationEligible: false
      },
      {
        id: WORKTREE_PALETTE_HOST_FIELD_ID,
        profile: 'structured-label',
        // Why conditional: the host chip only renders for active remote hosts, and
        // an unrendered match would be unexplainable on the row.
        // Why both keys: the palette keys this map by host identity so two same-id
        // workspaces keep distinct chips, but a bare-id map is still a valid input.
        text:
          sources.hostLabelByWorktreeId?.get(getPaletteWorktreeIdentity(worktree)) ??
          sources.hostLabelByWorktreeId?.get(worktree.id) ??
          '',
        role: 'secondary',
        destinationEligible: false
      }
    ],
    compositePairs: [
      {
        leftFieldId: WORKTREE_PALETTE_REPO_FIELD_ID,
        rightFieldId: WORKTREE_PALETTE_BRANCH_FIELD_ID
      },
      { leftFieldId: WORKTREE_PALETTE_REPO_FIELD_ID, rightFieldId: WORKTREE_PALETTE_NAME_FIELD_ID }
    ],
    evidence: buildEvidence(worktree)
  })
}

export function buildWorktreePaletteDocuments(
  worktrees: readonly Worktree[],
  sources: WorktreePaletteDocumentSources
): Map<string, PaletteDocument> {
  const documents = new Map<string, PaletteDocument>()
  for (const worktree of worktrees) {
    // Why the host identity (STA-4343): `repoId::path` repeats across hosts, so keying on
    // the bare id lets the second host overwrite the first and one workspace becomes
    // unsearchable by its own name.
    documents.set(
      getPaletteWorktreeIdentity(worktree),
      buildWorktreePaletteDocument(worktree, sources)
    )
  }
  return documents
}
