import type { AppState } from '../types'
import type { Repo } from '../../../../shared/repo-types'
import { syncRuntimeGitForkDefaultBranch } from '../../runtime/runtime-git-client'
import { getRepoExecutionHostId } from '../../../../shared/execution-host'
import { getRepoMainWorktreeId } from '../../../../shared/worktree/id'
import { settingsForRepoOwner } from './owner-routing'

export const SAFE_AUTO_FORK_SYNC_COOLDOWN_MS = 10 * 60 * 1000

export const safeAutoForkSyncAttempts = new Map<
  string,
  { attemptedAt: number; promise?: Promise<void> }
>()

export function getSafeAutoForkSyncKey(repo: Repo): string {
  return `${getRepoExecutionHostId(repo)}:${repo.id}:${repo.path}`
}

export function scheduleSafeAutoForkSync(get: () => AppState, repos: readonly Repo[]): void {
  for (const repo of repos) {
    if (repo.kind === 'folder' || repo.forkSyncMode !== 'safe-auto' || !repo.upstream) {
      continue
    }
    const key = getSafeAutoForkSyncKey(repo)
    const existingAttempt = safeAutoForkSyncAttempts.get(key)
    const now = Date.now()
    if (
      existingAttempt?.promise ||
      (existingAttempt && now - existingAttempt.attemptedAt < SAFE_AUTO_FORK_SYNC_COOLDOWN_MS)
    ) {
      continue
    }
    const expected = repo.upstream
    const promise = (async () => {
      // 增量5 终审 #4：fork-sync 现为真后台写（ls-remote+fetch+push，最长 300s）。纳入 store 远端门
      // beginRemoteOperation/endRemoteOperation，使 remoteOpActive 期间树/头部写动作置灰（§8.2）。
      const app = get()
      app.beginRemoteOperation('sync')
      try {
        await syncRuntimeGitForkDefaultBranch(
          {
            settings: settingsForRepoOwner(get(), repo.id),
            worktreeId: getRepoMainWorktreeId(repo),
            worktreePath: repo.path,
            connectionId: repo.connectionId ?? undefined
          },
          expected
        )
        // 成功后 fire-and-forget 刷 upstream（§8.3）；safe-auto 机会性、不阻断。
        void get().fetchUpstreamStatus(getRepoMainWorktreeId(repo), repo.path, repo.connectionId ?? undefined)
      } catch (error) {
        // Why: safe-auto is opportunistic; auth/protection/divergence failures shouldn't add startup noise (Sync Now handles explicit diagnosis).
        console.info('Safe fork auto-sync skipped', error)
      } finally {
        get().endRemoteOperation()
      }
    })()
      .finally(() => {
        const current = safeAutoForkSyncAttempts.get(key)
        if (current?.promise === promise) {
          safeAutoForkSyncAttempts.set(key, { attemptedAt: now })
        }
      })
    safeAutoForkSyncAttempts.set(key, { attemptedAt: now, promise })
  }
}
