use cef::args::Args;
use cef::*;

fn main() {
    #[cfg(target_os = "macos")]
    let _library = {
        let loader = library_loader::LibraryLoader::new(
            &std::env::current_exe().expect("current exe path is unavailable"),
            true,
        );
        assert!(loader.load(), "failed to load Chromium Embedded Framework");
        loader
    };

    let _ = api_hash(sys::CEF_API_VERSION_LAST, 0);

    let args = Args::new();
    execute_process(
        Some(args.as_main_args()),
        None::<&mut App>,
        std::ptr::null_mut(),
    );
}
