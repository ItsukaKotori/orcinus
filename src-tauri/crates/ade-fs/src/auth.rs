use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use ade_core::path_compare::normalize_for_comparison;

use crate::FsError;

/// LRU cap for session-scoped external grants; every caller re-authorizes
/// before operating, so evicting the oldest grant is safe.
pub const AUTHORIZED_EXTERNAL_PATHS_MAX: usize = 4096;

/// Allowed roots = registered repo/folder-workspace roots ∪ explicit external
/// grants. Comparison forms are canonicalized when possible so symlink escapes
/// resolve outside the root, and use `normalize_for_comparison` plus a path
/// segment boundary (`root` or `root + "/"`).
#[derive(Debug, Default)]
pub struct PathAuthRegistry {
    roots: Mutex<Vec<String>>,
    external: Mutex<Vec<String>>,
}

impl PathAuthRegistry {
    pub fn authorize_root(&self, path: &str) -> Result<(), FsError> {
        let forms = normalized_forms(path)?;
        let mut roots = lock(&self.roots);
        for form in forms {
            if !roots.contains(&form) {
                roots.push(form);
            }
        }
        Ok(())
    }

    pub fn revoke_root(&self, path: &str) -> Result<(), FsError> {
        let forms = normalized_forms(path)?;
        let mut roots = lock(&self.roots);
        roots.retain(|root| !forms.contains(root));
        Ok(())
    }

    pub fn authorize_external(&self, path: &str) -> Result<(), FsError> {
        let forms = normalized_forms(path)?;
        let mut external = lock(&self.external);
        for form in forms {
            external.retain(|existing| existing != &form);
            external.push(form);
        }
        while external.len() > AUTHORIZED_EXTERNAL_PATHS_MAX {
            external.remove(0);
        }
        Ok(())
    }

    pub fn resolve(&self, path: &str) -> Result<PathBuf, FsError> {
        self.resolve_inner(path, false)
    }

    pub fn resolve_preserving_symlink(&self, path: &str) -> Result<PathBuf, FsError> {
        self.resolve_inner(path, true)
    }

    pub fn is_allowed(&self, path: &Path) -> bool {
        let candidate = comparison_form(path);
        let roots = lock(&self.roots);
        let external = lock(&self.external);
        roots
            .iter()
            .chain(external.iter())
            .any(|root| is_segment_prefix(&candidate, root))
    }

    pub fn is_authorized_root(&self, path: &Path) -> bool {
        let candidate = comparison_form(path);
        lock(&self.roots).iter().any(|root| root == &candidate)
            || lock(&self.external).iter().any(|root| root == &candidate)
    }

    fn resolve_inner(&self, path: &str, preserve_leaf: bool) -> Result<PathBuf, FsError> {
        let absolute = lexical_absolute(path)?;
        let candidate = if preserve_leaf {
            match (absolute.file_name(), absolute.parent()) {
                (Some(name), Some(parent)) => canonicalize_missing(parent)?.join(name),
                _ => canonicalize_missing(&absolute)?,
            }
        } else {
            canonicalize_missing(&absolute)?
        };
        if self.is_allowed(&candidate) {
            Ok(candidate)
        } else {
            Err(FsError::PathAccessDenied)
        }
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn comparison_form(path: &Path) -> String {
    normalize_for_comparison(&path.to_string_lossy())
}

fn normalized_forms(path: &str) -> Result<Vec<String>, FsError> {
    let absolute = lexical_absolute(path)?;
    let mut forms = vec![comparison_form(&absolute)];
    if let Ok(real) = absolute.canonicalize() {
        let form = comparison_form(&real);
        if !forms.contains(&form) {
            forms.push(form);
        }
    }
    Ok(forms)
}

fn is_segment_prefix(candidate: &str, root: &str) -> bool {
    if root.is_empty() {
        return false;
    }
    if candidate == root {
        return true;
    }
    if root.ends_with('/') {
        candidate.starts_with(root)
    } else {
        candidate.starts_with(root) && candidate.as_bytes().get(root.len()) == Some(&b'/')
    }
}

fn lexical_absolute(path: &str) -> Result<PathBuf, FsError> {
    if path.is_empty() {
        return Err(FsError::InvalidInput("Path must not be empty".to_string()));
    }
    let absolute = std::path::absolute(Path::new(path)).map_err(FsError::Io)?;
    Ok(normalize_lexical(&absolute))
}

fn normalize_lexical(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::RootDir => normalized.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            Component::Normal(part) => normalized.push(part),
        }
    }
    normalized
}

fn canonicalize_missing(path: &Path) -> Result<PathBuf, FsError> {
    match path.canonicalize() {
        Ok(real) => Ok(real),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let mut ancestor = path.to_path_buf();
            let mut missing: Vec<OsString> = Vec::new();
            loop {
                match ancestor.canonicalize() {
                    Ok(real) => {
                        let mut candidate = real;
                        for name in missing.iter().rev() {
                            candidate.push(name);
                        }
                        return Ok(candidate);
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                        let name = ancestor.file_name().map(OsString::from);
                        let parent = ancestor.parent().map(Path::to_path_buf);
                        match (name, parent) {
                            (Some(name), Some(parent)) => {
                                missing.push(name);
                                ancestor = parent;
                            }
                            _ => return Err(FsError::PathAccessDenied),
                        }
                    }
                    Err(error) => return Err(FsError::Io(error)),
                }
            }
        }
        Err(error) => Err(FsError::Io(error)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segment_prefix_requires_a_boundary() {
        assert!(is_segment_prefix("/root", "/root"));
        assert!(is_segment_prefix("/root/file", "/root"));
        assert!(!is_segment_prefix("/root-other", "/root"));
        assert!(!is_segment_prefix("/root-other/file", "/root"));
        assert!(is_segment_prefix("/anything", "/"));
    }

    #[test]
    fn normalize_lexical_collapses_dots_and_trailing_separators() {
        assert_eq!(
            normalize_lexical(Path::new("/root/sub/../file")),
            PathBuf::from("/root/file")
        );
        assert_eq!(
            normalize_lexical(Path::new("/root/./file/")),
            PathBuf::from("/root/file")
        );
        assert_eq!(
            normalize_lexical(Path::new("/../file")),
            PathBuf::from("/file")
        );
    }

    #[test]
    fn rejects_empty_path_grants() {
        let registry = PathAuthRegistry::default();
        assert!(matches!(
            registry.authorize_root(""),
            Err(FsError::InvalidInput(_))
        ));
        assert!(matches!(
            registry.authorize_external(""),
            Err(FsError::InvalidInput(_))
        ));
        assert!(matches!(
            registry.revoke_root(""),
            Err(FsError::InvalidInput(_))
        ));
        assert!(!registry.is_allowed(Path::new("/")));
        assert!(!registry.is_allowed(Path::new("/tmp")));
        assert!(!is_segment_prefix("/tmp", ""));
    }

    #[test]
    fn deny_variant_uses_the_verbatim_message() {
        assert_eq!(
            FsError::PathAccessDenied.to_string(),
            crate::PATH_ACCESS_DENIED_MESSAGE
        );
    }
}
