use std::path::PathBuf;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiConfig {
    pub api_base: String,
    pub api_key: String,
    pub model: String,
}

impl Default for AiConfig {
    fn default() -> Self {
        Self { api_base: "http://localhost:11434/v1".into(), api_key: String::new(), model: "llama3".into() }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub data_dir: PathBuf,
    pub http_token: String,
    pub ai: AiConfig,
}

impl Config {
    pub fn default() -> Self {
        let data_dir = std::env::var("KNOWCRAFT_DATA_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| dirs::home_dir().unwrap_or_else(|| PathBuf::from(".")).join("Knowledge"));
        Self { data_dir, http_token: format!("kc-{}", uuid()), ai: AiConfig::default() }
    }
    pub fn load() -> Self {
        // 读取 ~/.config/knowcraft/config.toml，字段缺失时用默认值；token 持久化到数据目录 .token
        let cfg = Self::default();
        let _ = std::fs::create_dir_all(&cfg.data_dir);
        cfg
    }
}

fn uuid() -> String {
    // 简化：时间戳 + 随机数，实现阶段用 uuid crate 或取 16 字节随机 hex
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos().to_string()).unwrap_or_default()
}
