//! Publication: write immutable provenance, move the current pointer, then
//! update the lockfile as one recoverable mutation.
//!
//! Ported from `orca:src/main/plugins/plugin-install-publication.ts`:
//!
//! - A same-byte reinstall is a no-op for identity: existing provenance (or a
//!   legacy lock entry accepted for the current bytes) wins over the caller's
//!   new entry, so an interrupted source change can never be recovered later
//!   as though it had published.
//! - If the lockfile write fails, the previous pointer is restored (removed
//!   when there was none) and the original error surfaces.
//! - After a successful change, only the current and immediately previous
//!   version directories and provenance records are retained.

use std::collections::HashSet;
use std::fs;
use std::io::ErrorKind;
use std::path::Path;

use crate::discovery::{is_plugin_content_hash, is_qualified_plugin_key};
use crate::install::lockfile::{read_lockfile, upsert_entry, write_lockfile_atomic, LockEntry};
use crate::install::pointer::{read_pointer, restore_pointer, write_pointer};
use crate::install::provenance::{prune_provenance, read_provenance, write_provenance, Provenance};
use crate::paths::ScopeRoots;
use crate::types::{PluginError, INVALID_MANIFEST, IO_ERROR};

/// Publishes executable identity and provenance as one recoverable mutation,
/// then retains only current plus one rollback version.
pub fn publish_install(
    roots: &ScopeRoots,
    plugin_key: &str,
    content_hash: &str,
    entry: &LockEntry,
    lockfile_path: &Path,
) -> Result<(), PluginError> {
    if !is_qualified_plugin_key(plugin_key) {
        return Err(PluginError::new(
            INVALID_MANIFEST,
            "invalid qualified plugin key",
        ));
    }
    if entry.content_hash != content_hash || entry.plugin_key != plugin_key {
        return Err(PluginError::new(
            INVALID_MANIFEST,
            "lock entry identity does not match the published version",
        ));
    }
    let plugin_dir = roots.install_dir.join(plugin_key);
    let previous_content_hash = read_pointer(&plugin_dir)?;
    let matches_previous = previous_content_hash.as_deref() == Some(content_hash);
    let current_lock = read_lockfile(lockfile_path);
    let existing_provenance = if matches_previous {
        read_provenance(&plugin_dir, content_hash).filter(|provenance| {
            provenance.entry.plugin_key == plugin_key
                && provenance.entry.content_hash == content_hash
        })
    } else {
        None
    };
    let legacy_current_entry = if matches_previous {
        current_lock
            .plugins
            .get(plugin_key)
            .filter(|existing| {
                existing.plugin_key == plugin_key && existing.content_hash == content_hash
            })
            .cloned()
    } else {
        None
    };
    // Provenance is immutable per executable identity. A same-byte reinstall
    // is a no-op so a failed or interrupted source change cannot be recovered
    // later as though it had successfully published.
    let published_entry = existing_provenance
        .as_ref()
        .map(|provenance| provenance.entry.clone())
        .or(legacy_current_entry)
        .unwrap_or_else(|| entry.clone());
    let next_lock = upsert_entry(&current_lock, published_entry.clone());

    if existing_provenance.is_none() {
        write_provenance(
            &plugin_dir,
            content_hash,
            &Provenance::new(published_entry.clone()),
        )?;
    }
    write_pointer(&plugin_dir, content_hash)?;
    if let Err(publication_error) = write_lockfile_atomic(lockfile_path, &next_lock) {
        return match restore_pointer(&plugin_dir, previous_content_hash.as_deref()) {
            Ok(()) => Err(publication_error),
            Err(rollback_error) => Err(PluginError::new(
                IO_ERROR,
                format!(
                    "plugin install publication and pointer rollback both failed: \
                     {publication_error}; {rollback_error}"
                ),
            )),
        };
    }
    // Reinstalling B must not collapse an existing A rollback into {B}.
    if previous_content_hash.as_deref() != Some(content_hash) {
        let mut retained = HashSet::new();
        retained.insert(content_hash.to_string());
        if let Some(previous) = previous_content_hash.filter(|hash| is_plugin_content_hash(hash)) {
            retained.insert(previous);
        }
        let _ = prune_historical_versions(&plugin_dir, &retained);
    }
    Ok(())
}

/// Removes hash-named version directories not in `retained` and prunes
/// provenance to the same set. Cleanup failures are deliberately ignored by
/// [`publish_install`] (the oracle swallows prune errors too).
pub fn prune_historical_versions(
    plugin_dir: &Path,
    retained: &HashSet<String>,
) -> Result<(), PluginError> {
    let entries = match fs::read_dir(plugin_dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(PluginError::new(
                IO_ERROR,
                format!("failed to read the plugin directory: {error}"),
            ))
        }
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let is_directory = entry.file_type().is_ok_and(|file_type| file_type.is_dir());
        if is_directory && is_plugin_content_hash(&name) && !retained.contains(&name) {
            let _ = fs::remove_dir_all(entry.path());
        }
    }
    prune_provenance(plugin_dir, retained)?;
    Ok(())
}
