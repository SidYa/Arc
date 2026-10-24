//! Settings in %APPDATA%\Arc\config.json.

use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct Config {
    pub language: String,
    pub theme: String,  // system | light | dark
    pub accent: String, // purple | green | orange | blue | system | #rrggbb
    pub monochrome: bool,
    /// Arc submenu in the Explorer context menu
    pub context_menu: bool,
    /// When an extracted file already exists: rename | overwrite | skip
    pub conflict: String,
    pub default_format: String,
    /// 0 store … 9 ultra
    pub default_level: u32,
    /// Open the folder in Explorer when an extraction finishes
    pub open_folder_after: bool,
    /// Last window size in logical pixels
    #[serde(skip_serializing_if = "Option::is_none")]
    pub window_width: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub window_height: Option<f64>,
}

impl Default for Config {
    fn default() -> Self {
        let locale = sys_locale::get_locale().unwrap_or_default().to_lowercase();
        Config {
            language: if locale.starts_with("uk") { "uk".into() } else { "en".into() },
            theme: "system".into(),
            accent: "purple".into(),
            monochrome: false,
            context_menu: true,
            conflict: "rename".into(),
            default_format: "zip".into(),
            default_level: 5,
            open_folder_after: false,
            window_width: None,
            window_height: None,
        }
    }
}

pub fn config_path() -> PathBuf {
    let base = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
    if cfg!(windows) { base.join("Arc").join("config.json") } else { base.join("arc.json") }
}

pub fn data_dir() -> PathBuf {
    let base = dirs::data_local_dir().unwrap_or_else(|| PathBuf::from("."));
    base.join(if cfg!(windows) { "Arc" } else { "arc" })
}

/// Files opened from archives are extracted here; cleaned on start.
pub fn temp_dir() -> PathBuf {
    std::env::temp_dir().join("Arc")
}

impl Config {
    pub fn load() -> Config {
        match fs::read_to_string(config_path()) {
            Ok(text) => serde_json::from_str(&text).unwrap_or_else(|e| {
                crate::log(&format!("Error loading config: {e}"));
                Config::default()
            }),
            Err(_) => Config::default(),
        }
    }

    pub fn save(&self) {
        let path = config_path();
        let result = (|| -> std::io::Result<()> {
            if let Some(dir) = path.parent() {
                fs::create_dir_all(dir)?;
            }
            let tmp = path.with_extension("json.tmp");
            fs::write(&tmp, serde_json::to_vec_pretty(self)?)?;
            fs::rename(&tmp, &path)
        })();
        if let Err(e) = result {
            crate::log(&format!("Error saving config: {e}"));
        }
    }
}
