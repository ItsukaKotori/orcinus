import { useCallback, useEffect, useState } from 'react'
import type { GitRefEntry } from '../../../../shared/git-ref-types'

export type UseBranchRefsResult = {
  refs: GitRefEntry[]
  truncated: boolean
  loading: boolean
  error: string | null
  /** 让下一次渲染重新取数（写操作成功后由调用方触发）。 */
  invalidate: () => void
}

/**
 * `git_refs` 的取数 hook。
 *
 * 刻意不做缓存/去重/并发合流：refs 是一次 `for-each-ref`，便宜且必须新鲜
 * （分支被外部增删是常态），缓存只会制造第二份真相。
 *
 * `invalidate` 走 version 计数器而不是把自身塞进 effect 依赖 —— 后者会自触发成环。
 */
export function useBranchRefs(worktreePath: string | null): UseBranchRefsResult {
  const [refs, setRefs] = useState<GitRefEntry[]>([])
  const [truncated, setTruncated] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [loading, setLoading] = useState(worktreePath !== null)
  const [version, setVersion] = useState(0)

  const invalidate = useCallback(() => setVersion((v) => v + 1), [])

  useEffect(() => {
    if (!worktreePath) {
      setLoading(false)
      return
    }
    let alive = true
    setLoading(true)
    window.api.git
      .refs({ worktreePath })
      .then((result) => {
        if (!alive) return
        setRefs(result.refs)
        setTruncated(result.truncated)
        setError(null)
      })
      .catch((e: unknown) => {
        if (!alive) return
        // 降级为空列表：popover 出空态而不是崩面板（spec §7.5）。
        setRefs([])
        setTruncated(false)
        setError(e instanceof Error ? e.message : String(e))
      })
      .finally(() => {
        if (alive) setLoading(false)
      })
    return () => {
      alive = false
    }
  }, [worktreePath, version])

  return { refs, truncated, loading, error, invalidate }
}
