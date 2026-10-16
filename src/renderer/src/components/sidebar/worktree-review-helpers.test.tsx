import { renderToStaticMarkup } from 'react-dom/server'
import { describe, expect, it } from 'vitest'
import { ReviewIcon } from './worktree-review-helpers'
import type { WorktreeCardPrDisplay } from './worktree-card-pr-display'

const githubReview: WorktreeCardPrDisplay = {
  provider: 'github',
  number: 456,
  title: 'Review me',
  state: 'open',
  status: 'pending'
}

describe('ReviewIcon', () => {
  // Why: #13088 — a draft with failing checks used the same glyph as every other
  // review and was painted red, so it was indistinguishable from a closed PR.
  it('gives a draft its own glyph and never paints it with a check tone', () => {
    const draft = renderToStaticMarkup(
      <ReviewIcon
        review={{
          provider: 'github',
          number: 1,
          title: 'Draft',
          state: 'draft',
          status: 'failure'
        }}
        className="size-3"
      />
    )
    const closed = renderToStaticMarkup(
      <ReviewIcon
        review={{ provider: 'github', number: 1, title: 'Closed', state: 'closed' }}
        className="size-3"
      />
    )

    expect(draft).toContain('lucide-git-pull-request-draft')
    expect(draft).not.toContain('text-rose-500/85')
    expect(closed).toContain('lucide-git-pull-request-closed')
    expect(draft).not.toBe(closed)
  })

  it('keeps check tones on open reviews so failing checks still stand out', () => {
    const failing = renderToStaticMarkup(
      <ReviewIcon
        review={{ provider: 'github', number: 1, title: 'Open', state: 'open', status: 'failure' }}
        className="size-3"
      />
    )

    expect(failing).toContain('text-rose-500/85')
  })

  it('renders merged reviews with the merge glyph', () => {
    const merged = renderToStaticMarkup(
      <ReviewIcon
        review={{ provider: 'github', number: 1, title: 'Merged', state: 'merged' }}
        className="size-3"
      />
    )

    expect(merged).toContain('lucide-git-merge')
  })

  // Why: folder cards render a stateless row while a linked review loads or its
  // details fail. There is no state glyph to contradict, so problems must still be
  // visible — but a tone-less row must not be promoted to a passing green.
  it('flags problems on a stateless row without ever claiming success', () => {
    const stateless = (status: 'failure' | 'pending' | 'success') =>
      renderToStaticMarkup(
        <ReviewIcon
          review={{ provider: 'github', number: 1, title: 'Loading PR...', status }}
          className="size-3"
        />
      )

    expect(stateless('failure')).toContain('text-rose-500/85')
    expect(stateless('pending')).toContain('text-amber-500/85')
    expect(stateless('success')).not.toContain('text-emerald-500/80')
  })

  it('renders the pending tone for an open review awaiting checks', () => {
    const pending = renderToStaticMarkup(<ReviewIcon review={githubReview} className="size-3" />)

    expect(pending).toContain('text-amber-500/85')
  })
})
