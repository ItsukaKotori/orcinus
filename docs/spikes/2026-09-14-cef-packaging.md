# CEF 打包 Spike（2026-09-14）

## 目的

验证以 `cef` crate（tauri-apps 维护）内嵌 CEF 的可行性：构建耗时、安装体积、启动、内存、进程隔离，
决定 Phase 3 内嵌浏览器（Design Mode/CDP 对等）走 CEF，还是维持系统 WebView 降级方案
（规格 §4 决策 3 / §10 风险 1）。

## 环境

| 项 | 值 |
| --- | --- |
| OS | macOS 26.6.2（Build 25G83）arm64（Apple Silicon，8 核，16GB） |
| Rust | rustc 1.95.0 / cargo 1.95.0（Homebrew） |
| 构建前置 | Xcode CLT（Apple clang 21.0.0）+ CMake 4.4.3 + Ninja（Homebrew 安装；`cef-dll-sys` 硬编码 Ninja generator，缺一不可） |
| cef crate | `cef = "=152.2.0"`（Cargo.lock 锁定 `cef` 与 `cef-dll-sys` 均为 152.2.0+152.0.6；default-features 关闭，仅启用 `build-util`） |
| CEF 二进制 | `cef_binary_152.0.6+g708dc14+chromium-152.0.7977.83_macosarm64_minimal.tar.bz2`（125 MB 下载，解包发行目录 343 MB） |
| 工程 | `spikes/cef-embed/`（独立 Cargo 工程，**不加入** `src-tauri` workspace，release 构建） |
| 运行配置 | `no_sandbox: 1`（未启用 CEF sandbox；未签名/未公证） |
| Windows | **未验证（需在 Windows 机器复测）** |

> 原计划在本机 Windows 上测、macOS 记未验证；迁移后本机为 macOS（见 SDD ledger R31），方向相反：macOS 为实测平台，Windows 未验证。

## 测量方法

工程内容（README 用法落地，非凭记忆）：

- `src/main.rs`：`Args` → `execute_process` → `initialize(Settings{no_sandbox:1})` → `run_message_loop`；
  `wrap_app!`/`wrap_browser_process_handler!` 在 `on_context_initialized` 用 Views 创建窗口
  （`browser_view_create` + `window_create_top_level`），加载
  `data:text/html,<h1>ade cef spike</h1>`（percent-encoded，含 `<title>ade cef spike</title>`）。
- `src/macos_application.rs`：按 cefsimple 示例实现 `NSApplication` 子类（`sendEvent:` + `CefAppProtocol`），
  这是 macOS 上 CEF 事件处理的前置条件。
- `src/bin/cef_embed_helper.rs`：helper 进程入口（`LibraryLoader::new(.., helper=true)` + `execute_process`）。
- `src/bin/bundle_release.rs`：`bundle-cef-app` 工具只支持 debug 构建（`build_bundle` 硬编码 `target/debug`），
  故直接调用 `cef::build_util::mac::bundle()`，把 `target/release` 的二进制打成 `.app`（含 5 个 helper app 与 framework）。
- `build.rs`：占位（macOS 无需资源编译；README 示例的 build.rs 仅 Windows 图标资源用）。

复现命令（工作目录 `ade/`）：

```bash
# 冷构建（空 target；含 CEF 下载/解包 + libcef_dll_wrapper 编译 + 全部依赖）
CARGO_TARGET_DIR=/tmp/cef-cold-target cargo build --release --manifest-path spikes/cef-embed/Cargo.toml

# 打 .app（release 产物）
spikes/cef-embed/target/release/bundle_release

# 运行（README 推荐方式；也可直接执行 .app 内二进制并收集 stdout 日志）
open spikes/cef-embed/target/bundle/cef-embed.app
spikes/cef-embed/target/bundle/cef-embed.app/Contents/MacOS/cef-embed

# 体积
du -sh spikes/cef-embed/target/bundle/cef-embed.app
find <bundle> -type f -exec stat -f "%z" {} + | awk '{s+=$1} END {print s}'

# 内存 / 进程隔离
ps -o rss= -p <browser pid>
ps -axo pid,ppid,rss,comm,args | grep -i "cef-embed"
```

启动耗时 = 从 `Popen` 到日志出现 `[cef-spike] browser process started`（CEF 初始化完成）与
`[cef-spike] title changed: ade cef spike`（页面解析出标题）的墙钟差。

## 实测结果

### 构建耗时

| 场景 | 耗时 |
| --- | --- |
| 冷构建（临时空 target，含 125 MB CEF 下载 + wrapper + 依赖） | **80 s**（cargo 自报 `Finished release in 1m 19s`，计时器一致） |
| 首次构建（真实 target，到本工程编译阶段；该次暴露上游宏缺陷而报错） | 97 s（其后修复本工程源码） |
| 增量构建（仅本 crate） | 2–6 s |

冷构建两次产物字节数一致（`cef-embed` 542,928 B；`cef_embed_helper` 468,640 B），可复现。

### 安装体积（必须分发的 `.app`）

| 组成 | 大小 | 说明 |
| --- | --- | --- |
| **bundle 合计** | **334,487,951 B = 334.5 MB（十进制）= 319.0 MiB**（`du` 分配 327,464 KiB） | 对照阈值 <300 MB → **超阈值** |
| `Chromium Embedded Framework.framework` | 317 MB | |
| ├ 主二进制 | 219 MB | 未裁剪 |
| ├ `Resources/` | 82 MB | `icudtl.dat` 10 MB、`resources.pak` 18 MB、220 个多语言 `.lproj` 共 51.5 MB（en 仅 0.6 MB，其余约 50 MB 有裁剪空间，本 spike 未做） |
| └ `Libraries/` | 16 MB | |
| 5 个 helper `.app`（GPU/Renderer/Plugin/Alerts/无后缀） | 464 KB × 5 | 均为 `cef_embed_helper` 二进制的副本 |
| 主二进制 `cef-embed` | 530 KB | |

参考：CEF 下载包 125 MB；解包发行目录 343 MB（含头文件/CMake/CREDITS，不随应用分发）；开发 `target/` 构建树 1.4 GB。

### 启动耗时（两次独立运行，直接执行 bundle 内二进制）

| 运行 | CEF 初始化完成 | 页面标题回调 |
| --- | --- | --- |
| 1 | 0.271 s | 0.379 s |
| 2 | 0.270 s | 0.376 s |

`open cef-embed.app`（README 路径）同样可启动并保持存活。

### 内存与进程隔离（页面加载后约 10 s，`ps` RSS）

| 进程 | RSS |
| --- | --- |
| browser（主进程） | 178–182 MB |
| 全部 7 个进程合计 | 774–787 MiB（两次运行） |

进程树：1 × browser + 1 × `--type=gpu-process` + 2 × `--type=utility` + 3 × `--type=renderer`
（单页加载实测为 3 个 renderer，含备用 renderer），全部 helper 的 PPID 指向主进程；`kill` 主进程后全部退出。

### 运行证据

- 日志：`[cef-spike] browser process started`、`[cef-spike] title changed: ade cef spike`（两次）。
- 截图：`screencapture -x /tmp/cef-spike.png` 成功（2880×1864 RGBA PNG，1.3 MB），已查看：
  窗口标题栏与页面 `<h1>` 均显示 `ade cef spike`。截图仅作本机运行证据，未入库。
- 反例：直接运行 `target/release/cef-embed`（不带 bundle）panics（`library_loader.rs` framework 相对路径不存在，exit 101）
  → **macOS 上必须 .app bundle**（README bundle 指引已验证可行）。

## 问题清单

1. **签名/公证未测**：`codesign`/`notarytool`/Gatekeeper 分发路径均未执行。
2. **运行使用 `no_sandbox: 1`**：未启用 CEF sandbox feature（helper 无 entitlements/签名）；生产配置需签名后复测 sandbox 行为。
3. **Windows 未验证**：构建、体积、运行均需在 Windows 机器复测（本机为 macOS）。
4. **上游宏缺陷**：`cef` 152.2.0 的 `wrap_browser_view_delegate!` 零字段分支无法编译（递归展开时漏传 `impl ViewDelegate`）；
   本 spike 用带字段形式绕过。本 spike 实测可用的零字段形式：`wrap_app!`/`wrap_client!`/`wrap_display_handler!`。
5. **release 打包需自建**：`bundle-cef-app` 只打包 debug（`build_bundle` 硬编码 `target/debug`）；本 spike 通过自建 `bundle_release` bin 调 `bundle()` API 打包 release。
6. **构建前置依赖**：需 Homebrew 安装 `cmake` + `ninja`（macOS 不自带）。
7. **CEF 警告**：未设置 `root_cache_path` 会提示进程单例风险（spike 未处理）。
8. 未测：CDP/Design Mode、cookie 导入、网络隧道、多窗口、崩溃隔离语义、压缩分发体积（DMG/zip）、自动更新。

## 结论与 go/no-go

- 条件核对：构建耗时 80 s（可接受）；进程隔离成立（GPU/renderer/utility 独立子进程）；
  **安装体积 334.5 MB > 300 MB 阈值**（按 MiB 读作 319 MiB 亦超）。
- 按计划阈值判定：**CEF 方案 no-go（Phase 3）**，维持系统 WebView 降级方案
  （注入式元素拾取；Design Mode 仅 HTML/CSS，无录像/完整 cookie 能力；UI 标注能力差异）。
- 备注：超阈值幅度约 +11.5%，仅裁剪非英文 locale（220 个 `.lproj` 约 50 MB）即可能压到 ~285 MB，但属未验证的裁剪配置，
  且不能抵消签名/公证、Windows 双平台、构建链维护等未验证项；若后续愿意接受离线分发体积并完成裁剪+签名验证，可重开 go 决策。

### 规格风险 10.1 回写建议（docs/superpowers/specs/2026-09-14-ade-design.md）

> 1. **CEF 集成与打包**（体积、签名、公证、崩溃隔离）→ Phase 0 spike 结论（2026-09-14，macOS arm64）：
>    安装体积 334.5 MB（> 300 MB 阈值），冷构建 80 s，进程隔离成立（browser/GPU/2×utility/3×renderer）；
>    签名/公证与 Windows 未验证。按阈值判定 **no-go**，Phase 3 采用系统 WebView 降级方案（§4 决策 3）；
>    CEF 保留为后续候选，重开条件：分发体积裁剪 + 签名/公证 + Windows 复测。
