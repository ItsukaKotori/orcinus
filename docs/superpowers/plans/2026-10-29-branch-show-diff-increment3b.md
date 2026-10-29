# Show Diff with Working Tree（增量 3b）Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 给常驻分支树区块接上 `Show Diff with Working Tree`（§7.4：非当前分支，Local+Remote）——选中分支 X → 打开一个**自带 X 改动列表**的 combined-branch「Branch Changes (X)」tab。

**Architecture:** 复用既有 `getRuntimeGitBranchCompare(baseRef=X)`（返回 `{summary, entries}`）+ 既有 `openBranchAllDiffs` + 既有 `CombinedDiffViewer`。核实证明 viewer 的 `useCombinedDiffEntrySet` 已经 `getCombinedBranchEntries(file.branchEntriesSnapshot, liveBranchEntries)`（快照优先、缺省回落 live）且 `branchCompare` 取 `file.branchCompare`（本就 per-tab）。唯一缺口：`openBranchAllDiffs` 把 `branchEntriesSnapshot` 硬设为**当前 worktree 缓存**（面板 base 的改动），对任意 X 会串文件列表。补法：`openBranchAllDiffs` 加**可选 `entries` 参数**，传入时以之为 tab 快照、缺省时保持读缓存（向后兼容）。无新后端命令。

**Tech Stack:** TS/React + zustand store action + Vitest(happy-dom)。cargo 侧无改动（`git_branch_compare` 已存在）。

**Spec:** `docs/superpowers/specs/2026-10-24-idea-branch-tree-switcher-design.md` line 57/329/583（Show Diff 复用现成 + §7.4 矩阵）。路线图 `docs/superpowers/plans/2026-10-26-branch-actions-roadmap-increments3-5.md`。

## Global Constraints

- **非破坏·纯本地只读**（无 git 写、无网络命令）→ 门禁：`pnpm typecheck` + git-branch + 广域 source-control/editor 定向 vitest；**不需要**全量 `pnpm test`（非破坏，同增量2/3 口径）。**不触碰** `worktree_remove`、checkout 写路径。
- **向后兼容优先**：`openBranchAllDiffs` 的既有 3 个调用方（`branch-section.tsx:99`、`CombinedDiffViewer.tsx:250` 自刷新、`use-store-actions.ts` 透传）签名不变（新参数可选、置末）。现有 Branch Changes tab 行为**逐字节不变**（今天它已把快照设为缓存、viewer 已优先快照，故本就 freeze-at-open——无可破坏的 live-update）。
- **宿主裁剪不变**：`show-diff` 仅在 `HOST_A_ALLOWED` 声明 + 宿主注入 `ctx.showDiff` 时出现；Git Log 宿主（B）维持不声明 → 不可见（§7.4：Checkout 在 B 不可见，show-diff 在 B 由其自身接线决定，本增量不动 B）。
- **compare 非 ready 不出 tab**：`summary.status !== 'ready'`（invalid-base / no-merge-base / unborn-head / loading / error）→ toast 错误、不开 tab（否则空/错列表）。
- **store 选择器防御**：区块被面板测试以部分 store 桩渲染 → `openBranchAllDiffs` 调用点用可选链，缺省不误触（记忆：面板内新增组件易崩面板测试）。
- i18n 只补 en+zh（其余靠 defaultValue）；`translate` 惰性（AST 门禁）。无 pre-commit 钩子，门禁手动。

## 已核实事实（本计划撰写前实测）

| 项 | 结论 |
|---|---|
| 后端 | `git_branch_compare` 命令 + `getRuntimeGitBranchCompare(context, baseRef, admissionTier='interactive')→GitBranchCompareResult` 均已存在（`runtime-git-diff-client.ts`，本地走 `window.api.git.branchCompare`）。 |
| `GitBranchCompareResult` | `{ summary: GitBranchCompareSummary, entries: GitBranchChangeEntry[] }`（`git-diff-compare-types.ts`）。 |
| viewer per-tab 就绪 | `useCombinedDiffEntrySet`：`branchCompare = file.branchCompare`（per-tab）；`branchEntries = getCombinedBranchEntries(file.branchEntriesSnapshot, liveBranchEntries)`，后者 `snapshot ?? live`（快照优先，空数组也保留）。→ **文件列表已可 per-tab，无需改 viewer**。 |
| 唯一缺口 | `open-combined-diff.ts:109` `openBranchAllDiffs` 把 `branchEntriesSnapshot = s.gitBranchChangesByWorktree[worktreeId] ?? []`（第114行，当前缓存），不吃传入 compare 的 entries。 |
| 类型接缝 | `editor-files-slice.ts` 已 import `GitBranchChangeEntry`，`openCommitAllDiffs` 已带 `entries` 参数（对称先例）。 |
| show-diff 动作 | `branch-actions.ts` 已有 `show-diff`（visible: `category!=='tag' && !isHead` → Local+Remote，合 §7.4），`ctx.showDiff?:(ref)=>void`；`HOST_A_ALLOWED` 当前**不含** show-diff。 |

## 文件结构

**修改**：`store/slices/editor/actions/open-combined-diff.ts`（openBranchAllDiffs 加 entries）、`store/slices/editor/types/editor-files-slice.ts`（签名）、`git-branch/source-control-branch-tree-section.tsx`（HOST_A_ALLOWED + ctx.showDiff + executeShowDiff）、i18n en/zh。
**测试**：`store/slices/editor` 侧 openBranchAllDiffs 快照参数断言（就近既有测试文件或新增）；`git-branch/source-control-branch-tree-section.show-diff.test.tsx`（新增，宿主接线集成）；动作表 show-diff 可见性已存在（复用）。

---

## Task 1: `openBranchAllDiffs` 支持显式 tab entries

**Interfaces:** `openBranchAllDiffs(worktreeId: string, worktreePath: string, compare: GitBranchCompareSummary, alternate?: CombinedDiffAlternate, entries?: GitBranchChangeEntry[]) => void`。

- [ ] **Step 1: 失败测试**（就近：若已有 open-combined-diff 测试则加用例；否则 store 动作级测试）：
  - 传 `entries=[X1,X2]` → 产生的 `OpenFile.branchEntriesSnapshot` 恰为 `[X1,X2]`（不受 `gitBranchChangesByWorktree` 当前值影响）。
  - 不传 `entries` → `branchEntriesSnapshot` = `gitBranchChangesByWorktree[worktreeId]`（向后兼容，现有行为）。
  - tab id 仍按 `compare.baseRef`+compareVersion 去重（同 ref 复用 tab）。
- [ ] **Step 2 跑测试失败。**
- [ ] **Step 3: 实现**（`open-combined-diff.ts`）：签名加尾参 `entries?: GitBranchChangeEntry[]`；两处 `branchEntriesSnapshot`（第114行的 set 前计算 + existing/newFile 都用它）改为 `const branchEntriesSnapshot = entries ?? s.gitBranchChangesByWorktree[worktreeId] ?? []`。`editor-files-slice.ts:145` 类型加 `entries?: GitBranchChangeEntry[]`。
- [ ] **Step 4: `pnpm typecheck` + 相关 vitest 绿。**
- [ ] **Step 5: 提交。**

## Task 2: 宿主 A 接线 show-diff（executeShowDiff + i18n）

**Files:** `source-control-branch-tree-section.tsx`、i18n en/zh + 新测试 `source-control-branch-tree-section.show-diff.test.tsx`。
- [ ] **Step 1: 失败集成测试**（照 writes 测试的 mock 菜单原语 + dialog 内联；mock `@/runtime/runtime-git-client` 的 `getRuntimeGitBranchCompare` + store 的 `openBranchAllDiffs` spy）：
  - 点 `Show Diff with Working Tree`（对 local `feature` 行）→ `getRuntimeGitBranchCompare` 收到 `('feature')`（context 含 worktreePath），成功后 `openBranchAllDiffs` 收到 `(worktreeId, worktreePath, summary, undefined, entries)`。
  - compare `summary.status='no-merge-base'` → `toast.error`、**不**调 openBranchAllDiffs。
  - compare reject → `toast.error`、不开。
  - show-diff 对 `isHead` 行 / tag 行不可见（动作表既有谓词，断 HOST_A_ALLOWED 已含 show-diff 后仍按谓词裁剪）。
- [ ] **Step 2 失败。**
- [ ] **Step 3: 实现**（section）：
  - import `getRuntimeGitBranchCompare`（from `@/runtime/runtime-git-client`）；`const openBranchAllDiffs = useAppStore((s) => s.openBranchAllDiffs)`。
  - `HOST_A_ALLOWED` 加 `'show-diff'`。
  - `executeShowDiff = useCallback(async (ref) => { if (!worktree) return; try { const res = await getRuntimeGitBranchCompare({ settings, worktreeId, worktreePath: worktree.path }, ref.name); if (res.summary.status !== 'ready') { toast.error(translate('...showDiffUnavailable', 'Cannot compare this branch with the working tree.')); return } openBranchAllDiffs?.(worktreeId, worktree.path, res.summary, undefined, res.entries) } catch (e) { toast.error(e instanceof Error ? e.message : String(e)) } }, [worktree, worktreeId, settings, openBranchAllDiffs])`。**只读，不进 writeInFlightRef/opInFlight。**
  - ctx 加 `showDiff: (ref) => void executeShowDiff(ref)`（deps 补 executeShowDiff）。
- [ ] **Step 4: i18n** en/zh：`showDiff`（"Show Diff with Working Tree" / "与工作区显示差异"）、`showDiffUnavailable`（如上）。
- [ ] **Step 5: 测试绿 + 提交。**

## 收尾门禁（非破坏）

- [ ] `pnpm typecheck`；`pnpm build:web`。
- [ ] vitest：git-branch + `store/slices/editor`（diff 相关）+ `right-sidebar/source-control` 全绿（防面板回归 + openBranchAllDiffs 现有调用零回归）。
- [ ] 增量末定向 CodeReview（Task2 接线 + Task1 store 面）。
- [ ] 手工验收（`pnpm dev`）：选 local 与 remote 分支各点 Show Diff → 独立 "Branch Changes (X)" tab、文件列表属于 X 不串当前 base。

## 范围外
- Update/Push/fastForward（O-5、远端并发）→ 增量5。Git Log 宿主 B 的 show-diff 接线（本增量只宿主 A）。compare 缓存/轮询优化不动。
