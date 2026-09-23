use std::path::{Path, PathBuf};

use serde_json::Value;

pub struct JsonFile {
    path: PathBuf,
}

impl JsonFile {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn load(&self) -> Value {
        let candidates = [
            self.path.clone(),
            backup_path(&self.path, 1),
            backup_path(&self.path, 2),
        ];
        for candidate in candidates {
            if let Ok(text) = std::fs::read_to_string(&candidate) {
                if let Ok(value) = serde_json::from_str(&text) {
                    return value;
                }
            }
        }
        Value::Null
    }

    pub fn save(&self, value: &Value) -> std::io::Result<()> {
        if let Some(parent) = self
            .path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent)?;
        }
        self.rotate_backups();
        let tmp = tmp_path(&self.path);
        let payload = format!(
            "{}\n",
            serde_json::to_string_pretty(value).map_err(std::io::Error::other)?
        );
        let mut file = std::fs::File::create(&tmp)?;
        use std::io::Write;
        file.write_all(payload.as_bytes())?;
        file.sync_all()?;
        std::fs::rename(&tmp, &self.path)
    }

    fn rotate_backups(&self) {
        if !self.path.exists() {
            return;
        }
        let bak1 = backup_path(&self.path, 1);
        let bak2 = backup_path(&self.path, 2);
        let _ = std::fs::remove_file(&bak2);
        if bak1.exists() {
            let _ = std::fs::rename(&bak1, &bak2);
        }
        let _ = std::fs::rename(&self.path, &bak1);
    }
}

pub fn backup_path(path: &Path, index: u32) -> PathBuf {
    let mut os = path.as_os_str().to_os_string();
    os.push(format!(".bak{index}"));
    PathBuf::from(os)
}

fn tmp_path(path: &Path) -> PathBuf {
    path.with_extension("tmp")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestDir;
    use serde_json::json;

    fn read_json(path: &Path) -> Value {
        serde_json::from_str(&std::fs::read_to_string(path).expect("read file"))
            .expect("parse file")
    }

    #[test]
    fn save_then_load_round_trips() {
        let dir = TestDir::new("json-round-trip");
        let file = JsonFile::new(dir.file("settings.json"));
        let value = json!({"a": 1, "nested": {"b": [1, 2, 3]}});
        file.save(&value).unwrap();
        assert_eq!(file.load(), value);
        let text = std::fs::read_to_string(dir.file("settings.json")).unwrap();
        assert_eq!(
            text,
            format!("{}\n", serde_json::to_string_pretty(&value).unwrap())
        );
    }

    #[test]
    fn save_creates_parent_directories() {
        let dir = TestDir::new("json-parent-dirs");
        let file = JsonFile::new(dir.file("nested/deeper/settings.json"));
        file.save(&json!({"ok": true})).unwrap();
        assert_eq!(file.load(), json!({"ok": true}));
    }

    #[test]
    fn load_falls_back_to_bak1_when_main_is_corrupt() {
        let dir = TestDir::new("json-fallback-bak1");
        let file = JsonFile::new(dir.file("settings.json"));
        file.save(&json!({"v": 1})).unwrap();
        file.save(&json!({"v": 2})).unwrap();
        std::fs::write(dir.file("settings.json"), "{ not json").unwrap();
        assert_eq!(file.load(), json!({"v": 1}));
    }

    #[test]
    fn load_falls_back_to_bak2_when_main_and_bak1_are_corrupt() {
        let dir = TestDir::new("json-fallback-bak2");
        let file = JsonFile::new(dir.file("settings.json"));
        file.save(&json!({"v": 1})).unwrap();
        file.save(&json!({"v": 2})).unwrap();
        file.save(&json!({"v": 3})).unwrap();
        std::fs::write(dir.file("settings.json"), "broken").unwrap();
        std::fs::write(backup_path(&dir.file("settings.json"), 1), "broken").unwrap();
        assert_eq!(file.load(), json!({"v": 1}));
    }

    #[test]
    fn load_returns_null_when_everything_is_missing_or_corrupt() {
        let dir = TestDir::new("json-all-corrupt");
        let file = JsonFile::new(dir.file("settings.json"));
        assert_eq!(file.load(), Value::Null);
        std::fs::write(dir.file("settings.json"), "broken").unwrap();
        std::fs::write(backup_path(&dir.file("settings.json"), 1), "broken").unwrap();
        std::fs::write(backup_path(&dir.file("settings.json"), 2), "broken").unwrap();
        assert_eq!(file.load(), Value::Null);
    }

    #[test]
    fn save_rotates_backups() {
        let dir = TestDir::new("json-rotation");
        let file = JsonFile::new(dir.file("settings.json"));
        file.save(&json!({"v": 1})).unwrap();
        file.save(&json!({"v": 2})).unwrap();
        assert_eq!(read_json(&dir.file("settings.json")), json!({"v": 2}));
        assert_eq!(
            read_json(&backup_path(&dir.file("settings.json"), 1)),
            json!({"v": 1})
        );
        assert!(!backup_path(&dir.file("settings.json"), 2).exists());

        file.save(&json!({"v": 3})).unwrap();
        assert_eq!(read_json(&dir.file("settings.json")), json!({"v": 3}));
        assert_eq!(
            read_json(&backup_path(&dir.file("settings.json"), 1)),
            json!({"v": 2})
        );
        assert_eq!(
            read_json(&backup_path(&dir.file("settings.json"), 2)),
            json!({"v": 1})
        );
    }

    #[test]
    fn save_leaves_no_tmp_file_behind() {
        let dir = TestDir::new("json-no-tmp");
        let file = JsonFile::new(dir.file("settings.json"));
        file.save(&json!({"v": 1})).unwrap();
        file.save(&json!({"v": 2})).unwrap();
        assert!(!dir.file("settings.tmp").exists());
    }

    #[test]
    fn backup_path_appends_index_suffix() {
        assert_eq!(
            backup_path(Path::new("settings.json"), 1),
            Path::new("settings.json.bak1")
        );
        assert_eq!(
            backup_path(Path::new("settings.json"), 2),
            Path::new("settings.json.bak2")
        );
    }
}
