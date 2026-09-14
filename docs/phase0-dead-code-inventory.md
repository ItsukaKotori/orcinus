# Phase 0 死代码清单（Task 10）

- 分支：`phase0-skeleton-ui`；Task 10 基线：`6245317`（Task 9 提交）
- 日期：2026-09-14（macOS）
- 时间盒：21:33 开始，21:50 停止删除（实际删除阶段 17 分钟，低于 2 小时上限）；随后完成验证、本文档与提交
- 范围：仅 G（远程与多端）/ I（Orca CLI、skills 共享、ephemeral VM、多账号与用量）在前序 Task 已移除入口后遗留的代码；不做全量死代码清扫

## 方法与证据链

1. Step 1 按 brief 原命令统计 8 个入口词在 `src/renderer/src` + `src/shared` 的引用文件数（清扫前/后）。
2. 对 `src/renderer/src`、`src/shared` 全量文件做**生产可达性分析**：入口 = `src/renderer/index.html → main.tsx`、`popout.html → popout.tsx`、`web-index.html → web/main.tsx`，以及 `src/main`、`src/preload`、`src/bridge` 全部文件；解析相对导入与 `@/`、`@renderer/` 别名（含 lazy `import()` 字面量）。作为根的文件之外的模块若无生产引用，只在“仅剩测试或自身引用”时才删除。
3. 删除采用 `git rm`，每批（≤20 文件）后运行 `pnpm typecheck && pnpm build:web`；全部批次退出码 0。
4. 未删除任何 `src/preload/api*` 契约文件；未动保留功能（终端、worktree/composer、插件中心、设置等）。

## Step 1：引用计数

| 项 | 清扫前 | 清扫后 | 残留内容 | 判定 |
|---|---|---|---|---|
| `SshPassphraseDialog` | 7 | 7 | 6 个 locale + `app-startup-routing.test.ts` 的反回归断言（断言入口不得回潮） | Phase 0 已移除入口（组件已不存在；残留为 i18n 键）→ Phase 1 清键 |
| `RemoteServerUpdateDialog` | 9 | 9 | 6 locale + `en-runtime-required.json` + `store/slices/remote-server-updates.ts` + 其 integration test | Phase 0 已移除入口；slice 仍被 store 注册、runtime polling/coordinator 仍在启动链路 → Phase 1（见下） |
| `SkillsPage` | 7 | 7 | 6 locale + `en-runtime-required.json`（无代码模块） | Phase 0 已移除入口 → Phase 1 清 i18n 键 |
| `MobilePage` | 7 | 7 | 6 locale + `en-runtime-required.json`（无代码模块） | Phase 0 已移除入口 → Phase 1 清 i18n 键 |
| `EphemeralVm` | 76 | 63 | 已删 shared 独占 10 模块 + 7 测试；其余 63 文件为 live 链路（renderer composer run-target、worktree creation、runtime cleanup、store teardown） | Phase 0 已移除入口；残留为 Phase 1 清理代码（须待 composer/运行目标决策） |
| `skill-share` | 0 | 0 | 无内容引用；独占模块 `src/shared/skill-share-link.ts` 无任何引用 | 已删除（Phase 0 清理完成） |
| `cli-install` | 23 | 23 | 全部为 live CLI 安装路径（`cli-install-types.ts`、Linear skill setup、onboarding、BrowserUseCliStep、feature-tip 等） | 保留功能 → Phase 1 在 CLI 面统一复核后清理 |
| `orca-profile` | 20 | 20 | 多账号链仍被 store/preload/web/unexpected-signout/browser partition 消费 | Phase 0 仅删死组件；整链 Phase 1 |

## 清扫结果

- **删除文件：80**（其中生产模块 51、随模块孤立的测试 29）+ 修改 1 个 ratchet 夹具（删除 2 行已删文件的条目）。`src/preload/api*` 契约文件 0 删除。
- 删除批次与门禁（每批后 `pnpm typecheck && pnpm build:web`，均 exit 0）：

| 批次 | 文件数 | 内容 |
|---|---|---|
| A | 17 | `src/shared/ephemeral-vm-*` 独占子图：recipe-checkout-mode / destroy-result / doctor / lifecycle-payload / process / repo-url / runner / runtime-feature-store / runtime-rollback-projection / runtime-store（10 模块）+ 7 测试；并移除 child-process allowlist 中对应 2 行 |
| B | 17 | `src/shared/mobile-e2ee-*`（legacy-fixtures / v2-contract / v2-fixtures / v2-framing，3 测试）、`mobile-file-directory-limit`（+测试）、`mobile-relay-close-codes`（+测试）、`mobile-relay-phone-protocol`（+测试）、`windows-mobile-firewall`、`skill-share-link`、`cli-app-status-projection`、`OrcaProfileSignOutConfirmDialog`（+测试） |
| C | 15 | AddRemoteHost 死闭包（Task 4 R19 留下的底层模块）：`AddRemoteHostDialog/Fields/ServerFormPanel/SshConfigPicker/SshFormPanel`、`add-remote-host-ssh-actions`、`SshHostAdvancedFields`、`ssh-target-draft`、`ssh-target-duplicate`、`shared/ssh-config-alias` + 5 测试 |
| D1 | 15 | `skill-ssh-relay-contract`、`ssh-ai-vault-relay`、`relay-retry-after-header`、`relay-host-close-reason`、`skill-install-providers`、`skill-discovery-depth`、`wsl-browser-network-relay-contract`、`relay-frame-decoder(-contract)`、`relay-frame-buffer`(+test)、`relay-version-marker`(+test)、`pairing-address-auto-selection`(+test) |
| D2 | 16 | `ssh-relay-pty-ownership-proof`(+test)、`skill-deletion-eligibility`(+test)、`skill-bundle-name`(+test)、`skill-path-containment`(+test)、`skill-metadata`(+test)、`agent-skill-sharing-gate`(+test)、`cli-workspace-provenance`(+test)、`cli-argument-boundary`(+test) |

- 完整删除清单：`git show --stat <Task 10 提交>`（本文档与代码同一次提交）。

## 保留项（有引用，未删）与原因

| 保留项 | 引用证据 | 处置 |
|---|---|---|
| `MobileEmulatorSettingsPane` 及 `emulator-pane` 主体 | 由 `settings-interface-primary-section-renderers.tsx` 渲染，生产可达 | **维持 Task 4 判定**：浏览器仿真能力，非移动端配对；Phase 0 保留 |
| `src/shared/system-cli-install-dirs.ts` | 被 `node-cli-command-resolution.ts`、`posix-version-manager-bin-dirs.ts` 引用（二者自身只在死链中，非 G/I 命名） | Phase 1 随 CLI 死链整簇删除 |
| `src/shared/mobile-push-contract.ts` | 被 `rpc-contract/notifications-params.ts` 引用（rpc-contract 整目录为死代码但非 G/I 独占） | Phase 1 随 rpc-contract 死树删除 |
| `src/shared/mobile-relay-credential-contract.ts` | 被 `rpc-contract/rpc-params-catalog.generated.ts` 引用 | 同上 |
| `src/shared/e2ee-crypto.ts`、`src/shared/remote-runtime-*`、`src/renderer/src/components/terminal-pane/remote-runtime-pty-transport.ts` 等 | 被 live 终端/web 代码与 live 测试（含 `remote-runtime-shared-control-test-server` 测试支持）引用；live `remote-runtime-terminal-multiplexer-base.ts` 依赖 `remote-runtime-client-error-classification.ts` | Phase 1：先重构测试支持/调用点再清 |
| `mobile-relay-pairing-fixtures.ts`、`mobile-markdown-bridge-test-harness.ts` | live 模块的测试支持（`pairing.ts` 链、`mobile-markdown-bridge.ts`） | 保留（随 live 模块） |
| `store/slices/remote-server-updates.ts`、`store/slices/orca-profiles*.ts` | store index/types 注册、启动链路与 live 消费点 | Phase 1 切片清理 |
| i18n 键：`SkillsPage` / `MobilePage` / `SshPassphraseDialog` / `RemoteServerUpdateDialog`（6 locale + `en-runtime-required.json`） | 仅 locale/守卫断言引用 | Phase 1 清键（需同步 i18n 回归测试） |
| `app-startup-routing.test.ts` 的 SSH 反回归断言 | 断言入口不得回潮 | 保留 |
| `src/renderer/src/app-shell/workspace-view-cross-client-sync.test.tsx` | Task 2 记录：读取未 fork 的 `mobile/` 源码 ENOENT，已在 `vitest.config.ts` exclude | 保留并记录；Phase 1 决定重指向或删除（不要静默保留） |

## 其他已复核查证（未动）

- `settings.showMobileButton`：仍有生产读取方（`AppearanceWindowSidebarSection.tsx:246`），不是死设置；Phase 1 随移动按钮面统一移除。
- `setUsagePercentageDisplay`：仅 slice 定义与契约，无生产调用者（文件内死代码）→ Phase 1。
- stale copy「Settings → Remote Orca Servers」：宿主 `AddRemoteHostFields.tsx` 已随本 Task 删除；余下命中均在 live 代码的测试断言中 → Phase 1 文案统一。
- `MobilePage`/`SkillsPage` 等页面组件在前序 Task 已不存在（本次计数只剩 i18n）。

## 验证命令与结果

| 命令 | 结果 |
|---|---|
| 每批后 `pnpm typecheck` | exit 0（全部批次；末次 21:5x） |
| 每批后 `pnpm build:web` | exit 0（末次 `✓ built in 3.20s`） |
| `pnpm test src/shared/child-process/child-process-import-boundary.test.ts` | 2 failed / 2 passed：**预先存在**（旧 fork 遗留 146 条 stale allowlist；本 Task 删除其中 2 条后为 144 条；offender 9→7，pin 155 未动）。非本 Task 引入，未加避让 |
| `pnpm test src/shared/agent-cli-install-dir-fallback.test.ts` 等 4 个邻近/保留链路测试 | 通过（58 passed） |
| `pnpm test src/renderer/src/i18n/ko-ui-semantic-mistranslations.test.ts` | 1 failed：**预先存在**（`config/scripts/locale-ko-key-overrides.json` 缺失，Task 4 minor 已记录） |

## Phase 1 建议（按收益/风险排序）

1. **rpc-contract 死树**：`src/shared/rpc-contract/` 全目录生产不可达；删除后可连带清理 `mobile-push-contract.ts`、`mobile-relay-credential-contract.ts`、`agent-skill-sharing-contract.ts`、`skill-upload-session-contract.ts`。
2. **remote-runtime/relay 死树**：多数 `remote-runtime-*`/`relay-*` 已生产不可达，但被 `remote-runtime-shared-control-test-server.ts`（live 测试的测试支持）与 live 终端/web 代码牵住；先重构测试支持与调用点，再整簇删除。
3. **CLI 死链**：`node-cli-command-resolution.ts` → `system-cli-install-dirs.ts`/`posix-version-manager-bin-dirs.ts` → `local-agent-install-dir-detection.ts` 整簇删除（含 `nvm-default-alias.test.ts` 等孤立测试）。
4. **ratchet 重基线**：`child-process-import-allowlist.txt` 现有 144 条 stale 条目；删除 stale 行并把 `DIRECT_IMPORTER_PIN` 降到 7（当前 offender 数）。
5. **i18n 清键**：`SkillsPage`/`MobilePage`/`SshPassphraseDialog`/`RemoteServerUpdateDialog` 四组键（6 locale + en-runtime-required），同步更新 i18n 回归测试。
6. **store 切片**：`remote-server-updates`、`orca-profiles(-auth-actions)` 的用户面已移除；确认启动链路不需要后删除并清 store 注册。
7. **composer 运行目标**：`EphemeralVm` 运行目标仍可达（RunTargetCombobox 等）；按 spec §6.5 决定是否随 VM 能力一并移除，其独占的 renderer `ephemeral-vm-*` 库与 shared `ephemeral-vm-recipes/runtimes` 方可删除。
8. **保留文件内死代码**：`setUsagePercentageDisplay`、`settings.showMobileButton`、stale copy、`MobileEmulatorSettingsPane` 的移动端语义复核。
9. **Task 2 excluded test**：`workspace-view-cross-client-sync.test.tsx` 重指向或删除，去掉 vitest exclude。
