// @vitest-environment happy-dom
import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { GitRefEntry } from '../../../../shared/git-ref-types'
import { useBranchRefs } from './use-branch-refs'

;(globalThis as unknown as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true

const MAIN: GitRefEntry = {
  name: 'main',
  fullName: 'refs/heads/main',
  category: 'local',
  oid: '0'.repeat(40),
  isHead: true,
}

type Result = ReturnType<typeof useBranchRefs>

const refs = vi.fn()

let host: HTMLDivElement
let root: Root
let latest: Result
let api: { current: GitRefEntry[]; truncated: boolean; fail: boolean }

function Probe({ path }: { path: string | null }): React.JSX.Element {
  latest = useBranchRefs(path)
  return <span>{latest.loading ? 'loading' : 'idle'}</span>
}

beforeEach(() => {
  host = document.createElement('div')
  document.body.append(host)
  root = createRoot(host)
  api = { current: [MAIN], truncated: false, fail: false }
  refs.mockReset().mockImplementation(() =>
    api.fail
      ? Promise.reject(new Error('boom'))
      : Promise.resolve({ refs: api.current, truncated: api.truncated })
  )
  // 只挂 api，不替换 window（替换会让 Radix 等依赖真 window 的库挂掉）。
  ;(window as unknown as { api: unknown }).api = { git: { refs } }
  latest = { refs: [], truncated: false, loading: false, error: null, invalidate: () => {} }
})
afterEach(() => {
  act(() => root.unmount())
  host.remove()
  delete (window as unknown as { api?: unknown }).api
})

async function render(path: string | null = '/wt'): Promise<void> {
  await act(async () => {
    root.render(<Probe path={path} />)
  })
  await act(async () => {
    await Promise.resolve()
  })
}

describe('useBranchRefs', () => {
  it('挂载取一次，refs/truncated 落进返回值，loading 归 false', async () => {
    await render()
    expect(refs).toHaveBeenCalledTimes(1)
    expect(refs).toHaveBeenCalledWith({ worktreePath: '/wt' })
    expect(latest.refs).toEqual([MAIN])
    expect(latest.truncated).toBe(false)
    expect(latest.loading).toBe(false)
    expect(latest.error).toBeNull()
  })

  it('truncated 透传给调用方', async () => {
    api.truncated = true
    await render()
    expect(latest.truncated).toBe(true)
  })

  it('path 为 null 时不请求，且 loading 直接是 false', async () => {
    await render(null)
    expect(refs).not.toHaveBeenCalled()
    expect(latest.loading).toBe(false)
    expect(latest.refs).toEqual([])
  })

  it('invalidate 再取一次并换上新数据', async () => {
    await render()
    api.current = [{ ...MAIN, name: 'dev', fullName: 'refs/heads/dev' }]
    await act(async () => {
      latest.invalidate()
    })
    await act(async () => {
      await Promise.resolve()
    })
    expect(refs).toHaveBeenCalledTimes(2)
    expect(latest.refs[0]?.name).toBe('dev')
  })

  it('失败降级为空列表 + error，不把异常抛给渲染方', async () => {
    api.fail = true
    await render()
    expect(latest.refs).toEqual([])
    expect(latest.error).toBe('boom')
    expect(latest.loading).toBe(false)
  })

  it('path 变化重新取数', async () => {
    await render('/a')
    await act(async () => {
      root.render(<Probe path="/b" />)
    })
    await act(async () => {
      await Promise.resolve()
    })
    expect(refs).toHaveBeenCalledTimes(2)
    expect(refs).toHaveBeenLastCalledWith({ worktreePath: '/b' })
  })

  it('成功重取后清掉上一次的 error', async () => {
    api.fail = true
    await render()
    expect(latest.error).toBe('boom')
    api.fail = false
    await act(async () => {
      latest.invalidate()
    })
    await act(async () => {
      await Promise.resolve()
    })
    expect(latest.error).toBeNull()
    expect(latest.refs).toEqual([MAIN])
  })

  it('invalidate 是稳定引用（不会因它导致 effect 自触发循环）', async () => {
    await render()
    const first = latest.invalidate
    await act(async () => {
      latest.invalidate()
    })
    await act(async () => {
      await Promise.resolve()
    })
    expect(latest.invalidate).toBe(first)
    // 两次请求都来自同一路径；若 invalidate 进了 effect 依赖会形成无限重取。
    expect(refs.mock.calls.map((c) => c[0])).toEqual([
      { worktreePath: '/wt' },
      { worktreePath: '/wt' },
    ])
  })

  it('卸载后到达的响应不写 state', async () => {
    let resolve!: (v: { refs: GitRefEntry[]; truncated: boolean }) => void
    refs.mockImplementationOnce(
      () =>
        new Promise((r) => {
          resolve = r
        })
    )
    await act(async () => {
      root.render(<Probe path="/wt" />)
    })
    act(() => root.unmount())
    root = createRoot(host)
    expect(() => resolve({ refs: [MAIN], truncated: false })).not.toThrow()
  })
})
