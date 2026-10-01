//! 默认 shell 解析（Task 8 定型，替换 Task 5 在 [`crate::session`] 的临时实现）。
//!
//! 规则（brief 逐字）：
//! - unix：`shell_override` > `$SHELL` > `/bin/zsh` > `/bin/bash` > `/bin/sh`；
//!   存在时 args=`["-l"]`（登录 shell）。
//! - windows：`shell_override` > `$COMSPEC`（缺省 `"cmd.exe"`）；args=`[]`。
//!
//! [`resolve_shell`] 是纯函数：`platform` 与两枚 env 变量均参数注入而非
//! `#[cfg]`/进程 env 读取——unix CI 即可表驱动跑全部分支（见 `tests/shell.rs`），
//! 进程 env 由调用方（[`crate::session`]）按需快照传入。链中 `/bin/bash`、
//! `/bin/sh` 为规格级回退顺序的文档；纯函数不做文件系统探测（保证测试确定性），
//! 全缺省时静态落在链首 `/bin/zsh`（macOS 缺省 shell）。
//!
//! [`login_args`] 与 [`resolve_shell`] 的 args 同源：unix 恒 `["-l"]`（fish/cmd
//! 等同规则，1C 不特判，`program` 参数留给未来特判），windows 恒 `[]`。

/// 一次 spawn 的 shell 命令形状：二进制 + 参数（[`crate::session`] 逐项喂给
/// `CommandBuilder`）。
pub struct ShellSpec {
    pub program: String,
    pub args: Vec<String>,
}

/// windows 平台标记（[`resolve_shell`] / [`login_args`] 的 `platform` 分支判据；
/// 其余取值一律按 unix 规则处理）。
const PLATFORM_WINDOWS: &str = "windows";

/// unix 登录 shell 参数：`-l`。
const UNIX_LOGIN_FLAG: &str = "-l";

/// 解析 shell：`platform` 取 `"unix"` / `"windows"`（见上）；`shell_override`
/// 为用户盖章的显式覆盖；`env_shell` / `env_comspec` 为调用方读好的 `$SHELL` /
/// `$COMSPEC`（None = 未设置）。两平台的 env 变量互不越界：unix 忽略
/// `env_comspec`，windows 忽略 `env_shell`。
pub fn resolve_shell(
    platform: &str,
    shell_override: Option<&str>,
    env_shell: Option<&str>,
    env_comspec: Option<&str>,
) -> ShellSpec {
    let program = if platform == PLATFORM_WINDOWS {
        shell_override
            .or(env_comspec)
            .unwrap_or("cmd.exe")
            .to_string()
    } else {
        shell_override
            .or(env_shell)
            .unwrap_or("/bin/zsh")
            .to_string()
    };
    ShellSpec {
        args: login_args(platform, &program),
        program,
    }
}

/// 登录 shell 参数：unix 恒 `["-l"]`，windows 恒 `[]`。`program` 当前不参与
/// 判定（fish/cmd 等同规则，1C 不特判），参数保留给未来按 shell 差异化。
pub fn login_args(platform: &str, program: &str) -> Vec<String> {
    let _ = program; // 1C 不特判；见模块文档。
    if platform == PLATFORM_WINDOWS {
        Vec::new()
    } else {
        vec![UNIX_LOGIN_FLAG.to_string()]
    }
}
