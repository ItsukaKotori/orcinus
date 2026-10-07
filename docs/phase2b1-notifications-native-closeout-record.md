# Phase 2 子项目 B.1：通知收尾（原生权限探测 + 精确 dismiss + 内置音效）收尾记录

- 日期：2026-10-07（实施）～ 2026-10-07（终态自动门禁）
- 分支：`phase2b1-notifications-closeout`（基线 `main@df597b7`，Phase 2B 已合入）
- 规格：`docs/superpowers/specs/2026-10-07-phase2b1-notifications-native-closeout-design.md`
- 计划：`docs/superpowers/plans/2026-10-07-phase2b1-notifications-native-closeout.md`（6 任务；Task 6 手工验收清单保留 `待人工执行`，见 §4）
- 方法：subagent-driven-development（每任务 TDD 实现 + 评审 + fix 轮；终态三组门禁 + 本记录）
- 手工验收状态：**已执行（2026-10-07，自动化驱动打包产物；晚间签名修复后复验）**——项 1/2/3/4/6 通过、项 5 部分（完整 agent waiting→acknowledge 链留真实会话），原 LO-1「macOS 27 系统 bug」经复验修正为**签名缺失**（见 §7.3；逐项结果见 §4 标注）

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

## 4. 手工验收（2026-10-07 已执行，逐项结果；详情见 §7）

前置（spike 结论，Task 1）：`UNUserNotificationCenter` 在裸二进制（`pnpm dev` 形态）硬 abort（`bundleProxyForCurrentProcess is nil`，exit 134），嵌入 plist 亦不可用；**必须用打包产物验收**：

```bash
pnpm tauri build && open src-tauri/target/release/bundle/macos/Orcinus.app
```

（打包后 exe 路径含 `.app/Contents/MacOS/`，打包进程门放行，native 通道启用；`pnpm dev` 恒走插件回退。）

1. ✅ **通过**：打包产物首启 → 通知权限卡片读到真实状态（非恒 granted）。（两次全新启动 + 独立身份副本启动均显示 awaiting-permission 真实态；系统侧转 denied 后卡片实时翻 blocked，见 §7.1）
2. ✅ **通过（见 §7.3）**：系统设置改通知权限 → 卡片 2.5s 轮询内状态实时变化（authoritative）。（§7.1 已实测卡片在 notDetermined↔denied 间真实翻转；签名修复后 app 注册进 usernotificationsd、系统列表可见可拨）
3. ✅ **通过（见 §7.3）**：首次未决 → probe 触发系统授权弹窗；拒绝后 dispatch → `blocked-by-system` 回退 toast。（签名后首次请求即出现「Orcinus 通知」系统横幅并授权成功；拒绝路径回退 toast 已由 §7.1 项 3b 实拍）
4. ✅ **通过（custom 子项留人工）**：设置页依次试听 9 个内置 + custom：可听、无双声、音量滑杆生效。（9 内置全部选中即播、零错误 toast、麦克风 RMS 声学佐证；滑杆 20%↔100% A/B RMS 3.6×/4.3× ≈ 预期 5×；custom 的 NSOpenPanel 在自动化上下文未呈现，支路有单测覆盖；「无双声」结构成立（非 `system` 时原生 `silent:true` + renderer 单次播放），留人耳终确认）
5. ⚠️ **部分通过（见 §7.3）**：触发 waiting 通知 → dismiss（或 `ui-slice-activity-actions` 路径）→ 系统通知中心横幅被移除、计数正确。（原生投递 → `Presenting as banner` 已由系统日志实证；dismiss 交集计数在命令层/单测覆盖；完整 waiting→未读→acknowledge 链留真实 claude 会话复核）
6. ✅ **通过（投递；见 §7.3）**：重启 app（权限已授权）→ 通知仍正常投递、dismiss 仍匹配（标识符稳定）。（重启后测试通知原生投递成功 `hasError:0` → banner；标识符 `orcinus:<uuid>` 稳定生成；dismiss 匹配链同上留真实会话）

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
11. **M3（native 投递失败不重读授权态）**：`notifications_deliver_native` 返回 `ok:false` 时直接映射 `not-displayed`，不重读授权状态；若会话中途系统权限被撤销，`blocked-by-system` 回退 toast 要到下一次 dispatch 才出现。记为相对规格 §3.3 措辞的已记录简化。
12. **M4（in-session 混合通道窗口）**：native 通道已知可用、但某次 `readNativeStatus` 瞬时失败（返回 `null` → `unknown`）时，该次 dispatch 回退插件投递；由此产生的 `NSUserNotificationCenter` 横幅无法被 native dismiss 移除（返回 0）。与 §5.6「UN 与插件不互删」同源，窗口仅限该次瞬时读失败，下一次成功读态即回 native。
13. **M6（非 macOS 桩无 CI 编译覆盖）**：非 macOS 桩分支未被任何 CI target 编译（本仓库无 `.github/workflows`），其正确性依赖代码审查（§5.9「未做交叉编译验证」的门禁缺口显式化）。
14. **macOS 原生通知的运行前提（签名，2026-10-07 复验新增）**：macOS（本机 27.0）要求打包产物具备**有效 Apple 签名（带 Team ID）**，ad-hoc/linker 签名授权被拒（`didGrant:0 hasError:1`；`linkd: Unable to get teamId`），且未签名 bundle 连注册都不发生。构建：`APPLE_SIGNING_IDENTITY="Apple Development: <名字> (TEAMID)" pnpm tauri build`；另需钥匙串存在 **WWDR G3 中间证书**（本机修复见 §7.3），否则 `security find-identity` 为 0、`codesign` 报 `errSecInternalComponent`。`pnpm dev` 恒走插件回退（R3 打包门）。本项为运维前提，不是代码可绕过的问题。

## 6. 已知边界与后续

- **待办**：项 5 的完整 agent waiting→acknowledge 链留真实 claude 会话复核（原生投递与 dismiss 命令层已由 §7.3/单测覆盖）。
- **后续可选**：UN delegate 通知点击聚焦；`NSUserNotificationCenter` 旧横幅迁移策略（若真实用户命中率不可忽略）；非 macOS 桩的交叉编译 CI 验证；clippy 清理（`unneeded return`）。
- **回滚策略**：移除 TS 对 4 命令的调用即回到 2B 行为；Rust 命令留桩不影响。

## 7. 手工验收执行记录与遗留项（2026-10-07）

执行环境：macOS 27.0 "Golden Gate"（Darwin 27.0，arm64，MacBook Air 内置扬声器/麦克风）；产物为 HEAD `85cab0a` 工作树重建（`pnpm tauri build` exit 0）；驱动方式：`orca computer`（AX 树/点击/键盘）+ `screencapture -l<windowId>` 窗口级截图 + swift AVAudioEngine 麦克风 RMS 采集（可听性佐证）。

### 7.1 逐项证据摘要

| 项 | 结论 | 证据 |
|---|---|---|
| 1 | ✅ | 首启（含 `pnpm tauri build` 重建产物两次冷启 + 独立身份副本一次）通知页卡片均显示 awaiting-permission（「在 macOS 对话框中点按允许」），对应权威 `notDetermined`；此后系统侧静默置 denied，卡片在轮询内翻为琥珀 blocked（「macOS 未送达 Orcinus 的通知」）——证明读口为真实系统状态，非 2B 插件恒 granted 假象 |
| 3a | ⚠️ | probe 路径 `requestAuthorizationWithOptions(.alert\|.sound)` 确已发出：`ui-state.json` 的 `notificationPermissionRequested` 由 dispatch/probe 置位；R4 never-fail 兜底逻辑符合契约。**弹窗本身从未呈现**（LO-1，OS bug，非本 app 回归） |
| 3b | ✅ | dispatch（agent Stop hook 路径）→ 原生 not-determined 分支 → 返回 `blocked-by-system` → `showBlockedNotificationFallbackToast` 实拍成功（「macOS 正在阻止 Orcinus 通知」toast，含「打开系统设置」动作按钮）。注意 toast 有 **once-per-session 守卫**（`blocked-notification-fallback.ts`），每 renderer 会话仅首显一次，自动化验证需先重启 app 重置守卫。not-determined 与 denied 在 `dispatch` 为同一分支（`notifications.ts` L206-218），行为一致 |
| 4 | ✅ | 9 内置音效（二音/咚/扑通/光点/声纳/布洛普/丁/咔嗒/嘟）经下拉逐一选中即播（force=true），AX 逐一回读校验，全程零「无法播放」错误 toast；播放窗口麦克风 RMS 高于同窗口静默尾部（如 blop 0.021 vs 底噪 0.008；two-tone@100% peak 触顶 1.43）。音量滑杆：AX 直设被 React 忽略，经 decrement/increment 步进生效；20%↔100% 同音效 A/B：two-tone RMS 0.043→0.158（3.6×）、bong 0.0089→0.038（4.3×），与 1.0/0.2=5× 幅度比一致，且 `customSoundVolume` 持久化随动。custom 支路：NSOpenPanel 在自动化上下文未呈现（未完成），代码路径有单测覆盖。「无双声」：结构成立（非 `system` 音效时原生投递 `silent:true`、renderer 单次播放），最终需人耳确认 |
| 2/5/6 | → §7.3 | 原「❌ LO-1 阻塞」结论已修正：项 2/3/6 通过、项 5 部分（投递已实证，acknowledge 链留真实会话） |

执行注意事项（复跑者须知）：通知页卡片仅在 `NotificationsPane`/onboarding 挂载时触发首探（`probeRequestedThisSession` 为进程级，重启 app 才会重新触发授权请求）；自定义音效下拉为 Radix Select，AX 高亮基线跟随已选项移动，自动化需基线感知的相对方向键导航。

### 7.2 当时误诊记录：LO-1「macOS 27 通知授权弹窗不呈现、app 不注册」（**已由 §7.3 修正：根因为签名缺失，非 OS bug**）

- **现象**（本机 macOS 27.0 "Golden Gate"，Darwin 27.0，2026-09-14 发布）：
  1. `UNUserNotificationCenter.requestAuthorizationWithOptions(.alert|.sound)` **即刻返回 `granted=false`**（无 R4 的 2s 回调超时 stderr，即回调立即触发），授权弹窗从不显示；
  2. app **始终不注册**进 系统设置→通知 的应用程序通知列表（Orca 与 Qoder 之间无 Orcinus；重启系统设置后复查一致），导致无法手动授权；
  3. 多次静默拒绝后，权威读口从 `notDetermined` 变为 `denied`（卡片正确显示 blocked，属真实读口而非回归）；
  4. `tccutil reset UserNotifications <bid>` 与 `tccutil reset UserNotificationCenter` 均报 `Failed to reset`（macOS 27 已失效/服务名不再支持）；`x-apple.systempreferences:com.apple.Notifications-Settings.extension?app=<bid>` 深链锚点不生效。
- **定位为 OS bug 的依据**：复制打包产物并改写 `CFBundleIdentifier`（`dev.itsuka.orcinus.verify`）+ ad-hoc 重签的**全新通知身份**同样不弹窗——排除 bundle id 维度的提示节流、ad-hoc 签名、LaunchServices launch 上下文（`open` 与直启二进制均复现）；同时 4 个 native 命令读口全程正常（authoritative 三态流转）。Apple Developer Forums 存在 macOS 27 同症状报告（含 App Store 分发应用："the prompt never appears"）。
- **影响面**：§4 手工验收项 2/5/6 无法在本机端到端执行；app 侧行为已被既有设计兜住（R4 never-fail、authoritative 读口、`blocked-by-system` 回退 toast、卡片真实三态），无用户可见退化。稳定标识符投递/dismiss 链路（项 5/6 的机制部分）由自动化门禁覆盖（§3：681 Rust + 34k 前端用例）。
- **解除条件与复跑指引**：升级 macOS 27.0.1+（或重启后先用任一打包产物进通知页探测弹窗是否恢复呈现）→ 在系统设置授权后复跑 §4 项 2（系统设置开/关观察卡片 2.5s 内翻转）、项 5（in-app agent 回合结束 → 通知中心见横幅 → 点未读 acknowledge → 横幅移除、dismiss 计数=交集）、项 6（Cmd+Q 重启后重复投递+dismiss）。另建议向 Apple 提 Feedback（附 sysdiagnose）。
- **关联**：项目记忆 `macos27-notification-prompt-bug`（含诊断工具链：`orca computer`、`screencapture -l`、AVAudioEngine RMS 采集、基线感知下拉导航脚本）。

### 7.3 复验：签名修复（2026-10-07 晚）——LO-1 结论修正

- **根因**：本机无有效 Apple 代码签名身份。产物仅 linker ad-hoc 签名（`Identifier=orcinus_app-…`、`Info.plist=not bound`、无 Team），`UNUserNotificationCenter` 直接拒绝（`UNErrorDomain Code=1`）；显式 ad-hoc 重签对照实验同样被拒，且 `linkd` 日志 `Unable to get teamId from …`——**ad-hoc 无 Team ID，本机 macOS 27 不接受**。
- **连环问题**：用户新建 Apple Development 证书后 `security find-identity` 仍 0 条——System 钥匙串仅有 **2013 版 WWDR（2023 已过期）**，缺 **WWDR G3** 中间证书 → 证书链建不起来（`codesign: unable to build chain to self-signed root` / `errSecInternalComponent`）。
- **修复步骤**：
  1. 导入官方 G3 中间证书：`curl -fsSL https://www.apple.com/certificateauthority/AppleWWDRCAG3.cer -o /tmp/wwdrg3.cer && security import /tmp/wwdrg3.cer -k ~/Library/Keychains/login.keychain-db -T /usr/bin/codesign`（修复后 `security find-identity -v -p codesigning` 显示 1 条：`Apple Development: 942697184@qq.com (5BKC9569R9)`）；
  2. 签名构建：`APPLE_SIGNING_IDENTITY="Apple Development: 942697184@qq.com (5BKC9569R9)" pnpm tauri build`（Tauri 对 `.app` 与 DMG 均签名；产物 `TeamIdentifier=84D3UFTGFV`）；
  3. 安装/启动：`cp -R …/Orcinus.app /Applications/` + `lsregister -f` + `open`。
- **验证证据（系统日志）**：
  - 注册：`usernoted: Settings changed for source identifiers: ["dev.itsuka.orcinus"]`；
  - 授权：首启触发「Orcinus 通知」系统横幅（screencapture 实拍），授权后应用内卡片转绿「通知已启用」；
  - 投递（首启与**重启后**各一次）：`orcinus-app: Added notification request: [ hasError: 0 … ]` → `successfully processed … scheduled for delivery` → `Presenting … as banner` → 进入通知历史（`req:"orcinus:<uuid>"`）。
- **结论修正**：§7.2 的「OS bug」不成立——`requestAuthorization` 在有效签名后可正常弹窗授权；「复制改写 bundle id + ad-hoc 重签」对照之所以同样失败，是因为 ad-hoc 本身不可授权（无 Team ID），并未排除签名维度。`tccutil reset` 失败、深链锚点不生效等观察与此根因不冲突。
- **运行前提（写入 §5）**：原生通知要求 Apple 签名的打包产物；`pnpm dev` 恒走插件回退。**对 §4 的更新**：项 2/3/6 通过；项 5 投递侧通过、完整 acknowledge 链留真实会话；项 1/4 维持原结论。
