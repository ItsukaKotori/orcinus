import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { describe, expect, it } from 'vitest'

const COMPONENT_ROOT = __dirname

function componentSource(relativePath: string): string {
  return readFileSync(join(COMPONENT_ROOT, relativePath), 'utf8').replace(/\r\n?/g, '\n')
}

function sourceBetween(source: string, startPattern: string, endPattern: string): string {
  const start = source.indexOf(startPattern)
  expect(start).toBeGreaterThanOrEqual(0)
  const end = source.indexOf(endPattern, start + startPattern.length)
  expect(end).toBeGreaterThan(start)
  return source.slice(start, end)
}

describe('task drawer source boundaries', () => {
  it('threads GitHub task source context through detail mutations', () => {
    const source = [
      componentSource('github-item-dialog/edit-item-fields/gh-edit-section.tsx'),
      componentSource('github-item-dialog/edit-item-fields/gh-edit-section-mutations.ts')
    ].join('\n')
    const issueUpdate = sourceBetween(
      componentSource('github/github-work-item-edit-mutations.ts'),
      'async function runIssueUpdate',
      'async function runWorkItemBodyUpdate'
    )
    const commentUpdate = sourceBetween(
      componentSource('github/github-work-item-comment-mutations.ts'),
      'function addIssueCommentForRepo',
      'function addPRReviewCommentForRepo'
    )
    const editSection = source

    expect(issueUpdate).toContain('sourceContext: args.sourceContext')
    expect(commentUpdate).toContain('sourceContext: args.sourceContext')
    expect(editSection).toContain('sourceContext,')
    expect(editSection).toContain(
      'patchWorkItem(itemId, { state: newState }, itemRepoId, { sourceContext })'
    )
    expect(editSection).toContain(
      'patchWorkItem(itemId, { labels: newLabels }, itemRepoId, { sourceContext })'
    )
  })






})
