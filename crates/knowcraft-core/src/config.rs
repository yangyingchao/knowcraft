use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use crate::error::{Error, Result};

const MIGRATED_KEY: &str = "core_config_migrated";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AiConfig {
    pub api_base: String,
    #[serde(skip_serializing)]
    pub api_key: String,
    pub model: String,
}

impl Default for AiConfig {
    fn default() -> Self {
        Self {
            api_base: "http://127.0.0.1:8642/v1".into(),
            api_key: String::new(),
            model: "hermes-agent".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub data_dir: PathBuf,
    /// 鉴权凭证只存数据目录的 `.token`，不写入 config.toml。
    #[serde(skip_serializing)]
    pub http_token: String,
    pub ai: AiConfig,
}

impl Default for Config {
    fn default() -> Self {
        let data_dir = std::env::var("KNOWCRAFT_DATA_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                dirs::home_dir()
                    .unwrap_or_else(|| PathBuf::from("."))
                    .join("Knowledge")
            });
        Self {
            data_dir,
            http_token: String::new(),
            ai: AiConfig::default(),
        }
    }
}

impl Config {
    pub fn migrate_settings() -> Result<()> {
        migrate_config(&Self::settings_path(), &Self::legacy_path())
    }

    pub fn load() -> Result<Self> {
        let path = Self::settings_path();
        Self::migrate_settings()?;
        Self::load_from(&path)
    }

    /// 只更新 core 拥有的字段，保留 GUI 偏好及未知字段。
    pub fn persist(&self) -> Result<()> {
        let path = Self::settings_path();
        migrate_config(&path, &Self::legacy_path())?;
        self.persist_to(&path)
    }

    /// 仅更新 settings.toml 的知识库位置。
    pub fn persist_data_dir(path: &Path) -> Result<()> {
        let settings = Self::settings_path();
        migrate_config(&settings, &Self::legacy_path())?;
        edit_settings(&settings, |table| {
            table.insert(
                "data_dir".into(),
                toml::Value::String(path.to_string_lossy().into_owned()),
            );
            Ok(true)
        })?;
        Ok(())
    }

    fn settings_path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("knowcraft/settings.toml")
    }

    fn legacy_path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("knowcraft/config.toml")
    }

    fn load_from(config_path: &std::path::Path) -> Result<Self> {
        let mut cfg = Self::default();
        if config_path.exists() {
            let contents = fs::read_to_string(config_path)?;
            let table: toml::Table = toml::from_str(&contents)
                .map_err(|e| Error::Store(format!("config parse error: {e}")))?;
            if let Some(value) = table.get("data_dir") {
                cfg.data_dir = PathBuf::from(
                    value
                        .as_str()
                        .ok_or_else(|| Error::Store("data_dir must be a string".into()))?,
                );
            }
            if let Some(value) = table.get("ai") {
                cfg.ai = value
                    .clone()
                    .try_into()
                    .map_err(|e| Error::Store(format!("ai config parse error: {e}")))?;
            }
        }
        // KNOWCRAFT_DATA_DIR 环境变量优先级不变
        if let Ok(env_dir) = std::env::var("KNOWCRAFT_DATA_DIR") {
            cfg.data_dir = PathBuf::from(env_dir);
        }
        std::fs::create_dir_all(&cfg.data_dir)?;
        cfg.http_token = load_or_create_token(&cfg.data_dir.join(".token"))?;
        Ok(cfg)
    }

    fn persist_to(&self, config_path: &Path) -> Result<()> {
        edit_settings(config_path, |table| {
            table.insert(
                "data_dir".into(),
                toml::Value::String(self.data_dir.to_string_lossy().into_owned()),
            );
            table.insert(
                "ai".into(),
                toml::Value::try_from(&self.ai)
                    .map_err(|e| Error::Store(format!("ai serialize error: {e}")))?,
            );
            Ok(true)
        })?;
        Ok(())
    }
}

fn migrate_config(settings: &Path, legacy: &Path) -> Result<()> {
    if !settings.exists() && !legacy.exists() {
        return Ok(());
    }
    edit_settings(settings, |table| {
        if table.get(MIGRATED_KEY).and_then(toml::Value::as_bool) == Some(true) {
            return Ok(false);
        }
        let old: toml::Table = match fs::read_to_string(legacy) {
            Ok(contents) => toml::from_str(&contents)
                .map_err(|e| Error::Store(format!("legacy config parse error: {e}")))?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => toml::Table::new(),
            Err(error) => return Err(error.into()),
        };
        let mut gui = match table.remove("gui") {
            Some(toml::Value::Table(entries)) => entries,
            Some(_) => return Err(Error::Store("gui must be a table".into())),
            None => toml::Table::new(),
        };
        let keys: Vec<_> = table
            .keys()
            .filter(|key| !matches!(key.as_str(), "data_dir" | "ai" | MIGRATED_KEY))
            .cloned()
            .collect();
        for key in keys {
            if let Some(value) = table.remove(&key) {
                gui.entry(key).or_insert(value);
            }
        }
        if let Some(value) = old.get("data_dir") {
            table.entry("data_dir").or_insert_with(|| value.clone());
        }
        if let Some(value) = old.get("ai").and_then(toml::Value::as_table) {
            let ai = table
                .entry("ai")
                .or_insert_with(|| toml::Value::Table(toml::Table::new()));
            let entries = ai
                .as_table_mut()
                .ok_or_else(|| Error::Store("ai must be a table".into()))?;
            for key in ["api_base", "model"] {
                if let Some(value) = value.get(key) {
                    entries.entry(key).or_insert_with(|| value.clone());
                }
            }
        }
        if !gui.is_empty() {
            table.insert("gui".into(), toml::Value::Table(gui));
        }
        table.insert(MIGRATED_KEY.into(), toml::Value::Boolean(true));
        Ok(true)
    })?;
    Ok(())
}

/// Serializes `settings.toml` writes across processes: the GUI and every CLI
/// command lock the same directory file before replacing it.
pub fn lock_settings(path: &Path, wait: Duration) -> io::Result<fs::File> {
    let directory = path
        .parent()
        .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidInput))?;
    fs::create_dir_all(directory)?;
    let lock = fs::File::open(directory)?;
    let deadline = Instant::now() + wait;
    loop {
        match lock.try_lock() {
            Ok(()) => return Ok(lock),
            Err(fs::TryLockError::WouldBlock) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(error) => return Err(error.into()),
        }
    }
}

fn edit_settings(path: &Path, update: impl FnOnce(&mut toml::Table) -> Result<bool>) -> Result<()> {
    let _lock = lock_settings(path, Duration::from_millis(500))?;
    let body = match fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(error.into()),
    };
    let mut table: toml::Table =
        toml::from_str(&body).map_err(|e| Error::Store(format!("settings parse error: {e}")))?;
    if !update(&mut table)? {
        return Ok(());
    }
    let contents = toml::to_string_pretty(&table)
        .map_err(|e| Error::Store(format!("settings serialize error: {e}")))?;
    let target = match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => fs::canonicalize(path)?,
        Ok(_) => path.to_path_buf(),
        Err(error) if error.kind() == io::ErrorKind::NotFound => path.to_path_buf(),
        Err(error) => return Err(error.into()),
    };
    let temp = target.with_file_name(format!(".settings-{}.tmp", uuid::Uuid::new_v4()));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)?;
    let written = (|| -> Result<()> {
        if let Ok(metadata) = fs::metadata(&target) {
            file.set_permissions(metadata.permissions())?;
        }
        file.write_all(contents.as_bytes())?;
        file.sync_all()?;
        fs::rename(&temp, &target)?;
        Ok(())
    })();
    if written.is_err() {
        let _ = fs::remove_file(&temp);
    }
    written
}

// 读取或创建 .token：Unix 首次创建用 0600 权限（http_token 为鉴权凭证，避免同机其他用户可读）
#[cfg(unix)]
fn load_or_create_token(token_path: &std::path::Path) -> Result<String> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut f = match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(token_path)
    {
        Ok(f) => f,
        // 文件已存在（如先前加载过）：回退读取复用
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            return Ok(std::fs::read_to_string(token_path)?);
        }
        Err(e) => return Err(Error::Io(e)),
    };
    let token = generate_token()?;
    f.write_all(token.as_bytes())?;
    Ok(token)
}

#[cfg(not(unix))]
fn load_or_create_token(token_path: &std::path::Path) -> Result<String> {
    if token_path.exists() {
        return Ok(std::fs::read_to_string(token_path)?);
    }
    let token = generate_token()?;
    std::fs::write(token_path, &token)?;
    Ok(token)
}

// 16 字节随机数 hex，32 字符
fn generate_token() -> Result<String> {
    let mut bytes = [0u8; 16];
    getrandom::getrandom(&mut bytes)
        .map_err(|e| Error::Io(std::io::Error::other(e.to_string())))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch_config(name: &str) -> PathBuf {
        std::env::temp_dir()
            .join(format!("knowcraft-config-{}-{name}", std::process::id()))
            .join("config.toml")
    }

    #[test]
    fn persist_round_trips_data_dir_without_writing_token() {
        let path = scratch_config("roundtrip");
        let root = path.parent().expect("scratch root").to_path_buf();
        let _ = std::fs::remove_dir_all(&root);
        let config = Config {
            data_dir: root.join("library"),
            http_token: "secret-token".into(),
            ai: AiConfig {
                api_key: "secret-ai-key".into(),
                ..AiConfig::default()
            },
        };
        config.persist_to(&path).expect("persist");
        let body = std::fs::read_to_string(&path).expect("read back");
        assert!(
            !body.contains("secret-token") && !body.contains("secret-ai-key"),
            "credentials must not reach config.toml"
        );
        let parsed: Config = toml::from_str(&body).expect("parse");
        assert_eq!(parsed.data_dir, config.data_dir);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn persist_then_load_reads_back_data_dir() {
        if std::env::var_os("KNOWCRAFT_DATA_DIR").is_some() {
            // 环境变量优先，回读值不由 config.toml 决定。
            return;
        }
        let path = scratch_config("loadback");
        let root = path.parent().expect("scratch root").to_path_buf();
        let _ = std::fs::remove_dir_all(&root);
        let config = Config {
            data_dir: root.join("library"),
            http_token: String::new(),
            ai: AiConfig::default(),
        };
        config.persist_to(&path).expect("persist");
        let loaded = Config::load_from(&path).expect("load");
        assert_eq!(loaded.data_dir, config.data_dir);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn migration_merges_only_missing_core_values_once() {
        let root = tempfile::tempdir().unwrap();
        let settings = root.path().join("settings.toml");
        let legacy = root.path().join("config.toml");
        fs::write(
            &settings,
            "theme = 'azure'\ndata_dir = '/kept'\n[ai]\nmodel = 'kept-model'\n",
        )
        .unwrap();
        fs::write(&legacy, "data_dir = '/old'\n[ai]\napi_base = 'http://localhost:11434/v1'\nmodel = 'old-model'\napi_key = 'do-not-migrate'\n").unwrap();
        migrate_config(&settings, &legacy).unwrap();
        let body = fs::read_to_string(&settings).unwrap();
        let table: toml::Table = toml::from_str(&body).unwrap();
        assert_eq!(table["gui"]["theme"].as_str(), Some("azure"));
        assert!(!table.contains_key("theme"));
        assert_eq!(table["data_dir"].as_str(), Some("/kept"));
        assert_eq!(table["ai"]["model"].as_str(), Some("kept-model"));
        assert_eq!(
            table["ai"]["api_base"].as_str(),
            Some("http://localhost:11434/v1")
        );
        assert!(!body.contains("do-not-migrate"));
        fs::write(&legacy, "invalid toml [").unwrap();
        migrate_config(&settings, &legacy).unwrap();
        assert_eq!(fs::read_to_string(&settings).unwrap(), body);
    }

    #[test]
    fn persist_preserves_gui_fields() {
        let root = tempfile::tempdir().unwrap();
        let settings = root.path().join("settings.toml");
        fs::write(&settings, "theme = 'dark'\nlanguage = 'zh-CN'\n").unwrap();
        let config = Config {
            data_dir: root.path().join("library"),
            http_token: "secret".into(),
            ai: AiConfig::default(),
        };
        migrate_config(&settings, &root.path().join("config.toml")).unwrap();
        config.persist_to(&settings).unwrap();
        let table: toml::Table = toml::from_str(&fs::read_to_string(&settings).unwrap()).unwrap();
        assert_eq!(table["gui"]["theme"].as_str(), Some("dark"));
        assert_eq!(table["gui"]["language"].as_str(), Some("zh-CN"));
        assert!(table.get("http_token").is_none());
    }

    #[test]
    fn migration_moves_unknown_gui_keys_into_its_table() {
        let root = tempfile::tempdir().unwrap();
        let settings = root.path().join("settings.toml");
        let legacy = root.path().join("config.toml");
        fs::write(
            &settings,
            "future_setting = 'kept'\n[gui]\ntheme = 'dark'\n",
        )
        .unwrap();
        fs::write(
            &legacy,
            "data_dir = '/old'\n[ai]\napi_base = 'http://127.0.0.1:8642/v1'\n",
        )
        .unwrap();
        migrate_config(&settings, &legacy).unwrap();
        let table: toml::Table = toml::from_str(&fs::read_to_string(&settings).unwrap()).unwrap();
        assert_eq!(table["gui"]["theme"].as_str(), Some("dark"));
        assert_eq!(table["gui"]["future_setting"].as_str(), Some("kept"));
        assert_eq!(table["data_dir"].as_str(), Some("/old"));
        assert_eq!(
            table["ai"]["api_base"].as_str(),
            Some("http://127.0.0.1:8642/v1")
        );
        assert!(!table.contains_key("future_setting"));
    }

    #[test]
    fn invalid_settings_are_not_overwritten() {
        let root = tempfile::tempdir().unwrap();
        let settings = root.path().join("settings.toml");
        fs::write(&settings, "broken = [").unwrap();
        assert!(Config::default().persist_to(&settings).is_err());
        assert_eq!(fs::read_to_string(&settings).unwrap(), "broken = [");
    }
}
