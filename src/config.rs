use serde::Deserialize;
use std::{fs, path::PathBuf};

#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct Config {
    pub refresh_interval: u64,
    pub projects: ProjectsConfig,
    pub ui: UiConfig,
    pub actions: ActionsConfig,
    pub ports: PortsConfig,
    pub waybar: WaybarConfig,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct ProjectsConfig {
    pub search_git_root: bool,
}
#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct UiConfig {
    pub provider: String,
}
#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct ActionsConfig {
    pub terminal: String,
    pub editor: String,
}
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct PortsConfig {
    pub ignore: Vec<u16>,
}
#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct WaybarConfig {
    pub icon: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            refresh_interval: 3,
            projects: ProjectsConfig::default(),
            ui: UiConfig::default(),
            actions: ActionsConfig::default(),
            ports: PortsConfig::default(),
            waybar: WaybarConfig::default(),
        }
    }
}
impl Default for ProjectsConfig {
    fn default() -> Self {
        Self {
            search_git_root: true,
        }
    }
}
impl Default for UiConfig {
    fn default() -> Self {
        Self {
            provider: "auto".into(),
        }
    }
}
impl Default for ActionsConfig {
    fn default() -> Self {
        Self {
            terminal: "kitty".into(),
            editor: "nvim".into(),
        }
    }
}
impl Default for WaybarConfig {
    fn default() -> Self {
        Self {
            icon: "󰖟".into()
        }
    }
}

impl Config {
    pub fn load() -> anyhow::Result<Self> {
        let path = config_path();
        let Ok(contents) = fs::read_to_string(path) else {
            return Ok(Self::default());
        };
        toml::from_str(&contents)
            .map_err(|error| anyhow::anyhow!("invalid PortPilot config: {error}"))
    }
}

pub fn config_path() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".config")
        })
        .join("portpilot/config.toml")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_usable_without_a_config_file() {
        let config = Config::default();
        assert_eq!(config.refresh_interval, 3);
        assert_eq!(config.ui.provider, "auto");
        assert_eq!(config.waybar.icon, "󰖟");
    }

    #[test]
    fn parses_partial_toml_config() {
        let config: Config = toml::from_str(
            "refresh_interval = 5\n[ports]\nignore = [53, 631]\n[waybar]\nicon = 'PP'",
        )
        .unwrap();
        assert_eq!(config.refresh_interval, 5);
        assert_eq!(config.ports.ignore, vec![53, 631]);
        assert_eq!(config.waybar.icon, "PP");
        assert_eq!(config.actions.terminal, "kitty");
    }
}
