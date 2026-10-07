# Phase 2 子项目 B.1：通知收尾（原生权限探测 + 精确 dismiss + 内置音效）收尾记录

- 日期：2026-10-07（实施）～ 2026-10-07（终态自动门禁）
- 分支：`phase2b1-notifications-closeout`（基线 `main@df597b7`，Phase 2B 已合入）
- 规格：`docs/superpowers/specs/2026-10-07-phase2b1-notifications-native-closeout-design.md`
- 计划：`docs/superpowers/plans/2026-10-07-phase2b1-notifications-native-closeout.md`（6 任务；Task 6 手工验收清单保留 `待人工执行`，见 §4）
- 方法：subagent-driven-development（每任务 TDD 实现 + 评审 + fix 轮；终态三组门禁 + 本记录）
- 手工验收状态：**待人工执行**（依 spike 结论必须在打包产物上执行；自动化证据见 §3）

## 1. 交付概览

| 项 | 内容 |
|---|---|
| native 命令（4） | `ade-bridge/commands/notifications_native.rs`：`notifications_get_authorization_status` / `notifications_request_authorization` / `notifications_deliver_native` / `notifications_dismiss_native`；macOS 走 objc2 `UNUserNotificationCenter`（`run_on_main_thread` 发起 + `mpsc` 回调，2s 超时），非 macOS 返回 `unsupported` 桩；平台稳定注册（`collect_commands!` + 命令清单测试 + bindings + TS invoke 四处一致） |
| UN 通道 | 授权读口/触发；投递 `requestWithIdentifier:content:trigger:nil`（稳定字符串标识符，`silent` 控制 UN 默认音）；dismiss 先 `getDeliveredNotifications` 求交集、仅删命中、返回真实交集数 |
| bundle identity | `src-tauri/macos-info.plist` 补 `CFBundleIdentifier = dev.itsuka.orcinus` |
| 打包进程门（R3） | `macos::is_bundled_app_process()`：`current_exe()` 含 `.app/Contents/MacOS/` 才进入 UN；否则零 ObjC 调用直接短路（status `available:false` / deliver `ok:false,error:"not-bundled"` / dismiss `{dismissed:0}`）——防 dev 裸二进制硬 abort |
| never-fail 授权（R4） | `notifications_request_authorization` 用户未答致 2s 超时时不再报错：补读一次状态，仍失败回 `{status:'not-determined', available:true}` |
| TS 权限语义 | `getPermissionStatus().requested` 改读持久化 UI 态 `notificationPermissionRequested`（`ui_get`，只读不改）；`probeDelivery` darwin+native 走权威三态（authoritative:true），not-determined 触发一次/session 授权窗并 `ui_set(requested=true)` |
| TS 投递/回退 | `dispatch` native 分支：`authorized`→原生投递；`denied`→`blocked-by-system`；not-determined→fire-and-forget 触发弹窗 + 写 requested + 回 `blocked-by-system`（R4，不等用户）；投递失败→`not-displayed`；`unknown`/探测失败→插件路径 `authoritative:false` |
| TS dismiss | darwin+native 用原始字符串 ids 调 `notifications_dismiss_native` 返回交集计数；插件回退路径维持 2B（FNV 哈希 + `removeActive`，异常 `{dismissed:0}`） |
| 标识符/静音 | `args.notificationId` 优先，缺失生成 `orcinus:<uuid>`（`crypto.randomUUID` 不可用退 `Date.now()` 组合）；`customSoundId==='system'`→`silent:false`（UN 默认音），其余→`silent:true`（renderer 播，杜绝双声） |
| 内置音效 | `resources/notification-sounds/` 9 个 mp3（two-tone/bong/thump/blip/sonar/blop/ding/clack/beep，各 12,717B，git 跟踪，sha256 与 oracle 逐字节一致）；`src/renderer/src/lib/built-in-notification-sounds.ts` Vite `?url` 映射；`playSound` 顺序 = custom base64 通道 → 内置 URL → `system`/未知 `missing-path`；dedupe 键 id、`force` 跳过；音量 0–100 → /100 并 clamp 0..1 |
| 依赖 | macOS-only `objc2 0.6` / `objc2-foundation 0.3` / `objc2-user-notifications 0.3` / `block2 0.6`，全部沿用 Cargo.lock 既有版本；无新增 npm 依赖 |

## 2. 任务与提交

| 任务 | 交付 | 提交 |
|---|---|---|
| T1 CFBundleIdentifier + 授权读口/触发 | plist 键 + 4 依赖边 + 两命令 + `map_status` + bindings（含 dev spike） | 0c7182a |
| T1 fix R3 | 非 .app 进程硬门禁（`is_bundled_app_process`），dev 不 abort | 909ffd2 |
| T1 fix R4 | 授权请求超时 never-fail（读口兜底） | a4dd536 |
| T2 native 投递/dismiss | deliver/dismiss 命令 + 交集计数 + R3 门 + bindings | 97e47ad |
| T3 内置音效 | 9 资产 + `built-in-notification-sounds.ts` + `playSound`/`playAudio` | 85ce9f3 |
| T4 TS 权限语义 | 能力探测 + requested 持久化 + probe 权威三态 + 回退 | 05d2fa5 |
| T5 TS 投递/dismiss 分流 | 稳定标识符/静音规则/blocked·not-displayed 映射/原生 dismiss | 46eebf5 |
| T6 门禁 + 记录 | 三组门禁 + 本记录 + 计划勾选 | 本记录提交 |

## 3. 验收证据（自动化）

三组门禁在终态 HEAD（`46eebf5`，分支 `phase2b1-notifications-closeout`，`git status` 干净）顺序执行：

| 门禁 | 结果 |
|---|---|
| `cargo test --workspace`（`src-tauri/`） | **exit 0**；40 个 suite（含 doc-tests）累计 **681 passed / 0 failed / 0 ignored**（含 4 命令 serde 形状、`map_status` 映射、R3 门谓词、`bindings_are_fresh`、`export_lists_every_command`） |
| `pnpm test`（repo 根） | 首跑 **exit 1**：唯一失败为性能预算用例 `src/renderer/src/lib/browser-history-match.performance.test.ts > prepares a cold corpus within budget`（p95 6.358ms ≥ 预算 4ms，负载敏感）；聚合 **Test Files 3856 passed / 1 failed / 8 skipped (3865)**；**Tests 34339 passed / 1 failed / 122 skipped (34462)**；Duration 764.50s。隔离复跑该文件：**3 passed / 0 failed，exit 0**（319ms）→ 按环境负载 flaky 记档（brief 预告的是同类 palette 性能预算用例，实际命中的是 browser-history-match，判定标准不变） |
| `pnpm typecheck && pnpm build:web` | **exit 0**；`tsc --noEmit` 零错误；`✓ built in 7.09s`；构建日志 `externalized` / `MISSING_EXPORT` / `browser compatibility` **0 命中**；9 个音效 mp3 进入 `dist/assets/`；仅既有 >500kB chunk-size 警告 |

## 4. 手工验收（待人工执行）

前置（spike 结论，Task 1）：`UNUserNotificationCenter` 在裸二进制（`pnpm dev` 形态）硬 abort（`bundleProxyForCurrentProcess is nil`，exit 134），嵌入 plist 亦不可用；**必须用打包产物验收**：

```bash
pnpm tauri build && open src-tauri/target/release/bundle/macos/Orcinus.app
```

（打包后 exe 路径含 `.app/Contents/MacOS/`，打包进程门放行，native 通道启用；`pnpm dev` 恒走插件回退。）

1. **待人工执行**：打包产物首启 → 通知权限卡片读到真实状态（非恒 granted）。
2. **待人工执行**：系统设置改通知权限 → 卡片 2.5s 轮询内状态实时变化（authoritative）。
3. **待人工执行**：首次未决 → probe 触发系统授权弹窗；拒绝后 dispatch → `blocked-by-system` 回退 toast。
4. **待人工执行**：设置页依次试听 9 个内置 + custom：可听、无双声、音量滑杆生效。
5. **待人工执行**：触发 waiting 通知 → dismiss（或 `ui-slice-activity-actions` 路径）→ 系统通知中心横幅被移除、计数正确。
6. **待人工执行**：重启 app（权限已授权）→ 通知仍正常投递、dismiss 仍匹配（标识符稳定）。

任一项失败：补记到本文件偏差节并修复后，重跑对应自动门禁。

## 5. 规格偏差与边界备案

1. **dev 打包门（spike 结论，R3）**：spike 实证裸二进制与「嵌入 `CFBundleIdentifier` plist」两种形态调用 UN 均抛 `NSInternalInconsistencyException: bundleProxyForCurrentProcess is nil` 并 exit 134（`NSBundle` 能读到 `dev.itsuka.orcinus`，但 LaunchServices 不认，不可 catch）。故 4 命令全部前置 `is_bundled_app_process()` 门；dev（`tauri dev`）恒回退插件路径（功能不退化、不崩），native 验收位置上移至 `pnpm tauri build` 产物。非标准 bundle 布局会误判不可用 → 回退插件（可再调）。
2. **`requested` 语义变更**：从 2B 的「插件恒 granted 的假象」改为持久化 UI 态「是否触发过弹窗」（`notificationPermissionRequested`）。`getPermissionStatus` 只读 `ui_get`；仅 `probeDelivery` / `dispatch` 的 not-determined 分支写 `ui_set(requested=true)`；首次安装未触发时卡片显示 awaiting-permission 属预期。启用 native 后不再以插件 `isPermissionGranted` 的恒 true 为准。
3. **native 失败回退**：能力探测每域实例一次（darwin 首调 status，`available:true` 才启用 native），探测失败/`unknown` 置 `nativeAvailable=false` 并 warn，此后全走插件路径且 `authoritative:false`；单次业务失败（投递 `ok:false`→`not-displayed`、dismiss 异常→`{dismissed:0}`）不改变 `nativeAvailable`（权限被拒是业务态不是能力缺失）。native deliver 分支无 try/catch（命令契约 in-band 返回 `{ok:false}`，异常仅能力探测层面处理；与 dismiss 分支不对称，留档 minor）。
4. **R4 dispatch 不等待用户**：not-determined 时 fire-and-forget 触发授权窗 + 写 requested + 直接回 `blocked-by-system`（fork 语义）——该次通知不投递，授权后下一次 dispatch 走原生；`probeDelivery` 在授权前保持 `awaiting-decision`（卡片 2.5s 轮询自然收敛）；Rust 侧授权请求 never-fail（超时补读，仍失败回 not-determined），绝不阻塞 UI。
5. **dismiss 计数=交集**：`getDeliveredNotifications` 结果与请求 ids 精确求交（无前缀/模糊匹配），仅对命中调用 `removeDeliveredNotificationsWithIdentifiers`，返回 `matched.len()`；未命中/无交集 → `{dismissed:0}`（幂等）。TS 只透传计数。
6. **UN 与插件 `NSUserNotificationCenter` 不互删**：两套投递存储互不相通——native dismiss 无法移除插件（旧的 `NSUserNotificationCenter`）此前投递的横幅，反之亦然。native 不可用期间/非 macOS 的旧横幅属上一会话，窗口极小，属计划内一次性迁移裁定（规格 §6.2）。
7. **音量 0–100**：renderer 契约不变（options.volume 0–100），`playAudio` 统一 `/100` 并 `Math.min(1, Math.max(0, …))` clamp 到 Audio.volume 0..1；内置与 custom 两条通道共用，含 NaN/越界行为与 2B 一致。
8. **objc2 版本沿用 Cargo.lock**：`objc2 0.6.4` / `objc2-foundation 0.3.2` / `objc2-user-notifications 0.3.2` / `block2 0.6.2` 均为 lock 既有版本（仅新增 ade-bridge 依赖边与传递件），无新解析；tauri 升级时需复核 feature 兼容。
9. **平台边界**：非 macOS 4 命令恒返回 `unsupported` 桩（`{status:'unknown',available:false}` / `{ok:false,error:'unsupported-platform'}` / `{dismissed:0}`），Win/Linux 行为与 2B 完全一致；非 macOS 桩未做交叉编译验证（minor）。
10. **其他留档 minor**（详见各任务报告与 `.superpowers/sdd/2026-10-07-phase2b1-notifications-native-closeout/progress.md`，git 历史为最终依据）：macOS 分支 4 处 clippy `unneeded return`（brief 原样模式，无 clippy 门禁）；deliver `Ok(false)` 丢弃 NSError 详情；超时路径存在假阴性（回调可能仍成功）；`builtInSoundUrl` 原型键（`'constructor'` 等）返回继承值 → 优雅降级 `playback-failed`；not-determined 下每次 dispatch 都幂等写 `requested`（有意）；`randomNotificationIdentifier` 的 `crypto.randomUUID` 回退分支无测试（安全）；通知点击聚焦（UN delegate）与 action 按钮明确不做（规格 §2.2）。

## 6. 已知边界与后续

- **待办（本任务唯一未闭环项）**：§4 六项手工验收须在打包产物上由人工执行；失败即补记 §5 并修复后重跑三组门禁。
- **后续可选**：UN delegate 通知点击聚焦；`NSUserNotificationCenter` 旧横幅迁移策略（若真实用户命中率不可忽略）；非 macOS 桩的交叉编译 CI 验证；clippy 清理（`unneeded return`）。
- **回滚策略**：移除 TS 对 4 命令的调用即回到 2B 行为；Rust 命令留桩不影响。
