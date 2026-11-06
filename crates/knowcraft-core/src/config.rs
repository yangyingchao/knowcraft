use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::error::{Error, Result};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AiConfig {
    pub api_base: String,
    pub api_key: String,
    pub model: String,
}

impl Default for AiConfig {
    fn default() -> Self {
        Self {
            api_base: "http://localhost:11434/v1".into(),
            api_key: String::new(),
            model: "llama3".into(),
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
    // 读取 ~/.config/knowcraft/config.toml（字段缺失回落默认值），token 持久化到数据目录 .token
    pub fn load() -> Result<Self> {
        Self::load_from(&Self::config_path())
    }

    /// 将当前配置（含 data_dir）写回 ~/.config/knowcraft/config.toml，
    /// 供 GUI 修改默认知识库位置；其余字段原样保留。
    pub fn persist(&self) -> Result<()> {
        self.persist_to(&Self::config_path())
    }

    fn config_path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("knowcraft")
            .join("config.toml")
    }

    fn load_from(config_path: &std::path::Path) -> Result<Self> {
        let mut cfg = Self::default();
        if config_path.exists() {
            let contents = std::fs::read_to_string(config_path)?;
            let file_cfg: Self = toml::from_str(&contents)
                .map_err(|e| Error::Store(format!("config parse error: {e}")))?;
            cfg.data_dir = file_cfg.data_dir;
            cfg.ai = file_cfg.ai;
        }
        // KNOWCRAFT_DATA_DIR 环境变量优先级不变
        if let Ok(env_dir) = std::env::var("KNOWCRAFT_DATA_DIR") {
            cfg.data_dir = PathBuf::from(env_dir);
        }
        std::fs::create_dir_all(&cfg.data_dir)?;
        cfg.http_token = load_or_create_token(&cfg.data_dir.join(".token"))?;
        Ok(cfg)
    }

    fn persist_to(&self, config_path: &std::path::Path) -> Result<()> {
        if let Some(parent) = config_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let body = toml::to_string_pretty(self)
            .map_err(|e| Error::Store(format!("config serialize error: {e}")))?;
        std::fs::write(config_path, body)?;
        Ok(())
    }
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
            ai: AiConfig::default(),
        };
        config.persist_to(&path).expect("persist");
        let body = std::fs::read_to_string(&path).expect("read back");
        assert!(
            !body.contains("secret-token"),
            "token must not reach config.toml"
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
}
