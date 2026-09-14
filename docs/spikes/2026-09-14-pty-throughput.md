# PTY 吞吐 Spike（2026-09-14）

## 目的

验证 ConPTY（`portable-pty` 0.9.0）在 Windows 本机上的批量吞吐，决定 Phase 1 终端数据投递方案：
Tauri Channel 承载，或改用本地 socket（Windows 命名管道 / macOS unix socket）。

## 环境

| 项 | 值 |
| --- | --- |
| OS | Microsoft Windows 10.0.26200.7840（Windows 11 24H2/25H2 线） |
| CPU | 13th Gen Intel(R) Core(TM) i5-1340P（12 核） |
| Rust | rustc 1.98.1 / cargo 1.98.1 |
| 依赖 | `portable-pty = "0.9.0"`（ConPTY 后端） |
| 构建 | debug 与 release 均测（见下表） |

## 测量方法

- `src/bin/pty_sink.rs`：独立 sink 进程，从 argv 读目标字节数，向 stdout 写该数量个 `x`（无换行），退出。
- `src/lib.rs::measure_pty_throughput(sink_exe, total_bytes)`：`native_pty_system().openpty()`（40×120）→
  `spawn_command` 启动 sink → 从 master 以 64 KiB 分块读，直到累计 `received >= total_bytes`，统计耗时。
- `tests/throughput.rs` 用 `env!("CARGO_BIN_EXE_pty_sink")` 定位 sink；断言 `received_bytes >= total_bytes`。
- `src/bin/pty_bench.rs` 用 `current_exe().with_file_name("pty_sink" + EXE_SUFFIX)` 定位同目录 sink，默认 8 MiB。

复现命令（工作目录 `src-tauri`）：

```bash
cargo test -p ade-pty --test throughput -- --nocapture
cargo build -p ade-pty --bins && cargo run -p ade-pty --bin pty_bench
cargo build --release -p ade-pty --bins && cargo run --release -p ade-pty --bin pty_bench
```

> `cargo run --bin pty_bench` 只构建 bench 本体，不会构建同目录的 `pty_sink`；缺 sink 时 bench 会打印预期路径并提示先执行 `cargo build -p ade-pty --bins` 后以非零码退出。
> 测量读取带 30 s 兜底（`READ_DEADLINE`）：若 ConPTY 再次因 CPR 等问题停止输出，测试以 `received < total` 断言失败，而不是挂起。

## 实测结果（8 MiB，64 KiB 读块）

| 场景 | 运行 1 | 运行 2 | 运行 3 | 运行 4 | 运行 5 |
| --- | --- | --- | --- | --- | --- |
| debug（`cargo run`） | 23.0 | 23.8 | 21.5 | — | — |
| release（直接执行 exe） | 23.6 | 23.2 | 24.1 | 20.9 | 23.0 |

- 结果区间：**约 21–24 MB/s**（单次波动 ±10%）。
- 早期几次 release 测量曾落在 ~11.8 MB/s，重跑后无法复现，判定为当时系统负载（同批 debug 构建/其它进程）干扰，正式取值以重复稳定后的 ~21–24 MB/s 为准。
- 分尺寸（release，单次）：1 MiB=180 ms / 2 MiB=94 ms / 4 MiB=176 ms / 8 MiB=338 ms / 16 MiB=714 ms，
  小尺寸被 ~100–180 ms 的 PTY 创建+ConPTY 冷启动成本主导；4 MiB 以上边际吞吐约 22–26 MB/s。

## 关键发现

### 1. ConPTY 启动依赖 CPR，否则完全阻塞（已修复）

`portable-pty` 以 `PSUEDOCONSOLE_INHERIT_CURSOR` 创建 ConPTY。启动时 ConPTY 先输出 `ESC[6n`
（光标位置查询），在收到终端回复（Cursor Position Report）之前**不转发任何输出**：

- 对照实验：`cmd.exe /c echo hello` 经此 PTY 只收到 4 字节 `ESC[6n`，随后 10 s 无输出、子进程不退出。
- 修复：测量循环扫描读到的字节，发现 `ESC[6n` 即向 master writer 回写 `ESC[1;1R`（见 `lib.rs::reply_to_cursor_query`）。
- 回写后 `cmd /c echo` 立即完成；8 MiB 测量正常收敛。**Phase 1 正式实现必须包含该 CPR 应答，或改用不继承光标的方式创建 PTY。**

### 2. ConPTY 是渲染层，输出会被变换放大

sink 写 8,388,608 字节纯 `x`（无换行），完整捕获 ConPTY 输出为 **9,227,127 字节（+10.0%）**：

| 组成 | 字节数 | 说明 |
| --- | --- | --- |
| `x` | 8,458,475 | 输入 8,388,608 + 69,867 个重绘字符 |
| `\r\n` | 69,866 × 2 | 每 120 列自动换行 |
| `ESC[39;120H` | 69,866 × 9 | 每行滚动后光标重定位 |
| 启动序列 | ~123 | `ESC[6n`、`ESC[?9001h`、`ESC[?1004h`、`ESC[m`、OSC 标题（子进程 exe 路径）、`ESC[?25l/h` 等 |

- 首次换行样本：`...xxxx\r\n ESC[39;120H x...`（ConPTY 换行后重绘末字符）。
- 测试读取在累计达到目标字节时即停止，因此断言处实测 delta 仅 **+433…+462 字节**（含 4 字节 `ESC[6n`），
  并非全量 +10%；全量放大发生在停止读取之后仍在管道中的数据里。
- 结论：`received >= total` 成立，但 **不能用等值断言**；若要拿到原始字节，需绕开 ConPTY 渲染（passthrough 模式）。

## 观察

- 修复 CPR 后无死锁、无丢字节；读到 EOF/子进程退出的路径未出现异常。
- 读取循环达到 `total_bytes` 后立即 `child.kill()`，此时 sink 可能仍在写（放大后的输出先到达阈值），属预期。
- 吞吐瓶颈是 ConPTY 渲染/转发（conhost 侧），debug 与 release 差异不显著。

## 结论与决策

- 8 MiB 批量场景实测 **~21–24 MB/s，低于 50 MB/s 阈值**。
- **Phase 1 决策：终端输出数据通道不使用 Tauri Channel，改用本地 socket 投递**
  （Windows 命名管道 / macOS unix socket，由独立 PTY 宿主进程推送，配合背压分片）。
  Tauri Channel 仅承载控制/状态类小消息（tab 状态、resize 等）。
- 后续如吞吐不足，可评估 Windows `PSEUDOCONSOLE_PASSTHROUGH_MODE` 直连以绕过渲染放大，
  但 `portable-pty` 未暴露该开关，需要直接调用 ConPTY API，留待 Phase 1+ 评估。
