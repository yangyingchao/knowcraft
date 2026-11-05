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
        let mut cfg = Self::default();
        let config_path = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("knowcraft")
            .join("config.toml");
        if config_path.exists() {
            let contents = std::fs::read_to_string(&config_path)?;
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
