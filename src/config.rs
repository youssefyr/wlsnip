use serde::Deserialize;
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Deserialize, Default)]
pub struct Config {
    #[serde(default)]
    pub allow_upload: bool,
    pub padding: Option<u32>,
    pub shadow: Option<bool>,
    pub selection_color: Option<String>,
    pub ignore_apps: Option<Vec<String>>,
    pub format: Option<String>,
    pub clipboard: Option<bool>,
    pub no_cursor: Option<bool>,
    pub jpeg_quality: Option<u8>,
}

impl Config {
    pub fn load() -> Self {
        let config_path = Self::get_config_path();
        if let Some(path) = config_path {
            if path.exists() {
                if let Ok(content) = fs::read_to_string(&path) {
                    return toml::from_str(&content).unwrap_or_else(|e| {
                        eprintln!("wlsnip: failed to parse config file: {}", e);
                        Config::default()
                    });
                }
            }
        }
        Config::default()
    }

    fn get_config_path() -> Option<PathBuf> {
        let home = std::env::var("HOME").ok()?;
        Some(PathBuf::from(home).join(".config").join("wlsnip").join("config.toml"))
    }
}
