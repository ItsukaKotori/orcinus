// @vitest-environment happy-dom
import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import type { GitRefEntry } from '../../../../shared/git-ref-types'
import { BranchRefTree } from './branch-ref-tree'

;(globalThis as unknown as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true

function entry(over: Partial<GitRefEntry> & { name: string }): GitRefEntry {
  return {
    fullName: `refs/heads/${over.name}`,
    category: 'local',
    oid: '0'.repeat(40),
    isHead: false,
    ...over,
  }
}

const REFS: GitRefEntry[] = [
  entry({ name: 'main', isHead: true, ahead: 2, behind: 1, upstream: 'origin/main' }),
  entry({ name: 'feature/login' }),
  entry({
    name: 'origin/main',
    fullName: 'refs/remotes/origin/main',
    category: 'remote',
    remote: 'origin',
  }),
  entry({ name: 'v1.2.0', fullName: 'refs/tags/v1.2.0', category: 'tag' }),
]

let host: HTMLDivElement
let root: Root
let activated: string[]

beforeEach(() => {
  host = document.createElement('div')
  document.body.append(host)
  root = createRoot(host)
  activated = []
})
afterEach(() => {
  act(() => root.unmount())
  host.remove()
})

async function render(props: Partial<React.ComponentProps<typeof BranchRefTree>> = {}): Promise<void> {
  await act(async () => {
    root.render(
      <BranchRefTree
        refs={REFS}
        headBranch="main"
        onActivate={(row) => activated.push(row.id)}
        {...props}
      />
    )
  })
  await act(async () => {
    await Promise.resolve()
  })
}

const row = (id: string): HTMLElement | null => host.querySelector(`[data-testid="branch-row-${id}"]`)
const rowIds = (): string[] =>
  [...host.querySelectorAll('[data-testid^="branch-row-"]')].map((el) =>
    el.getAttribute('data-testid')!.replace('branch-row-', '')
  )

async function click(el: Element | null): Promise<void> {
  await act(async () => {
    el?.dispatchEvent(new MouseEvent('click', { bubbles: true }))
  })
}

async function press(el: Element | null, key: string): Promise<void> {
  await act(async () => {
    el?.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true }))
  })
}

/** happy-dom 下受控 input 必须走原生 value setter，否则 React 收不到 onChange。 */
async function typeSearch(value: string): Promise<void> {
  const input = host.querySelector('input[type="search"]')!
  const setter = Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype, 'value')?.set
  await act(async () => {
    setter?.call(input, value)
    input.dispatchEvent(new Event('input', { bubbles: true }))
  })
}

describe('BranchRefTree', () => {
  it('渲染 head-current 导航行、三组与 Local 里的当前分支行', async () => {
    await render()
    expect(row('row:head-current')).not.toBeNull()
    expect(row('ref:refs/heads/main')).not.toBeNull()
    expect(host.textContent).toContain('feature/login')
    expect(host.textContent).toContain('v1.2.0')
    expect(rowIds()).toContain('group:local')
    expect(rowIds()).toContain('group:remote/origin')
  })

  it('includeAllBranchesRow 把 All Branches 放在 head-current 之前', async () => {
    await render({ includeAllBranchesRow: true })
    expect(rowIds().slice(0, 2)).toEqual(['row:all-branches', 'row:head-current'])
  })

  it('ahead/behind 画在**当前分支的 ref 行**上（不是导航行）', async () => {
    await render()
    const main = row('ref:refs/heads/main')!
    expect(main.textContent).toContain('↑2')
    expect(main.textContent).toContain('↓1')
    expect(row('row:head-current')!.textContent).not.toContain('↑')
  })

  it('单边计数不补 0：仅领先时不得出现 ↓0', async () => {
    await render({
      refs: [entry({ name: 'main', isHead: true, ahead: 14 })],
    })
    const main = row('ref:refs/heads/main')!
    expect(main.textContent).toContain('↑14')
    // `↓0` 会被读成「落后 0」，而 tooltip 走 aheadOnlyLabel 说的是 "14 ahead"。
    expect(main.textContent).not.toContain('↓')
  })

  it('每行带 aria-level（depth+1），否则 AT 读到的是层级不明的 treeitem 平铺', async () => {
    await render()
    expect(row('group:local')!.getAttribute('aria-level')).toBe('1')
    expect(row('ref:refs/heads/main')!.getAttribute('aria-level')).toBe('2')
    expect(row('group:remote/origin')!.getAttribute('aria-level')).toBe('2')
    expect(row('ref:refs/remotes/origin/main')!.getAttribute('aria-level')).toBe('3')
  })

  it('checkedOutByBranch 出 ▣ 徽标，title 带 displayName', async () => {
    await render({
      checkedOutByBranch: new Map([
        ['feature/login', { worktreeId: 'r::/b', displayName: 'My App', path: '/b' }],
      ]),
    })
    const el = row('ref:refs/heads/feature/login')!
    expect(el.querySelector('[data-testid="branch-workspace-badge"]')).not.toBeNull()
    expect(el.getAttribute('title')).toContain('My App')
  })

  it('当前分支的 upstream 带 ★，且只此一行有 ★', async () => {
    await render()
    expect(host.querySelectorAll('[data-testid="branch-upstream-star"]')).toHaveLength(1)
    expect(row('ref:refs/remotes/origin/main')!.querySelector('[data-testid="branch-upstream-star"]')).not.toBeNull()
  })

  it('搜索裁剪 refs；无命中时出空态**但导航行仍在**', async () => {
    await render()
    await typeSearch('login')
    expect(host.textContent).toContain('feature/login')
    expect(host.textContent).not.toContain('v1.2.0')
    expect(row('row:head-current')).not.toBeNull()

    await typeSearch('zzz')
    expect(host.textContent).toContain('No matching branches.')
    // 导航行不被抹掉：否则用户得先清空搜索才能跳回当前分支范围。
    expect(row('row:head-current')).not.toBeNull()
    expect(row('ref:refs/heads/main')).toBeNull()
  })

  it('清掉搜索后，刚才折叠的组仍是折叠的', async () => {
    await render()
    await click(row('group:tags'))
    expect(row('ref:refs/tags/v1.2.0')).toBeNull()
    await typeSearch('login')
    await typeSearch('')
    expect(row('ref:refs/tags/v1.2.0')).toBeNull()
    await click(row('group:tags'))
    expect(row('ref:refs/tags/v1.2.0')).not.toBeNull()
  })

  it('单击 ref 行触发 onActivate；单击组头只折叠、不触发', async () => {
    await render()
    await click(row('ref:refs/heads/feature/login'))
    expect(activated).toEqual(['ref:refs/heads/feature/login'])
    await click(row('group:tags'))
    expect(activated).toHaveLength(1)
    expect(row('ref:refs/tags/v1.2.0')).toBeNull()
  })

  it('truncated=true 出截断提示，缺省不出', async () => {
    await render({ truncated: true })
    expect(host.textContent).toContain('Some refs are not shown')
    await render({ truncated: false })
    expect(host.textContent).not.toContain('Some refs are not shown')
  })

  it('空 refs 出空态而不是空面板', async () => {
    await render({ refs: [] })
    expect(host.textContent).toContain('No matching branches.')
  })

  it('activeValue 高亮对应行', async () => {
    await render({ activeValue: 'ref:refs/heads/main' })
    expect(row('ref:refs/heads/main')!.getAttribute('aria-selected')).toBe('true')
    expect(row('ref:refs/heads/feature/login')!.getAttribute('aria-selected')).toBe('false')
  })

  it('搜索框里按 ↓ 进第一行；↑↓ 在可见行间移动焦点', async () => {
    await render()
    const input = host.querySelector('input[type="search"]')!
    await press(input, 'ArrowDown')
    expect((document.activeElement as HTMLElement)?.dataset.rowIndex).toBe('0')
    await press(document.activeElement as HTMLElement, 'ArrowDown')
    expect((document.activeElement as HTMLElement)?.dataset.rowIndex).toBe('1')
    await press(document.activeElement as HTMLElement, 'ArrowUp')
    expect((document.activeElement as HTMLElement)?.dataset.rowIndex).toBe('0')
  })

  it('→ 展开、← 折叠当前组，与单击组头共用同一份折叠态', async () => {
    await render()
    await click(row('group:remote'))
    expect(row('ref:refs/remotes/origin/main')).toBeNull()
    await press(row('group:remote'), 'ArrowRight')
    expect(row('ref:refs/remotes/origin/main')).not.toBeNull()
    await press(row('group:remote'), 'ArrowLeft')
    expect(row('ref:refs/remotes/origin/main')).toBeNull()
  })

  it('←→ 落在非组行上不改变折叠态', async () => {
    await render()
    await press(row('ref:refs/heads/main'), 'ArrowRight')
    expect(row('ref:refs/remotes/origin/main')).not.toBeNull()
  })

  // Enter 不在此测：happy-dom 下 KeyboardEvent('Enter') 不会在 <button> 上合成 click，
  // 真浏览器行为由「单击 ref 行触发 onActivate」那条用例等价覆盖。
  //
  // 搜索框获焦也不在此测：机制是 requestAnimationFrame 后 focus（沿用
  // sidebar/SidebarProjectFilterPanel 的既有修法，因为 autoFocus 在 Radix 焦点陷阱里会被拉回），
  // happy-dom 下测 rAF 时序只会测到 polyfill 而不是真 Popover 行为。
  // → 由 Task 11 手工验收项「弹层打开后直接打字能过滤」兜住。
})

describe('renderRowMenu 接缝（增量 2B）', () => {
  it('未提供接缝：ref 行直接是 <button>，无 ⋯（宿主 B 零回归）', async () => {
    await render()
    const el = row('ref:refs/heads/feature/login')
    expect(el?.tagName).toBe('BUTTON')
    expect(host.querySelector('[data-testid="branch-row-more"]')).toBeNull()
  })

  it('提供接缝：ref 行经接缝包裹，且外层不出现 button 套 button', async () => {
    await render({
      renderRowMenu: (r, content) => (
        <div data-testid="seam-wrap" data-kind={r.kind}>
          {content}
          <button type="button" data-testid="branch-row-more">
            more
          </button>
        </div>
      ),
    })
    expect(host.querySelector('[data-testid="seam-wrap"]')).not.toBeNull()
    expect(host.querySelector('[data-testid="branch-row-more"]')).not.toBeNull()
    // 关键嵌套守卫：main button 与 ⋯ 是兄弟，不存在 button 内嵌 button
    expect(host.querySelector('button button')).toBeNull()
  })

  it('接缝只作用于 ref 行：group / synthetic 行不请求接缝', async () => {
    const kinds = new Set<string>()
    await render({
      renderRowMenu: (r, content) => {
        kinds.add(r.kind)
        return content
      },
    })
    expect([...kinds]).toEqual(['ref'])
    // 未包裹的 group/synthetic 行仍是原样 <button>
    expect(row('group:local')?.tagName).toBe('BUTTON')
  })
})

describe('§7.3 F2 → onRenameRequest', () => {
  it('F2 对焦点 ref 行回调该行', async () => {
    const hit: string[] = []
    await render({ onRenameRequest: (r) => { if (r.kind === 'ref') hit.push(r.ref.name) } })
    await press(row('ref:refs/heads/feature/login'), 'F2')
    expect(hit).toEqual(['feature/login'])
  })

  it('未声明 onRenameRequest 时 F2 无操作、不抛', async () => {
    await render()
    await expect(press(row('ref:refs/heads/feature/login'), 'F2')).resolves.toBeUndefined()
  })
})

describe('远程行剥 <remote>/ 前缀显示（组头已含 remote 名）', () => {
  it('origin/main 在远程组内显示为 main，不含 origin/', async () => {
    await render()
    const remoteRow = row('ref:refs/remotes/origin/main')
    expect(remoteRow).not.toBeNull()
    expect(remoteRow!.textContent).toContain('main')
    expect(remoteRow!.textContent).not.toContain('origin/')
  })
})
