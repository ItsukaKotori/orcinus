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
                commands::ui::ui_set_with_ack,
                commands::ui::ui_record_feature_interaction,
                commands::onboarding::onboarding_get,
                commands::onboarding::onboarding_update,
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
                commands::worktrees::worktrees_create,
                commands::worktrees::worktrees_remove,
                commands::worktrees::worktrees_forget_local,
                commands::worktrees::worktrees_force_delete_preserved_branch,
                commands::worktrees::worktrees_update_meta,
                commands::worktrees::worktrees_persist_sort_order,
                commands::project_groups::project_groups_list,
                commands::project_groups::project_groups_create,
                commands::project_groups::project_groups_update,
                commands::project_groups::project_groups_delete,
                commands::project_groups::project_groups_move_project,
                commands::project_groups::project_groups_scan_nested,
                commands::project_groups::project_groups_cancel_nested_scan,
                commands::project_groups::project_groups_import_nested,
                commands::folder_workspaces::folder_workspaces_list,
                commands::folder_workspaces::folder_workspaces_create,
                commands::folder_workspaces::folder_workspaces_update,
                commands::folder_workspaces::folder_workspaces_delete,
                commands::folder_workspaces::folder_workspaces_get_path_status,
                commands::git::git_status,
                commands::git::git_cancel_status,
                commands::git::git_diff,
                commands::git::git_stage,
                commands::git::git_bulk_stage,
                commands::git::git_unstage,
                commands::git::git_bulk_unstage,
                commands::git::git_discard,
                commands::git::git_bulk_discard,
                commands::git::git_commit,
                commands::git::git_upstream_status,
                commands::git::git_conflict_operation,
                commands::git::git_branch_compare,
                commands::git::git_commit_compare,
                commands::git::git_branch_diff,
                commands::git::git_commit_diff,
                commands::git::git_history,
            ])
            .typ::<crate::state::BootstrapPayload>()
            .typ::<ade_fs::FsChangedPayload>()
            .typ::<crate::events::WorktreeChangedPayload>()
            .typ::<crate::events::ScanNestedProgressPayload>()
            .typ::<commands::project_groups::NestedRepoImportResult>()
            .typ::<ade_core::models::folder_workspace::FolderWorkspacePathStatus>()
            .typ::<ade_core::models::project_group::NestedRepoScanResult>()
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
            "ui_set_with_ack",
            "ui_record_feature_interaction",
            "onboarding_get",
            "onboarding_update",
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
            "worktrees_create",
            "worktrees_remove",
            "worktrees_forget_local",
            "worktrees_force_delete_preserved_branch",
            "worktrees_update_meta",
            "worktrees_persist_sort_order",
            "project_groups_list",
            "project_groups_create",
            "project_groups_update",
            "project_groups_delete",
            "project_groups_move_project",
            "project_groups_scan_nested",
            "project_groups_cancel_nested_scan",
            "project_groups_import_nested",
            "folder_workspaces_list",
            "folder_workspaces_create",
            "folder_workspaces_update",
            "folder_workspaces_delete",
            "folder_workspaces_get_path_status",
            "git_status",
            "git_cancel_status",
            "git_diff",
            "git_stage",
            "git_bulk_stage",
            "git_unstage",
            "git_bulk_unstage",
            "git_discard",
            "git_bulk_discard",
            "git_commit",
            "git_upstream_status",
            "git_conflict_operation",
            "git_branch_compare",
            "git_commit_compare",
            "git_branch_diff",
            "git_commit_diff",
            "git_history",
        ] {
            assert!(
                bindings.contains(&format!("\"{command}\"")),
                "bindings are missing command {command}"
            );
        }
    }
}
