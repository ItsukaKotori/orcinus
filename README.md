# Orcinus

基于 stablyai/orca 二次开发的 AI 编排 IDE（Tauri v2 + Rust，Windows/macOS）。项目原名 ade，Phase 0 收口后更名为 Orcinus。

- 规格：`docs/superpowers/specs/2026-09-14-ade-design.md`
- Phase 0 计划：`docs/superpowers/plans/2026-09-14-ade-phase0-skeleton-ui.md`
- Phase 0 验收记录：`docs/phase0-acceptance.md`
- 参照仓库（只读）：fork 自 stablyai/orca；本机只读检出为 `../orca`（若存在，优先使用）；原参照检出 `../orca-main/orca-main` 迁移后本机不存在，如需对照请用原机检出。

## 开发

```bash
pnpm install
pnpm dev          # Tauri 开发窗口
pnpm typecheck    # TS 类型检查
pnpm test         # Vitest（全量，等价 vitest run）
pnpm build:web    # 仅构建渲染层
cargo test -p orcinus-pty
```
