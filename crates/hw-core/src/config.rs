//! Config directory resolution and `config.json` loading.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// Environment variable that overrides the config directory.
pub const CONFIG_ENV: &str = "HYPRWIDGET_CONFIG";
/// Lua entry point inside the config directory.
pub const ENTRY_FILE: &str = "hyprwidget.lua";
/// Optional JSON config inside the config directory.
pub const JSON_FILE: &str = "config.json";

/// Inputs for [`resolve_dir_from`], split out so resolution is testable
/// without touching the process environment.
#[derive(Debug, Default, Clone)]
pub struct DirSources {
    /// `-c <dir>` from the command line.
    pub cli: Option<PathBuf>,
    /// `$HYPRWIDGET_CONFIG`.
    pub env: Option<PathBuf>,
    /// `$XDG_CONFIG_HOME`.
    pub xdg_config_home: Option<PathBuf>,
    /// `$HOME`.
    pub home: Option<PathBuf>,
}

impl DirSources {
    /// Reads the environment for everything except `cli`.
    pub fn from_env(cli: Option<PathBuf>) -> Self {
        let var = |k| {
            std::env::var_os(k)
                .filter(|v| !v.is_empty())
                .map(PathBuf::from)
        };
        Self {
            cli,
            env: var(CONFIG_ENV),
            xdg_config_home: var("XDG_CONFIG_HOME"),
            home: var("HOME"),
        }
    }
}

/// Picks the config directory: `-c` > `$HYPRWIDGET_CONFIG` >
/// `$XDG_CONFIG_HOME/hyprwidget` > `~/.config/hyprwidget`.
pub fn resolve_dir_from(src: DirSources) -> Result<PathBuf> {
    if let Some(dir) = src.cli.or(src.env) {
        return Ok(dir);
    }
    // XDG spec: a relative $XDG_CONFIG_HOME is invalid and must be ignored.
    if let Some(xdg) = src.xdg_config_home.filter(|p| p.is_absolute()) {
        return Ok(xdg.join("hyprwidget"));
    }
    let home = src
        .home
        .context("cannot locate config dir: $HOME and $XDG_CONFIG_HOME are unset")?;
    Ok(home.join(".config").join("hyprwidget"))
}

/// Resolved config directory with its well-known files.
#[derive(Debug, Clone)]
pub struct ConfigDir {
    pub root: PathBuf,
}

impl ConfigDir {
    /// Resolves the directory from `cli` and the environment, makes it
    /// absolute and checks that it exists.
    pub fn resolve(cli: Option<PathBuf>) -> Result<Self> {
        let dir = resolve_dir_from(DirSources::from_env(cli))?;
        let root = std::path::absolute(&dir)
            .with_context(|| format!("invalid config dir {}", dir.display()))?;
        anyhow::ensure!(
            root.is_dir(),
            "config dir {} does not exist",
            root.display()
        );
        Ok(Self { root })
    }

    pub fn entry(&self) -> PathBuf {
        self.root.join(ENTRY_FILE)
    }

    pub fn json(&self) -> PathBuf {
        self.root.join(JSON_FILE)
    }

    /// Parses `config.json`; a missing file yields `Value::Null`.
    pub fn load_json(&self) -> Result<serde_json::Value> {
        load_json(&self.json())
    }

    /// Resolves a path from the config: `~/` is expanded, relative paths are
    /// relative to the config dir.
    pub fn path(&self, p: &str) -> PathBuf {
        match p.strip_prefix("~/") {
            Some(rest) => std::env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_default()
                .join(rest),
            None => self.root.join(p),
        }
    }

    /// Loads `config.json` into the app settings and the option layer.
    pub fn load_app(&self) -> Result<AppConfig> {
        AppConfig::from_json(self, &self.load_json()?)
    }
}

/// Keys of `config.json` that configure the app itself; every other key is
/// an option (same tree as `hw.config{}`).
pub const APP_KEYS: &[&str] = &[
    "settings_file",
    "style",
    "layer_rules",
    "theme",
    "plugins",
    "monitors",
    "hypr_custom_dir",
];

/// App-level settings from `config.json`.
#[derive(Debug, Clone)]
pub struct AppConfig {
    /// AGS-compatible settings store.
    pub settings_file: PathBuf,
    /// SCSS/CSS entry point.
    pub style: PathBuf,
    /// Register Hyprland layer rules (blur…) for our namespaces.
    pub layer_rules: bool,
    /// Option overrides from JSON (nested; flat keys expanded).
    pub options: serde_json::Value,
    /// Where `hw.set_option` writes Hyprland options (`<option>.lua` with
    /// `hl.config{}`, same as AGS). Default `~/.config/hypr/config/custom`.
    pub hypr_custom_dir: PathBuf,
}

impl AppConfig {
    pub fn from_json(dir: &ConfigDir, v: &serde_json::Value) -> Result<Self> {
        let empty = serde_json::Map::new();
        let obj = match v {
            serde_json::Value::Null => &empty,
            serde_json::Value::Object(o) => o,
            _ => anyhow::bail!("{}: top level must be an object", dir.json().display()),
        };
        let str_key = |k: &str, def: &str| -> Result<PathBuf> {
            match obj.get(k) {
                None => Ok(dir.path(def)),
                Some(serde_json::Value::String(s)) => Ok(dir.path(s)),
                Some(other) => anyhow::bail!("config.json: '{k}' must be a string, got {other}"),
            }
        };
        let options: serde_json::Map<_, _> = obj
            .iter()
            .filter(|(k, _)| !APP_KEYS.contains(&k.as_str()))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        Ok(Self {
            settings_file: str_key("settings_file", "cache/settings/settings.json")?,
            style: str_key("style", "style/main.scss")?,
            layer_rules: obj
                .get("layer_rules")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(true),
            options: crate::options::expand_keys(&serde_json::Value::Object(options)),
            hypr_custom_dir: str_key("hypr_custom_dir", "~/.config/hypr/config/custom")?,
        })
    }
}

fn load_json(path: &Path) -> Result<serde_json::Value> {
    match std::fs::read_to_string(path) {
        Ok(s) => serde_json::from_str(&s).with_context(|| format!("parsing {}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(serde_json::Value::Null),
        Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn src(
        cli: Option<&str>,
        env: Option<&str>,
        xdg: Option<&str>,
        home: Option<&str>,
    ) -> DirSources {
        DirSources {
            cli: cli.map(Into::into),
            env: env.map(Into::into),
            xdg_config_home: xdg.map(Into::into),
            home: home.map(Into::into),
        }
    }

    fn resolve(s: DirSources) -> PathBuf {
        resolve_dir_from(s).unwrap()
    }

    #[test]
    fn cli_wins() {
        let s = src(Some("a"), Some("/b"), Some("/x"), Some("/h"));
        assert_eq!(resolve(s), PathBuf::from("a"));
    }

    #[test]
    fn env_beats_xdg() {
        let s = src(None, Some("/b"), Some("/x"), Some("/h"));
        assert_eq!(resolve(s), PathBuf::from("/b"));
    }

    #[test]
    fn xdg_beats_home() {
        let s = src(None, None, Some("/x"), Some("/h"));
        assert_eq!(resolve(s), PathBuf::from("/x/hyprwidget"));
    }

    #[test]
    fn relative_xdg_ignored() {
        let s = src(None, None, Some("rel"), Some("/h"));
        assert_eq!(resolve(s), PathBuf::from("/h/.config/hyprwidget"));
    }

    #[test]
    fn nothing_set_errors() {
        assert!(resolve_dir_from(DirSources::default()).is_err());
    }

    #[test]
    fn demo_example_resolves() {
        let demo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/demo");
        let dir = ConfigDir::resolve(Some(demo)).unwrap();
        assert!(dir.entry().is_file());
        assert!(dir.load_json().unwrap().is_object());
    }

    #[test]
    fn missing_json_is_null() {
        assert!(
            load_json(Path::new("/nonexistent/config.json"))
                .unwrap()
                .is_null()
        );
    }
}
