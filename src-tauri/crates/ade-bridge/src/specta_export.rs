use std::sync::OnceLock;

use tauri_specta::{collect_commands, Builder};

use crate::commands;

/// The single source of truth for the command surface: registering a command
/// here wires both the Tauri invoke handler and the generated bindings.
/// Cached in a `OnceLock` because `Builder::invoke_handler` borrows the builder
/// and the Tauri handler must be `'static`.
pub fn bridge_builder() -> &'static Builder<tauri::Wry> {
    static BUILDER: OnceLock<Builder<tauri::Wry>> = OnceLock::new();
    BUILDER.get_or_init(|| {
        Builder::<tauri::Wry>::new()
            // The renderer contract types `size`/`mtime`/`line`/`maxResults` as
            // JS `number`; u64/usize are known to stay in safe-integer range.
            .dangerously_cast_bigints_to_number()
            .commands(collect_commands![
                commands::fs::fs_read_dir,
                commands::fs::fs_read_file,
                commands::fs::fs_write_file,
                commands::fs::fs_create_file,
                commands::fs::fs_create_dir,
                commands::fs::fs_rename,
                commands::fs::fs_copy,
                commands::fs::fs_delete_path,
                commands::fs::fs_stat,
                commands::fs::fs_path_exists,
                commands::fs::fs_paths_exist,
                commands::fs::fs_list_files,
                commands::fs::fs_cancel_list_files,
                commands::fs::fs_search,
                commands::fs::fs_watch_worktree,
                commands::fs::fs_unwatch_worktree,
                commands::fs::fs_list_markdown_documents,
                commands::fs::fs_authorize_external_path,
                commands::settings::settings_get,
                commands::settings::settings_set,
                commands::ui::ui_get,
                commands::ui::ui_set,
                commands::ui::ui_record_feature_interaction,
                commands::platform::platform_get,
                commands::app::app_get_identity,
                commands::repos::repos_list,
                commands::repos::repos_add,
                commands::repos::repos_update,
                commands::repos::repos_remove,
                commands::repos::repos_reorder_for_host,
                commands::repos::repos_pick_folder,
                commands::repos::repos_pick_folders,
                commands::repos::repos_pick_directory,
                commands::repos::repos_is_git_available,
                commands::repos::repos_get_default_create_project_parent,
                commands::worktrees::worktrees_list,
                commands::worktrees::worktrees_list_all,
            ])
            .typ::<crate::state::BootstrapPayload>()
            .typ::<ade_fs::FsChangedPayload>()
            .typ::<crate::events::WorktreeChangedPayload>()
    })
}

/// Render the TypeScript bindings into a string. Exports through a temporary
/// file because `tauri-specta`'s `LanguageExt` only writes to disk.
pub fn export_bindings() -> String {
    let path = std::env::temp_dir().join(format!(
        "ade-bridge-bindings-{}.ts",
        ade_core::ids::new_uuid()
    ));
    bridge_builder()
        .export(specta_typescript::Typescript::default(), &path)
        .expect("failed to export tauri-specta bindings");
    let text = std::fs::read_to_string(&path).expect("failed to read exported bindings");
    let _ = std::fs::remove_file(&path);
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bindings_are_fresh() {
        let expected = include_str!("../../../../src/bridge/real/generated/tauri-bindings.ts");
        assert_eq!(
            export_bindings(),
            expected,
            "regenerate with: cargo run -p ade-bridge --bin export-bindings"
        );
    }

    #[test]
    fn export_is_deterministic() {
        assert_eq!(export_bindings(), export_bindings());
    }

    #[test]
    fn export_lists_every_command() {
        let bindings = export_bindings();
        for command in [
            "fs_read_dir",
            "fs_read_file",
            "fs_write_file",
            "fs_create_file",
            "fs_create_dir",
            "fs_rename",
            "fs_copy",
            "fs_delete_path",
            "fs_stat",
            "fs_path_exists",
            "fs_paths_exist",
            "fs_list_files",
            "fs_cancel_list_files",
            "fs_search",
            "fs_watch_worktree",
            "fs_unwatch_worktree",
            "fs_list_markdown_documents",
            "fs_authorize_external_path",
            "settings_get",
            "settings_set",
            "ui_get",
            "ui_set",
            "ui_record_feature_interaction",
            "platform_get",
            "app_get_identity",
            "repos_list",
            "repos_add",
            "repos_update",
            "repos_remove",
            "repos_reorder_for_host",
            "repos_pick_folder",
            "repos_pick_folders",
            "repos_pick_directory",
            "repos_is_git_available",
            "repos_get_default_create_project_parent",
            "worktrees_list",
            "worktrees_list_all",
        ] {
            assert!(
                bindings.contains(&format!("\"{command}\"")),
                "bindings are missing command {command}"
            );
        }
    }
}
