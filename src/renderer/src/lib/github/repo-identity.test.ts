import { describe, expect, it, vi } from 'vitest'
import { createRepoIdentityResolver } from './repo-identity'
import type { GhExecClient } from './repo-identity'

type Remote = { name: string; url: string }

function makeResolver(remotes: Record<string, Remote[]>) {
  let nowValue = 0
  const readRemoteUrls = vi.fn(async (path: string) => remotes[path] ?? [])
  const client = { run: vi.fn(), runOrThrow: vi.fn() }
  const resolver = createRepoIdentityResolver({
    client: client as unknown as GhExecClient,
    readRemoteUrls,
    now: () => nowValue
  })
  return {
    resolver,
    client,
    readRemoteUrls,
    advance: (ms: number) => {
      nowValue += ms
    }
  }
}

describe('repo identity', () => {
  it('derives upstream-first candidates with origin as head repo', async () => {
    const { resolver } = makeResolver({
      '/repo': [
        { name: 'origin', url: 'git@github.com:me/fork.git' },
        { name: 'upstream', url: 'https://github.com/org/repo.git' }
      ]
    })
    const { candidates, headRepo } = await resolver.resolveCandidates('/repo')
    expect(candidates).toEqual([
      { owner: 'org', repo: 'repo', host: undefined },
      { owner: 'me', repo: 'fork', host: undefined }
    ])
    expect(headRepo).toEqual({ owner: 'me', repo: 'fork', host: undefined })
  })

  it('returns origin only when upstream is absent or identical', async () => {
    const { resolver } = makeResolver({
      '/repo': [{ name: 'origin', url: 'https://github.com/org/repo.git' }],
      '/same': [
        { name: 'origin', url: 'https://github.com/org/repo.git' },
        { name: 'upstream', url: 'git@github.com:org/repo.git' }
      ]
    })
    const only = await resolver.resolveCandidates('/repo')
    expect(only.candidates).toEqual([{ owner: 'org', repo: 'repo', host: undefined }])
    expect(only.headRepo?.owner).toBe('org')

    const identical = await resolver.resolveCandidates('/same')
    expect(identical.candidates).toEqual([{ owner: 'org', repo: 'repo', host: undefined }])
    expect(identical.headRepo?.owner).toBe('org')
  })

  it('ignores non-GitHub remotes', async () => {
    const { resolver } = makeResolver({
      '/repo': [{ name: 'origin', url: 'git@gitlab.com:org/repo.git' }]
    })
    const { candidates, headRepo } = await resolver.resolveCandidates('/repo')
    expect(candidates).toEqual([])
    expect(headRepo).toBeNull()
  })

  it('caches positive identity for 30s and negative for 5min', async () => {
    const remotes: Record<string, Remote[]> = {
      '/repo': [{ name: 'origin', url: 'https://github.com/org/repo.git' }]
    }
    const { resolver, readRemoteUrls, advance } = makeResolver(remotes)

    expect((await resolver.resolveCandidates('/repo')).candidates).toHaveLength(1)
    expect(readRemoteUrls).toHaveBeenCalledTimes(1)

    remotes['/repo'] = []
    expect((await resolver.resolveCandidates('/repo')).candidates).toHaveLength(1)
    expect(readRemoteUrls).toHaveBeenCalledTimes(1)

    advance(31_000)
    expect((await resolver.resolveCandidates('/repo')).candidates).toHaveLength(0)
    expect(readRemoteUrls).toHaveBeenCalledTimes(2)

    remotes['/repo'] = [{ name: 'origin', url: 'https://github.com/org/repo.git' }]
    expect((await resolver.resolveCandidates('/repo')).candidates).toHaveLength(0)
    expect(readRemoteUrls).toHaveBeenCalledTimes(2)

    advance(5 * 60_000 + 1)
    expect((await resolver.resolveCandidates('/repo')).candidates).toHaveLength(1)
    expect(readRemoteUrls).toHaveBeenCalledTimes(3)
  })

  it('does not cache read failures and propagates them', async () => {
    let failNext = true
    const readRemoteUrls = vi.fn(async () => {
      if (failNext) {
        failNext = false
        throw new Error('git remote failed')
      }
      return [{ name: 'origin', url: 'https://github.com/org/repo.git' }] satisfies Remote[]
    })
    const resolver = createRepoIdentityResolver({
      client: { run: vi.fn(), runOrThrow: vi.fn() } as unknown as GhExecClient,
      readRemoteUrls,
      now: () => 0
    })

    await expect(resolver.resolveCandidates('/repo')).rejects.toThrow('git remote failed')
    const recovered = await resolver.resolveCandidates('/repo')
    expect(recovered.candidates).toEqual([{ owner: 'org', repo: 'repo', host: undefined }])
    expect(readRemoteUrls).toHaveBeenCalledTimes(2)
  })

  it('re-resolves unresolved ssh host aliases without negative caching', async () => {
    const readRemoteUrls = vi.fn(
      async () => [{ name: 'origin', url: 'git@github-work:org/repo.git' }] satisfies Remote[]
    )
    const resolver = createRepoIdentityResolver({
      client: { run: vi.fn(), runOrThrow: vi.fn() } as unknown as GhExecClient,
      readRemoteUrls,
      now: () => 0
    })

    expect((await resolver.resolveCandidates('/repo')).candidates).toEqual([])
    await resolver.resolveCandidates('/repo')
    expect(readRemoteUrls).toHaveBeenCalledTimes(2)
  })

  it('gates non-default hosts on gh auth inventory', async () => {
    const { resolver, client } = makeResolver({
      '/repo': [{ name: 'origin', url: 'https://ghe.internal:8443/org/repo.git' }]
    })
    client.run.mockResolvedValueOnce({
      stdout: '',
      stderr:
        "ghe.internal:8443\n  ✓ Logged in to ghe.internal:8443 account bob (keyring)\n  - Active account: true\n  - Token scopes: 'repo'\n",
      code: 1
    })
    const slug = await resolver.getRepoSlug('/repo')
    expect(slug).toEqual({ owner: 'org', repo: 'repo', host: 'ghe.internal:8443' })
    expect(client.run).toHaveBeenCalledWith(['auth', 'status'])
  })

  it('drops unauthenticated GHES candidates from resolveCandidates', async () => {
    const { resolver, client } = makeResolver({
      '/repo': [{ name: 'origin', url: 'https://ghe.internal:8443/org/repo.git' }]
    })
    client.run.mockResolvedValueOnce({
      stdout: '',
      stderr: 'You are not logged into any GitHub hosts.',
      code: 1
    })
    await expect(resolver.resolveCandidates('/repo')).resolves.toEqual({
      candidates: [],
      headRepo: null
    })
  })

  it('treats unauthenticated GHES hosts as non-GitHub', async () => {
    const { resolver, client } = makeResolver({
      '/repo': [{ name: 'origin', url: 'https://ghe.internal:8443/org/repo.git' }]
    })
    client.run.mockResolvedValueOnce({
      stdout: '',
      stderr: 'You are not logged into any GitHub hosts.',
      code: 1
    })
    expect(await resolver.getRepoSlug('/repo')).toBeNull()
  })

  it('caches the gh auth inventory for 60s including negative results', async () => {
    const { resolver, client, advance } = makeResolver({
      '/repo': [{ name: 'origin', url: 'https://ghe.internal:8443/org/repo.git' }]
    })
    client.run.mockResolvedValue({
      stdout: '',
      stderr: 'You are not logged into any GitHub hosts.',
      code: 1
    })
    expect(await resolver.getRepoSlug('/repo')).toBeNull()
    expect(client.run).toHaveBeenCalledTimes(1)
    await resolver.getRepoSlug('/repo')
    expect(client.run).toHaveBeenCalledTimes(1)
    advance(60_001)
    await resolver.getRepoSlug('/repo')
    expect(client.run).toHaveBeenCalledTimes(2)
  })

  it('does not probe gh for default github.com hosts', async () => {
    const { resolver, client } = makeResolver({
      '/repo': [{ name: 'origin', url: 'https://github.com/org/repo.git' }]
    })
    expect(await resolver.getRepoSlug('/repo')).toEqual({ owner: 'org', repo: 'repo' })
    expect(client.run).not.toHaveBeenCalled()
  })

  it('resolves upstream via gh repo view parent when no upstream remote', async () => {
    const { resolver, client } = makeResolver({
      '/repo': [{ name: 'origin', url: 'https://github.com/me/fork.git' }]
    })
    client.runOrThrow.mockResolvedValueOnce(
      JSON.stringify({ isFork: true, parent: { name: 'repo', owner: { login: 'org' } } })
    )
    await expect(resolver.getRepoUpstream('/repo')).resolves.toEqual({
      owner: 'org',
      repo: 'repo',
      host: undefined
    })
    expect(client.runOrThrow).toHaveBeenCalledWith(
      ['repo', 'view', 'me/fork', '--json', 'isFork,parent'],
      { timeoutMs: 10_000 }
    )
  })

  it('prefers a distinct upstream remote over gh repo view', async () => {
    const { resolver, client } = makeResolver({
      '/repo': [
        { name: 'origin', url: 'https://github.com/me/fork.git' },
        { name: 'upstream', url: 'https://github.com/org/repo.git' }
      ]
    })
    await expect(resolver.getRepoUpstream('/repo')).resolves.toEqual({
      owner: 'org',
      repo: 'repo',
      host: undefined
    })
    expect(client.runOrThrow).not.toHaveBeenCalled()
  })

  it('returns null for an unauthenticated GHES upstream without probing repo view', async () => {
    const { resolver, client } = makeResolver({
      '/repo': [
        { name: 'origin', url: 'https://ghe.internal:8443/me/fork.git' },
        { name: 'upstream', url: 'https://ghe.internal:8443/org/repo.git' }
      ]
    })
    client.run.mockResolvedValue({
      stdout: '',
      stderr: 'You are not logged into any GitHub hosts.',
      code: 1
    })
    await expect(resolver.getRepoUpstream('/repo')).resolves.toBeNull()
    expect(client.runOrThrow).not.toHaveBeenCalled()
  })

  it('returns null when the fork probe fails or reports no parent', async () => {
    const failing = makeResolver({
      '/repo': [{ name: 'origin', url: 'https://github.com/me/fork.git' }]
    })
    failing.client.runOrThrow.mockRejectedValueOnce(new Error('gh failed'))
    await expect(failing.resolver.getRepoUpstream('/repo')).resolves.toBeNull()

    const nonFork = makeResolver({
      '/repo': [{ name: 'origin', url: 'https://github.com/me/fork.git' }]
    })
    nonFork.client.runOrThrow.mockResolvedValueOnce(
      JSON.stringify({ isFork: false, parent: null })
    )
    await expect(nonFork.resolver.getRepoUpstream('/repo')).resolves.toBeNull()

    const noOrigin = makeResolver({
      '/repo': [{ name: 'upstream', url: 'https://github.com/org/repo.git' }]
    })
    await expect(noOrigin.resolver.getRepoUpstream('/repo')).resolves.toBeNull()
    expect(noOrigin.client.runOrThrow).not.toHaveBeenCalled()
  })
})
