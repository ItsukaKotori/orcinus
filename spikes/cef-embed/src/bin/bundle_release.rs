fn main() -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(target_os = "macos")]
    {
        use cef::build_util::mac::{bundle, BundleInfo};
        use semver::Version;
        use std::path::PathBuf;

        let target_directory = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target");
        let bundle_info = BundleInfo::new(
            "cef-embed",
            "dev.ade.spike.cef-embed",
            "ade cef spike",
            "English",
            Version::new(0, 1, 0),
        );
        let app = bundle(
            &target_directory.join("bundle"),
            &target_directory.join("release"),
            "cef-embed",
            "cef_embed_helper",
            None,
            bundle_info,
        )?;
        println!("bundled app: {}", app.display());
    }

    Ok(())
}
