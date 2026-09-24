const BINDINGS_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../src/bridge/real/generated/tauri-bindings.ts"
);

fn main() {
    let bindings = ade_bridge::specta_export::export_bindings();
    let path = std::path::Path::new(BINDINGS_PATH);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create bindings directory");
    }
    std::fs::write(path, &bindings).expect("write bindings");
    eprintln!("len={}", bindings.len());
    println!("wrote {}", path.display());
}
