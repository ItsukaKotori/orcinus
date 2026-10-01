//! Task 8 集成测试：shell 解析纯函数表驱动用例。
//!
//! `platform` 为参数注入而非 `#[cfg]`——unix 环境即可跑全部分支（含 windows
//! 路径）。用例对应 brief Step 1：unix override 命中 / $SHELL 命中 / 全缺省
//! 回退 / windows COMSPEC（含缺省与跨平台 env 隔离）/ windows override。

use ade_pty::shell::{login_args, resolve_shell};

/// 一条表驱动用例：输入四元组 → 期望 program 与 args。
struct Case {
    name: &'static str,
    platform: &'static str,
    shell_override: Option<&'static str>,
    env_shell: Option<&'static str>,
    env_comspec: Option<&'static str>,
    want_program: &'static str,
    want_args: &'static [&'static str],
}

fn cases() -> Vec<Case> {
    vec![
        Case {
            name: "unix override 命中（压过 $SHELL）",
            platform: "unix",
            shell_override: Some("/usr/bin/fish"),
            env_shell: Some("/bin/zsh"),
            env_comspec: None,
            want_program: "/usr/bin/fish",
            want_args: &["-l"],
        },
        Case {
            name: "unix $SHELL 命中",
            platform: "unix",
            shell_override: None,
            env_shell: Some("/bin/bash"),
            env_comspec: None,
            want_program: "/bin/bash",
            want_args: &["-l"],
        },
        Case {
            name: "unix 全缺省回退",
            platform: "unix",
            shell_override: None,
            env_shell: None,
            env_comspec: None,
            want_program: "/bin/zsh",
            want_args: &["-l"],
        },
        Case {
            name: "unix 忽略 COMSPEC",
            platform: "unix",
            shell_override: None,
            env_shell: Some("/bin/bash"),
            env_comspec: Some("cmd.exe"),
            want_program: "/bin/bash",
            want_args: &["-l"],
        },
        Case {
            name: "windows COMSPEC 命中",
            platform: "windows",
            shell_override: None,
            env_shell: None,
            env_comspec: Some("C:\\Windows\\system32\\cmd.exe"),
            want_program: "C:\\Windows\\system32\\cmd.exe",
            want_args: &[],
        },
        Case {
            name: "windows COMSPEC 缺省回退 cmd.exe",
            platform: "windows",
            shell_override: None,
            env_shell: None,
            env_comspec: None,
            want_program: "cmd.exe",
            want_args: &[],
        },
        Case {
            name: "windows override powershell.exe 命中（压过 COMSPEC）",
            platform: "windows",
            shell_override: Some("powershell.exe"),
            env_shell: None,
            env_comspec: Some("cmd.exe"),
            want_program: "powershell.exe",
            want_args: &[],
        },
        Case {
            name: "windows 忽略 SHELL",
            platform: "windows",
            shell_override: None,
            env_shell: Some("/bin/bash"),
            env_comspec: None,
            want_program: "cmd.exe",
            want_args: &[],
        },
    ]
}

#[test]
fn resolve_shell_table() {
    for case in cases() {
        let spec = resolve_shell(
            case.platform,
            case.shell_override,
            case.env_shell,
            case.env_comspec,
        );
        assert_eq!(
            spec.program, case.want_program,
            "case `{}` program",
            case.name
        );
        let want_args: Vec<String> = case.want_args.iter().map(|s| s.to_string()).collect();
        assert_eq!(spec.args, want_args, "case `{}` args", case.name);
    }
}

#[test]
fn login_args_table() {
    // unix 恒 ["-l"]：fish/cmd 等同规则，1C 不特判（program 参数留给未来）。
    assert_eq!(login_args("unix", "/bin/zsh"), vec!["-l".to_string()]);
    assert_eq!(login_args("unix", "/usr/bin/fish"), vec!["-l".to_string()]);
    // windows 恒 []。
    assert_eq!(login_args("windows", "cmd.exe"), Vec::<String>::new());
    assert_eq!(
        login_args("windows", "powershell.exe"),
        Vec::<String>::new()
    );
}

#[test]
fn resolve_shell_args_come_from_login_args() {
    // ShellSpec.args 与 login_args 同源：两平台各抽一条对账。
    let unix = resolve_shell("unix", None, Some("/bin/bash"), None);
    assert_eq!(unix.args, login_args("unix", &unix.program));
    let win = resolve_shell("windows", Some("powershell.exe"), None, Some("cmd.exe"));
    assert_eq!(win.args, login_args("windows", &win.program));
}
