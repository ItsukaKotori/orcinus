fn main() {
    // macOS：把 macos-info.plist 嵌入 __TEXT,__info_plist 段（NSBundle 对裸
    // 二进制的回退读取路径），使 WKWebView 网络子进程的环回 WebSocket 通过
    // 本地网络隐私与 ATS（见 plist 内 WHY 注释）。发布 .app 的 bundle
    // Info.plist 优先于该段，不受影响。
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        println!(
            "cargo:rustc-link-arg=-Wl,-sectcreate,__TEXT,__info_plist,{}",
            std::path::Path::new(&std::env::var("CARGO_MANIFEST_DIR").unwrap())
                .join("macos-info.plist")
                .display()
        );
    }
    tauri_build::build()
}
