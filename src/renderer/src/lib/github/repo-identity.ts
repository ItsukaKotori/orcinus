import { invokeCommand } from '../../../../bridge/real/invoke'
import {
  foldComparableGitLabHost,
  isUnresolvedSshHostAlias,
  normalizeGitHubRemoteHost
} from '../../../../shared/git-remote-host-alias'
import { deriveGitRemoteIdentity, splitGitRemoteKey } from '../../../../shared/git-remote-identity'
import {
  githubRepoIdentityKey,
  isDefaultGitHubHost
} from '../../../../shared/github/repository-identity-key'
import { parseAuthStatus } from './auth-diagnose'
import type { GhExecOptions, GhExecResult } from './gh-exec-client'

export type GitHubRepoIdentity = { owner: string; repo: string; host?: string }

/** Structural shape of `createGhExecClient`'s return value (Task 3). */
export type GhExecClient = {
  run: (args: string[], options?: GhExecOptions) => Promise<GhExecResult>
  runOrThrow: (args: string[], options?: GhExecOptions) => Promise<string>
}

export type GitRemoteUrl = { name: string; url: string }

export type RepoIdentityCandidates = {
  candidates: GitHubRepoIdentity[]
  headRepo: GitHubRepoIdentity | null
}

export type RepoIdentityResolver = {
  resolveCandidates: (worktreePath: string) => Promise<RepoIdentityCandidates>
  getRepoSlug: (worktreePath: string) => Promise<GitHubRepoIdentity | null>
  getRepoUpstream: (worktreePath: string) => Promise<GitHubRepoIdentity | null>
}

export type RepoIdentityResolverDeps = {
  client: GhExecClient
  /** Defaults to the `git_remote_urls` bridge command. */
  readRemoteUrls?: (worktreePath: string) => Promise<GitRemoteUrl[]>
  now?: () => number
}

export const POSITIVE_TTL_MS = 30_000
export const NEGATIVE_TTL_MS = 5 * 60_000
export const AUTH_INVENTORY_TTL_MS = 60_000
export const REPO_VIEW_TIMEOUT_MS = 10_000

export async function readRemoteUrlsViaBridge(worktreePath: string): Promise<GitRemoteUrl[]> {
  return invokeCommand<GitRemoteUrl[]>('git_remote_urls', { args: { worktreePath } })
}

// Why: only public forges that can never be a GHES endpoint are dropped up
// front. Any other host is a GHES candidate and is gated on `gh auth status`
// later, so an internal hostname without a GitHub-ish shape still works.
const KNOWN_NON_GITHUB_HOSTS = new Set(['bitbucket.org'])

function bareHost(host: string): string {
  return host.trim().toLowerCase().replace(/:\d+$/, '')
}

function isKnownNonGitHubHost(host: string): boolean {
  const bare = bareHost(host)
  return KNOWN_NON_GITHUB_HOSTS.has(bare) || foldComparableGitLabHost(bare) === 'gitlab.com'
}

function scpRemoteHost(remoteUrl: string): string | null {
  const match = /^(?:[^@\s/]+@)?([^:\s/]+):/.exec(remoteUrl.trim())
  return match?.[1] ? normalizeGitHubRemoteHost(match[1]) : null
}

/** HTTP(S) ports identify the API endpoint; SSH/git ports are transport-only. */
function rawRemoteHost(remoteUrl: string): string | null {
  const trimmed = remoteUrl.trim()
  if (!trimmed) {
    return null
  }
  if (!trimmed.includes('://')) {
    return scpRemoteHost(trimmed)
  }
  try {
    const url = new URL(trimmed)
    const protocol = url.protocol.toLowerCase()
    if (!['git:', 'git+ssh:', 'http:', 'https:', 'ssh:'].includes(protocol)) {
      return null
    }
    const host = protocol === 'http:' || protocol === 'https:' ? url.host : url.hostname
    return host ? normalizeGitHubRemoteHost(host) : null
  } catch {
    return null
  }
}

/**
 * The canonical key drops explicit ports, but GHES identity needs HTTP(S)
 * endpoint ports back to satisfy the auth-inventory exact match.
 */
function effectiveHost(remoteUrl: string, canonicalHost: string): string {
  const rawHost = rawRemoteHost(remoteUrl)
  if (!rawHost || rawHost === canonicalHost) {
    return canonicalHost
  }
  return bareHost(rawHost) === canonicalHost ? rawHost : canonicalHost
}

// Why: a dotless SSH host is an OpenSSH `Host` alias that only `ssh -G` can
// expand, so its GitHub-ness is unknowable here; the result must not be cached.
function isIndeterminateAlias(remoteUrl: string, host: string): boolean {
  if (!isUnresolvedSshHostAlias(host)) {
    return false
  }
  const trimmed = remoteUrl.trim().toLowerCase()
  return (
    !trimmed.includes('://') || trimmed.startsWith('ssh://') || trimmed.startsWith('git+ssh://')
  )
}

type ParsedRemote = { identity: GitHubRepoIdentity | null; indeterminate: boolean }

function parseRemoteIdentity(remote: GitRemoteUrl): ParsedRemote {
  const canonicalKey = deriveGitRemoteIdentity(`${remote.name}\t${remote.url} (fetch)\n`)
    ?.canonicalKey
  const parts = splitGitRemoteKey(canonicalKey, normalizeGitHubRemoteHost)
  if (!parts) {
    return { identity: null, indeterminate: false }
  }
  const host = effectiveHost(remote.url, parts.host)
  if (isIndeterminateAlias(remote.url, host)) {
    return { identity: null, indeterminate: true }
  }
  const tailSegments = parts.tail.split('/')
  if (tailSegments.length !== 2) {
    return { identity: null, indeterminate: false }
  }
  const [owner, repo] = tailSegments
  if (!owner || !repo || isKnownNonGitHubHost(host)) {
    return { identity: null, indeterminate: false }
  }
  return {
    identity: {
      owner,
      repo,
      ...(isDefaultGitHubHost(host) ? {} : { host })
    },
    indeterminate: false
  }
}

function cloneResult(result: RepoIdentityCandidates): RepoIdentityCandidates {
  return {
    candidates: result.candidates.map((identity) => ({ ...identity })),
    headRepo: result.headRepo ? { ...result.headRepo } : null
  }
}

export function createRepoIdentityResolver(deps: RepoIdentityResolverDeps): RepoIdentityResolver {
  const readRemoteUrls = deps.readRemoteUrls ?? readRemoteUrlsViaBridge
  const now = deps.now ?? (() => Date.now())
  const candidateCache = new Map<string, { result: RepoIdentityCandidates; expiresAt: number }>()
  const authInventoryCache = new Map<string, { authenticated: boolean; expiresAt: number }>()

  async function ensureHostAuthenticated(host: string): Promise<boolean> {
    const normalizedHost = host.trim().toLowerCase()
    if (!normalizedHost) {
      return false
    }
    const cached = authInventoryCache.get(normalizedHost)
    if (cached && cached.expiresAt > now()) {
      return cached.authenticated
    }
    let authenticated = false
    try {
      const result = await deps.client.run(['auth', 'status'])
      const accounts = parseAuthStatus(`${result.stdout}\n${result.stderr}`)
      authenticated = accounts.some(
        (account) => account.host.trim().toLowerCase() === normalizedHost
      )
    } catch {
      authenticated = false
    }
    authInventoryCache.set(normalizedHost, {
      authenticated,
      expiresAt: now() + AUTH_INVENTORY_TTL_MS
    })
    return authenticated
  }

  async function resolveCandidates(worktreePath: string): Promise<RepoIdentityCandidates> {
    const cacheKey = `${worktreePath}\0candidates`
    const cached = candidateCache.get(cacheKey)
    if (cached && cached.expiresAt > now()) {
      return cloneResult(cached.result)
    }

    const rows = await readRemoteUrls(worktreePath)
    const upstreamRow = rows.find((row) => row.name === 'upstream')
    const originRow = rows.find((row) => row.name === 'origin')
    const upstream = upstreamRow ? parseRemoteIdentity(upstreamRow) : null
    const origin = originRow ? parseRemoteIdentity(originRow) : null
    const indeterminate = upstream?.indeterminate === true || origin?.indeterminate === true

    const candidates: GitHubRepoIdentity[] = []
    const seen = new Set<string>()
    for (const identity of [upstream?.identity ?? null, origin?.identity ?? null]) {
      if (!identity) {
        continue
      }
      const key = githubRepoIdentityKey(identity)
      if (seen.has(key)) {
        continue
      }
      seen.add(key)
      candidates.push(identity)
    }
    const result: RepoIdentityCandidates = { candidates, headRepo: origin?.identity ?? null }

    if (!indeterminate) {
      const ttl = candidates.length > 0 ? POSITIVE_TTL_MS : NEGATIVE_TTL_MS
      candidateCache.set(cacheKey, { result, expiresAt: now() + ttl })
    }
    return cloneResult(result)
  }

  async function getRepoSlug(worktreePath: string): Promise<GitHubRepoIdentity | null> {
    const { headRepo } = await resolveCandidates(worktreePath)
    if (!headRepo) {
      return null
    }
    if (isDefaultGitHubHost(headRepo.host)) {
      return headRepo
    }
    return (await ensureHostAuthenticated(headRepo.host ?? '')) ? headRepo : null
  }

  async function getRepoUpstream(worktreePath: string): Promise<GitHubRepoIdentity | null> {
    const { candidates, headRepo: origin } = await resolveCandidates(worktreePath)
    if (!origin) {
      return null
    }
    const originKey = githubRepoIdentityKey(origin)
    const upstream = candidates.find(
      (candidate) => githubRepoIdentityKey(candidate) !== originKey
    )
    if (upstream) {
      return upstream
    }
    if (!isDefaultGitHubHost(origin.host) && !(await ensureHostAuthenticated(origin.host ?? ''))) {
      return null
    }
    try {
      const hostPrefix = isDefaultGitHubHost(origin.host) ? '' : `${origin.host ?? ''}/`
      const stdout = await deps.client.runOrThrow(
        ['repo', 'view', `${hostPrefix}${origin.owner}/${origin.repo}`, '--json', 'isFork,parent'],
        { timeoutMs: REPO_VIEW_TIMEOUT_MS }
      )
      const data = JSON.parse(stdout) as {
        isFork?: unknown
        parent?: { name?: unknown; owner?: { login?: unknown } } | null
      }
      const parentName = typeof data.parent?.name === 'string' ? data.parent.name : null
      const parentOwner =
        data.parent && typeof data.parent.owner?.login === 'string'
          ? data.parent.owner.login
          : null
      if (data.isFork !== true || !parentName || !parentOwner) {
        return null
      }
      return {
        owner: parentOwner,
        repo: parentName,
        ...(isDefaultGitHubHost(origin.host) ? {} : { host: origin.host })
      }
    } catch {
      return null
    }
  }

  return { resolveCandidates, getRepoSlug, getRepoUpstream }
}
