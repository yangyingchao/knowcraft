use std::path::PathBuf;
use std::sync::Mutex;

use knowcraft_core::config::Config;
use knowcraft_core::error::Error;

// 环境变量相关测试串行执行，避免并行互相干扰
static ENV_LOCK: Mutex<()> = Mutex::new(());

// 记录并恢复环境变量原值（panic 时也恢复）
struct EnvGuard {
    saved: Vec<(&'static str, Option<String>)>,
}

impl EnvGuard {
    fn new() -> Self {
        Self { saved: Vec::new() }
    }

    fn set(&mut self, key: &'static str, value: &str) {
        self.saved.push((key, std::env::var(key).ok()));
        std::env::set_var(key, value);
    }

    fn unset(&mut self, key: &'static str) {
        self.saved.push((key, std::env::var(key).ok()));
        std::env::remove_var(key);
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        for (key, value) in self.saved.drain(..).rev() {
            match value {
                Some(v) => std::env::set_var(key, v),
                None => std::env::remove_var(key),
            }
        }
    }
}

fn temp_dir(label: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!(
        "knowcraft-test-{label}-{}-{nanos}",
        std::process::id()
    ))
}

#[test]
fn error_display_roundtrip() {
    let e = Error::Store("db corrupt".into());
    assert!(e.to_string().contains("db corrupt"));
}

#[test]
fn config_defaults_to_knowledge_dir() {
    let _guard = ENV_LOCK.lock().unwrap();
    let mut env = EnvGuard::new();
    env.unset("KNOWCRAFT_DATA_DIR");
    env.set("HOME", "/tmp/knowcraft-default-home");
    let cfg = Config::default();
    assert!(cfg.data_dir.to_string_lossy().ends_with("Knowledge"));
}

#[test]
fn load_reads_config_toml_overrides_data_dir() {
    let _guard = ENV_LOCK.lock().unwrap();
    let mut env = EnvGuard::new();
    env.unset("KNOWCRAFT_DATA_DIR");
    let cfg_dir = temp_dir("cfg");
    let data_dir = temp_dir("data");
    let knowcraft_dir = cfg_dir.join("knowcraft");
    std::fs::create_dir_all(&knowcraft_dir).unwrap();
    let toml_content = format!("data_dir = \"{}\"\n", data_dir.display());
    std::fs::write(knowcraft_dir.join("config.toml"), toml_content).unwrap();
    env.set("XDG_CONFIG_HOME", cfg_dir.to_str().unwrap());
    let cfg = Config::load().expect("load should succeed");
    assert_eq!(cfg.data_dir, data_dir);
    assert!(cfg.data_dir.join(".token").exists());
    let _ = std::fs::remove_dir_all(&cfg_dir);
    let _ = std::fs::remove_dir_all(&data_dir);
}

#[test]
fn load_persists_token_across_calls() {
    let _guard = ENV_LOCK.lock().unwrap();
    let mut env = EnvGuard::new();
    let cfg_dir = temp_dir("cfg");
    let data_dir = temp_dir("data");
    std::fs::create_dir_all(&cfg_dir).unwrap();
    env.set("KNOWCRAFT_DATA_DIR", data_dir.to_str().unwrap());
    env.set("XDG_CONFIG_HOME", cfg_dir.to_str().unwrap());
    let first = Config::load().expect("first load");
    let second = Config::load().expect("second load");
    assert!(!first.http_token.is_empty());
    assert_eq!(first.http_token, second.http_token);
    let token_file = data_dir.join(".token");
    assert!(token_file.exists());
    assert_eq!(
        first.http_token,
        std::fs::read_to_string(&token_file).unwrap()
    );
    let _ = std::fs::remove_dir_all(&cfg_dir);
    let _ = std::fs::remove_dir_all(&data_dir);
}

#[test]
fn load_returns_error_on_corrupt_config_toml() {
    let _guard = ENV_LOCK.lock().unwrap();
    let mut env = EnvGuard::new();
    env.unset("KNOWCRAFT_DATA_DIR");
    let cfg_dir = temp_dir("cfg");
    let knowcraft_dir = cfg_dir.join("knowcraft");
    std::fs::create_dir_all(&knowcraft_dir).unwrap();
    std::fs::write(knowcraft_dir.join("config.toml"), "data_dir = [\n").unwrap();
    env.set("XDG_CONFIG_HOME", cfg_dir.to_str().unwrap());
    let result = Config::load();
    assert!(matches!(result, Err(Error::Store(_))));
    let _ = std::fs::remove_dir_all(&cfg_dir);
}

#[cfg(unix)]
#[test]
fn load_creates_token_with_0600_permissions() {
    use std::os::unix::fs::PermissionsExt;
    let _guard = ENV_LOCK.lock().unwrap();
    let mut env = EnvGuard::new();
    let cfg_dir = temp_dir("cfg");
    let data_dir = temp_dir("data");
    std::fs::create_dir_all(&cfg_dir).unwrap();
    env.set("KNOWCRAFT_DATA_DIR", data_dir.to_str().unwrap());
    env.set("XDG_CONFIG_HOME", cfg_dir.to_str().unwrap());
    let cfg = Config::load().expect("load should succeed");
    let meta = std::fs::metadata(cfg.data_dir.join(".token")).expect("token file should exist");
    assert_eq!(meta.permissions().mode() & 0o777, 0o600);
    let _ = std::fs::remove_dir_all(&cfg_dir);
    let _ = std::fs::remove_dir_all(&data_dir);
}

#[test]
fn load_honors_knowcraft_data_dir_env() {
    let _guard = ENV_LOCK.lock().unwrap();
    let mut env = EnvGuard::new();
    let cfg_dir = temp_dir("cfg");
    let data_dir = temp_dir("data");
    std::fs::create_dir_all(&cfg_dir).unwrap();
    env.set("KNOWCRAFT_DATA_DIR", data_dir.to_str().unwrap());
    env.set("XDG_CONFIG_HOME", cfg_dir.to_str().unwrap());
    let cfg = Config::load().expect("load should succeed");
    assert_eq!(cfg.data_dir, data_dir);
    let _ = std::fs::remove_dir_all(&cfg_dir);
    let _ = std::fs::remove_dir_all(&data_dir);
}
