use serde::Deserialize;
use std::path::PathBuf;

pub const DEFAULT_BASE_URL: &str = "https://ranksyapp.com/api/v1";

#[derive(Debug, Default, Deserialize)]
pub struct ConfigFile {
    pub api_key: Option<String>,
    pub app: Option<String>,
    pub base_url: Option<String>,
}

#[derive(Debug)]
pub struct Resolved {
    pub api_key: String,
    pub base_url: String,
    pub app: Option<String>,
}

#[derive(Debug)]
#[allow(dead_code)] // Parse variant is part of the public config API
pub enum ConfigError {
    MissingKey,
    Io(std::io::Error),
    Parse(String),
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigError::MissingKey => write!(f, "no API key found. Run `ranksy login <key>`, set RANKSY_API_KEY, or pass --api-key."),
            ConfigError::Io(e) => write!(f, "config io error: {e}"),
            ConfigError::Parse(e) => write!(f, "config parse error: {e}"),
        }
    }
}
impl std::error::Error for ConfigError {}

pub fn resolve(
    flag_key: Option<String>,
    env_key: Option<String>,
    flag_base: Option<String>,
    env_base: Option<String>,
    flag_app: Option<String>,
    file: &ConfigFile,
) -> Result<Resolved, ConfigError> {
    let api_key = flag_key
        .or(env_key)
        .or_else(|| file.api_key.clone())
        .ok_or(ConfigError::MissingKey)?;
    let base_url = flag_base
        .or(env_base)
        .or_else(|| file.base_url.clone())
        .unwrap_or_else(|| DEFAULT_BASE_URL.to_string());
    let app = flag_app.or_else(|| file.app.clone());
    Ok(Resolved { api_key, base_url, app })
}

pub fn config_path() -> PathBuf {
    let dirs = directories::ProjectDirs::from("com", "ranksy", "ranksy");
    match dirs {
        Some(d) => d.config_dir().join("config.toml"),
        None => PathBuf::from(".ranksy/config.toml"),
    }
}

pub fn load() -> ConfigFile {
    let path = config_path();
    match std::fs::read_to_string(&path) {
        Ok(s) => toml::from_str(&s).unwrap_or_default(),
        Err(_) => ConfigFile::default(),
    }
}

pub fn save_api_key(key: &str) -> Result<(), ConfigError> {
    let path = config_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(ConfigError::Io)?;
    }
    let mut existing = load();
    existing.api_key = Some(key.to_string());
    let body = format!(
        "api_key = \"{}\"\n{}{}",
        key,
        existing.app.map(|a| format!("app = \"{a}\"\n")).unwrap_or_default(),
        existing.base_url.map(|b| format!("base_url = \"{b}\"\n")).unwrap_or_default(),
    );
    std::fs::write(&path, body).map_err(ConfigError::Io)?;
    set_permissions_0600(&path)?;
    Ok(())
}

#[cfg(unix)]
fn set_permissions_0600(path: &PathBuf) -> Result<(), ConfigError> {
    use std::os::unix::fs::PermissionsExt;
    let perms = std::fs::Permissions::from_mode(0o600);
    std::fs::set_permissions(path, perms).map_err(ConfigError::Io)
}

#[cfg(not(unix))]
fn set_permissions_0600(_path: &PathBuf) -> Result<(), ConfigError> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file() -> ConfigFile {
        ConfigFile { api_key: Some("file-key".into()), app: Some("app-file".into()), base_url: None }
    }

    #[test]
    fn flag_beats_env_beats_file() {
        let r = resolve(Some("flag".into()), Some("env".into()), None, None, None, &file()).unwrap();
        assert_eq!(r.api_key, "flag");
    }

    #[test]
    fn env_beats_file() {
        let r = resolve(None, Some("env".into()), None, None, None, &file()).unwrap();
        assert_eq!(r.api_key, "env");
    }

    #[test]
    fn file_used_when_no_flag_or_env() {
        let r = resolve(None, None, None, None, None, &file()).unwrap();
        assert_eq!(r.api_key, "file-key");
        assert_eq!(r.app.as_deref(), Some("app-file"));
    }

    #[test]
    fn missing_key_errors() {
        let empty = ConfigFile { api_key: None, app: None, base_url: None };
        assert!(matches!(resolve(None, None, None, None, None, &empty), Err(ConfigError::MissingKey)));
    }

    #[test]
    fn base_url_defaults() {
        let r = resolve(Some("k".into()), None, None, None, None, &file()).unwrap();
        assert_eq!(r.base_url, DEFAULT_BASE_URL);
    }
}
