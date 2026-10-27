use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const DEFAULT_BASE_URL: &str = "https://ranksyapp.com/api/v1";

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConfigFile {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    /// Shopify App Store ads sync, written by `ranksy ads connect`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ads: Option<AdsConfig>,
    /// Keys this version doesn't know about, kept so rewriting the file never drops them.
    #[serde(flatten)]
    pub extra: BTreeMap<String, toml::Value>,
}

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(from = "RawAdsConfig")]
pub struct AdsConfig {
    /// Connected Shopify Partners organization ids; `ranksy ads sync` syncs all of them.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub orgs: Vec<String>,
    /// Chrome profile dir; only stored when it differs from the default.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chrome_profile: Option<String>,
    /// Chrome debug port; only stored when it differs from the default.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    /// Shopify app (Partners app API key) -> Ranksy app.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub apps: BTreeMap<String, AdsApp>,
}

/// What `[ads]` looks like on disk: the same table plus the legacy single `org` key.
#[derive(Deserialize)]
struct RawAdsConfig {
    #[serde(default)]
    org: Option<String>,
    #[serde(default)]
    orgs: Vec<String>,
    #[serde(default)]
    chrome_profile: Option<String>,
    #[serde(default)]
    port: Option<u16>,
    #[serde(default)]
    apps: BTreeMap<String, AdsApp>,
}

impl From<RawAdsConfig> for AdsConfig {
    /// Folds the legacy `org = "..."` into `orgs`; it is never written back.
    fn from(raw: RawAdsConfig) -> Self {
        let mut config = AdsConfig {
            orgs: Vec::new(),
            chrome_profile: raw.chrome_profile,
            port: raw.port,
            apps: raw.apps,
        };
        for org in raw.orgs.into_iter().chain(raw.org) {
            config.add_org(&org);
        }

        config
    }
}

impl AdsConfig {
    /// Adds a Partners organization unless it is already connected.
    pub fn add_org(&mut self, org: &str) {
        if !org.is_empty() && !self.orgs.iter().any(|o| o == org) {
            self.orgs.push(org.to_string());
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AdsApp {
    /// Ranksy app ulid the ads are imported into.
    pub ulid: String,
    /// Shopify app name, for humans reading the file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Partners organization the app's ads live in. Mappings written before multi-org
    /// support have none; they belong to the single org that was connected then.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub org: Option<String>,
}

impl AdsApp {
    /// Whether this app can be synced from `org`: a legacy mapping without an org matches any.
    pub fn in_org(&self, org: &str) -> bool {
        self.org.as_deref().is_none_or(|o| o == org)
    }
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
    Ok(Resolved {
        api_key,
        base_url,
        app,
    })
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

/// Reads the config for a read-modify-write: a missing file is empty, but a file that
/// doesn't parse is an error, so saving never overwrites settings we couldn't read.
fn load_for_update(path: &Path) -> Result<ConfigFile, ConfigError> {
    match std::fs::read_to_string(path) {
        Ok(s) => {
            toml::from_str(&s).map_err(|e| ConfigError::Parse(format!("{}: {e}", path.display())))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(ConfigFile::default()),
        Err(e) => Err(ConfigError::Io(e)),
    }
}

/// Serializes `file` to `path` (0600 on unix), creating the directory if needed.
fn write_file(path: &Path, file: &ConfigFile) -> Result<(), ConfigError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(ConfigError::Io)?;
    }
    let body = toml::to_string(file).map_err(|e| ConfigError::Parse(e.to_string()))?;
    std::fs::write(path, body).map_err(ConfigError::Io)?;
    set_permissions_0600(path)
}

pub fn save_api_key(key: &str) -> Result<(), ConfigError> {
    let path = config_path();
    let mut file = load_for_update(&path)?;
    file.api_key = Some(key.to_string());
    write_file(&path, &file)
}

/// Replaces the `[ads]` table, leaving every other setting as it was.
pub fn save_ads(ads: AdsConfig) -> Result<(), ConfigError> {
    let path = config_path();
    let mut file = load_for_update(&path)?;
    file.ads = Some(ads);
    write_file(&path, &file)
}

#[cfg(unix)]
fn set_permissions_0600(path: &Path) -> Result<(), ConfigError> {
    use std::os::unix::fs::PermissionsExt;
    let perms = std::fs::Permissions::from_mode(0o600);
    std::fs::set_permissions(path, perms).map_err(ConfigError::Io)
}

#[cfg(not(unix))]
fn set_permissions_0600(_path: &Path) -> Result<(), ConfigError> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file() -> ConfigFile {
        ConfigFile {
            api_key: Some("file-key".into()),
            app: Some("app-file".into()),
            ..Default::default()
        }
    }

    #[test]
    fn flag_beats_env_beats_file() {
        let r = resolve(
            Some("flag".into()),
            Some("env".into()),
            None,
            None,
            None,
            &file(),
        )
        .unwrap();
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
        let empty = ConfigFile::default();
        assert!(matches!(
            resolve(None, None, None, None, None, &empty),
            Err(ConfigError::MissingKey)
        ));
    }

    #[test]
    fn base_url_defaults() {
        let r = resolve(Some("k".into()), None, None, None, None, &file()).unwrap();
        assert_eq!(r.base_url, DEFAULT_BASE_URL);
    }

    fn ads() -> AdsConfig {
        AdsConfig {
            orgs: vec!["1234567".into(), "7654321".into()],
            chrome_profile: None,
            port: Some(9444),
            apps: BTreeMap::from([
                (
                    "abc123".to_string(),
                    AdsApp {
                        ulid: "01JABCDEF".into(),
                        name: Some("Block Country".into()),
                        org: Some("1234567".into()),
                    },
                ),
                (
                    "def456".to_string(),
                    AdsApp {
                        ulid: "01JXYZ".into(),
                        name: None,
                        org: Some("7654321".into()),
                    },
                ),
            ]),
        }
    }

    #[test]
    fn ads_round_trip_keeps_existing_keys() {
        let original = ConfigFile {
            api_key: Some("rk_live_x".into()),
            app: Some("01HAPP".into()),
            base_url: Some("https://example.test/api/v1".into()),
            ads: Some(ads()),
            ..Default::default()
        };
        let text = toml::to_string(&original).unwrap();
        assert!(text.contains("api_key = \"rk_live_x\""));
        assert!(text.contains("orgs = [\"1234567\", \"7654321\"]"));
        assert!(text.contains("[ads.apps.abc123]"));
        assert!(text.contains("org = \"7654321\""));
        assert!(!text.contains("chrome_profile"));

        let parsed: ConfigFile = toml::from_str(&text).unwrap();
        assert_eq!(parsed, original);
    }

    #[test]
    fn parses_hand_written_ads_table() {
        let parsed: ConfigFile = toml::from_str(
            "api_key = \"k\"\n[ads]\norg = \"42\"\n[ads.apps.key1]\nulid = \"01A\"\nname = \"One\"\n[ads.apps.key2]\nulid = \"01B\"\n",
        )
        .unwrap();
        let ads = parsed.ads.unwrap();
        assert_eq!(ads.orgs, ["42"]);
        assert_eq!(
            ads.apps["key1"],
            AdsApp {
                ulid: "01A".into(),
                name: Some("One".into()),
                org: None
            }
        );
        assert_eq!(ads.apps["key2"].name, None);
        assert_eq!(parsed.api_key.as_deref(), Some("k"));
    }

    #[test]
    fn legacy_org_loads_as_orgs_and_is_not_written_back() {
        let parsed: ConfigFile =
            toml::from_str("[ads]\norg = \"1\"\n[ads.apps.key1]\nulid = \"01A\"\n").unwrap();
        let ads = parsed.ads.as_ref().unwrap();
        assert_eq!(ads.orgs, ["1"]);
        assert_eq!(ads.apps["key1"].org, None);
        assert!(ads.apps["key1"].in_org("1"));

        let text = toml::to_string(&parsed).unwrap();
        assert!(text.contains("orgs = [\"1\"]"));
        assert!(!text.contains("\norg ="));
        assert!(!text.starts_with("org ="));
    }

    #[test]
    fn legacy_org_merges_with_orgs_without_duplicates() {
        let parsed: ConfigFile =
            toml::from_str("[ads]\norg = \"1\"\norgs = [\"2\", \"1\", \"2\"]\n").unwrap();
        assert_eq!(parsed.ads.unwrap().orgs, ["2", "1"]);
    }

    #[test]
    fn apps_match_their_own_org_and_legacy_apps_match_any() {
        let scoped = AdsApp {
            ulid: "01A".into(),
            name: None,
            org: Some("1".into()),
        };
        let legacy = AdsApp {
            ulid: "01B".into(),
            name: None,
            org: None,
        };
        assert!(scoped.in_org("1") && !scoped.in_org("2"));
        assert!(legacy.in_org("1") && legacy.in_org("2"));
    }

    #[test]
    fn config_without_ads_still_loads_and_unknown_keys_survive() {
        let parsed: ConfigFile =
            toml::from_str("api_key = \"k\"\ntheme = \"dark\"\n[other]\nx = 1\n").unwrap();
        assert!(parsed.ads.is_none());
        let again: ConfigFile = toml::from_str(&toml::to_string(&parsed).unwrap()).unwrap();
        assert_eq!(again, parsed);
        assert_eq!(again.extra["theme"].as_str(), Some("dark"));
    }

    #[test]
    fn saving_ads_keeps_api_key_on_disk_with_0600() {
        let dir = std::env::temp_dir().join(format!("ranksy-config-test-{}", std::process::id()));
        let path = dir.join("config.toml");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            &path,
            "api_key = \"keep-me\"\nbase_url = \"https://b.test/api/v1\"\n",
        )
        .unwrap();

        let mut loaded = load_for_update(&path).unwrap();
        loaded.ads = Some(ads());
        write_file(&path, &loaded).unwrap();

        let reread = load_for_update(&path).unwrap();
        assert_eq!(reread.api_key.as_deref(), Some("keep-me"));
        assert_eq!(reread.base_url.as_deref(), Some("https://b.test/api/v1"));
        assert_eq!(reread.ads, Some(ads()));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn unparseable_config_is_not_overwritten() {
        let dir = std::env::temp_dir().join(format!("ranksy-config-bad-{}", std::process::id()));
        let path = dir.join("config.toml");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&path, "api_key = [oops").unwrap();
        assert!(matches!(load_for_update(&path), Err(ConfigError::Parse(_))));
        assert!(
            matches!(load_for_update(&dir.join("missing.toml")), Ok(f) if f == ConfigFile::default())
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
