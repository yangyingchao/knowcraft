//! 本地 AI 原型的 Hermes 凭据与 GUI 启动辅助逻辑。

use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant},
};

use crate::error::{Error, Result};

const KEY: &str = "API_SERVER_KEY";

pub fn hermes_env_path() -> Result<PathBuf> {
    let home = std::env::var_os("HERMES_HOME")
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|home| home.join(".hermes")))
        .ok_or_else(|| Error::Ai("cannot find the Hermes home directory".into()))?;
    Ok(home.join(".env"))
}

/// 返回有效密钥；重复或不能安全读取的配置直接失败，不覆盖旧值。
pub fn api_key(path: &Path) -> Result<Option<String>> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if !metadata.file_type().is_file() => {
            return Err(Error::Ai("Hermes .env must be a regular file".into()));
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    }
    let body = match fs::read_to_string(path) {
        Ok(body) => body,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let mut found = None;
    let mut seen = false;
    for line in body.lines() {
        let line = line.trim();
        let line = line.strip_prefix("export ").unwrap_or(line).trim();
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        let Some((name, value)) = line.split_once('=') else {
            continue;
        };
        if name.trim() != KEY {
            continue;
        }
        if seen {
            return Err(Error::Ai("duplicate API_SERVER_KEY in Hermes .env".into()));
        }
        seen = true;
        let value = value.trim();
        let value = if value.len() >= 2
            && ((value.starts_with('"') && value.ends_with('"'))
                || (value.starts_with('\'') && value.ends_with('\'')))
        {
            &value[1..value.len() - 1]
        } else {
            value
        };
        if !value.bytes().all(|byte| {
            byte.is_ascii_graphic() && !matches!(byte, b'\'' | b'"' | b'$' | b'\\' | b'#' | b'`')
        }) {
            return Err(Error::Ai(
                "API_SERVER_KEY uses unsupported shell syntax".into(),
            ));
        }
        if value.len() < 16 {
            return Err(Error::Ai(
                "API_SERVER_KEY is shorter than 16 characters".into(),
            ));
        }
        found = Some(value.to_owned());
    }
    Ok(found)
}

/// 仅在缺失时追加新密钥；新文件以 0600 创建。
pub fn ensure_api_key(path: &Path) -> Result<(String, bool)> {
    if let Some(key) = api_key(path)? {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(path)?.permissions().mode();
            if mode & 0o077 != 0 {
                fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
            }
        }
        return Ok((key, false));
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut bytes = [0u8; 32];
    getrandom::getrandom(&mut bytes).map_err(|error| Error::Ai(error.to_string()))?;
    let key: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    use std::fs::OpenOptions;
    #[cfg(unix)]
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    let mut options = OpenOptions::new();
    options.append(true).create(true);
    #[cfg(unix)]
    options.mode(0o600);
    let mut file = options.open(path)?;
    #[cfg(unix)]
    if file.metadata()?.permissions().mode() & 0o077 != 0 {
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }
    if file.metadata()?.len() > 0 {
        file.write_all(b"\n")?;
    }
    file.write_all(format!("{KEY}={key}\n").as_bytes())?;
    file.sync_all()?;
    Ok((key, true))
}

pub fn prepare(env_path: &Path, api_base: &str) -> Result<()> {
    prepare_with(
        env_path,
        api_base,
        || {
            let output = Command::new("hermes")
                .args(["config", "get", "API_SERVER_ENABLED"])
                .output()?;
            Ok(output.status.success() && String::from_utf8_lossy(&output.stdout).trim() == "true")
        },
        || {
            let status = Command::new("hermes")
                .args(["config", "set", "API_SERVER_ENABLED", "true"])
                .status()?;
            if !status.success() {
                return Err(Error::Ai("failed to enable Hermes API Server".into()));
            }
            Ok(())
        },
        || {
            let status = Command::new("hermes")
                .args(["gateway", "restart"])
                .status()?;
            if !status.success() {
                return Err(Error::Ai(
                    "Gateway restart failed; run kc ai prepare outside the Gateway".into(),
                ));
            }
            Ok(())
        },
        api_ready,
    )
}

fn prepare_with(
    env_path: &Path,
    api_base: &str,
    mut enabled: impl FnMut() -> Result<bool>,
    mut enable: impl FnMut() -> Result<()>,
    mut restart: impl FnMut() -> Result<()>,
    ready: impl Fn(&str, &str) -> bool,
) -> Result<()> {
    let (key, created) = ensure_api_key(env_path)?;
    let already_enabled = enabled()?;
    if !already_enabled {
        enable()?;
    }
    if created || !already_enabled || !ready(api_base, &key) {
        restart()?;
        let deadline = Instant::now() + Duration::from_secs(15);
        while !ready(api_base, &key) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(300));
        }
    }
    if !ready(api_base, &key) {
        return Err(Error::Ai("Hermes API Server is not ready".into()));
    }
    Ok(())
}

pub fn gui_sibling(cli: &Path) -> Result<PathBuf> {
    let gui = cli
        .parent()
        .ok_or_else(|| Error::Ai("kc has no parent directory".into()))?
        .join("knowcraft");
    if !gui.is_file() {
        return Err(Error::Ai(format!(
            "GUI executable not found beside kc: {}",
            gui.display()
        )));
    }
    Ok(gui)
}

pub fn gui_command(cli: &Path, key: Option<&str>) -> Result<Command> {
    let gui = gui_sibling(cli)?;
    let mut command = Command::new("dbus-run-session");
    command.arg("--").arg(gui);
    if let Some(key) = key {
        command.env("KNOWCRAFT_HERMES_API_KEY", key);
    }
    Ok(command)
}

pub fn launch_gui(cli: &Path, env_path: &Path) -> Result<()> {
    let key = api_key(env_path)?
        .ok_or_else(|| Error::Ai("API_SERVER_KEY not set; run kc ai prepare first".into()))?;
    let status = gui_command(cli, Some(&key))?.status()?;
    if !status.success() {
        return Err(Error::Ai(format!("Knowcraft GUI exited with {status}")));
    }
    Ok(())
}

/// 不带 AI 支持的启动：不读取任何 Hermes 配置，仅以私有 D-Bus 会话启动 GUI。
pub fn launch_gui_plain(cli: &Path) -> Result<()> {
    let status = gui_command(cli, None)?.status()?;
    if !status.success() {
        return Err(Error::Ai(format!("Knowcraft GUI exited with {status}")));
    }
    Ok(())
}

pub fn api_ready(base: &str, key: &str) -> bool {
    let base = base.trim_end_matches('/');
    let Some(host) = base.strip_suffix("/v1") else {
        return false;
    };
    let Ok(client) = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(3))
        .build()
    else {
        return false;
    };
    let health = client
        .get(format!("{host}/health"))
        .send()
        .is_ok_and(|response| response.status().is_success());
    let models = client
        .get(format!("{base}/models"))
        .bearer_auth(key)
        .send()
        .is_ok_and(|response| response.status().is_success());
    health && models
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_creation_is_idempotent_and_preserves_other_settings() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join(".env");
        fs::write(&path, "# hello\nOTHER=keep\n").unwrap();
        let (key, created) = ensure_api_key(&path).unwrap();
        assert!(created);
        assert_eq!(key.len(), 64);
        assert_eq!(ensure_api_key(&path).unwrap(), (key.clone(), false));
        assert!(fs::read_to_string(&path).unwrap().contains("OTHER=keep"));
        assert!(api_key(&path).unwrap().as_deref() == Some(key.as_str()));
    }

    #[test]
    fn refuses_duplicate_or_short_keys() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join(".env");
        fs::write(&path, "API_SERVER_KEY=too-short\n").unwrap();
        assert!(api_key(&path).is_err());
        fs::write(&path, "API_SERVER_KEY=\"\n").unwrap();
        assert!(api_key(&path).is_err());
        fs::write(&path, "API_SERVER_KEY=\nAPI_SERVER_KEY=abcdefghijklmnop\n").unwrap();
        assert!(api_key(&path).is_err());
        fs::write(
            &path,
            "API_SERVER_KEY=abcdefghijklmnop\nAPI_SERVER_KEY=abcdefghijklmnop\n",
        )
        .unwrap();
        assert!(api_key(&path).is_err());
    }

    #[test]
    fn reuse_existing_key_restricts_file_permissions() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join(".env");
        fs::write(&path, "API_SERVER_KEY=abcdefghijklmnop\n").unwrap();
        assert_eq!(
            ensure_api_key(&path).unwrap(),
            ("abcdefghijklmnop".into(), false)
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }

    #[test]
    fn creates_a_private_env_file() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join(".env");
        assert!(ensure_api_key(&path).unwrap().1);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }

    #[test]
    fn prepare_restarts_only_when_needed() {
        let root = tempfile::tempdir().unwrap();
        let env = root.path().join(".env");
        fs::write(&env, "API_SERVER_KEY=abcdefghijklmnop\n").unwrap();
        let enables = std::cell::Cell::new(0);
        let restarts = std::cell::Cell::new(0);
        prepare_with(
            &env,
            "http://127.0.0.1:8642/v1",
            || Ok(true),
            || {
                enables.set(enables.get() + 1);
                Ok(())
            },
            || {
                restarts.set(restarts.get() + 1);
                Ok(())
            },
            |_, _| true,
        )
        .unwrap();
        assert_eq!((enables.get(), restarts.get()), (0, 0));

        prepare_with(
            &env,
            "http://127.0.0.1:8642/v1",
            || Ok(true),
            || {
                enables.set(enables.get() + 1);
                Ok(())
            },
            || {
                restarts.set(restarts.get() + 1);
                Ok(())
            },
            |_, _| restarts.get() != 0,
        )
        .unwrap();
        assert_eq!((enables.get(), restarts.get()), (0, 1));
    }

    #[test]
    fn prepare_enables_and_restarts_for_new_key() {
        let root = tempfile::tempdir().unwrap();
        let env = root.path().join(".env");
        let restarts = std::cell::Cell::new(0);
        let enables = std::cell::Cell::new(0);
        prepare_with(
            &env,
            "http://127.0.0.1:8642/v1",
            || Ok(false),
            || {
                enables.set(enables.get() + 1);
                Ok(())
            },
            || {
                restarts.set(restarts.get() + 1);
                Ok(())
            },
            |_, _| true,
        )
        .unwrap();
        assert_eq!((enables.get(), restarts.get()), (1, 1));
        assert!(api_key(&env).unwrap().is_some());
    }

    #[test]
    fn launch_uses_only_sibling_gui_and_private_bus() {
        let root = tempfile::tempdir().unwrap();
        let cli = root.path().join("kc");
        assert!(gui_command(&cli, None).is_err());
        assert!(gui_command(&cli, Some("secret")).is_err());
        fs::write(root.path().join("knowcraft"), "gui").unwrap();
        let command = gui_command(&cli, Some("secret")).unwrap();
        let args: Vec<_> = command.get_args().collect();
        assert_eq!(
            args,
            ["--", root.path().join("knowcraft").to_str().unwrap()]
        );
        assert_eq!(
            command
                .get_envs()
                .find(|(name, _)| *name == "KNOWCRAFT_HERMES_API_KEY")
                .unwrap()
                .1
                .unwrap(),
            "secret"
        );
    }

    #[test]
    fn launch_plain_sets_no_ai_variables() {
        let root = tempfile::tempdir().unwrap();
        let cli = root.path().join("kc");
        fs::write(root.path().join("knowcraft"), "gui").unwrap();
        let command = gui_command(&cli, None).unwrap();
        let args: Vec<_> = command.get_args().collect();
        assert_eq!(
            args,
            ["--", root.path().join("knowcraft").to_str().unwrap()]
        );
        assert!(command
            .get_envs()
            .find(|(name, _)| *name == "KNOWCRAFT_HERMES_API_KEY")
            .is_none());
    }
}
