use std::path::PathBuf;

use serde::Deserialize;
use snap_core::Options;

/// User settings: render options plus output behavior.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Settings {
    #[serde(flatten)]
    pub render: Options,
    pub show_filename: bool,
    pub output_dir: Option<String>,
    pub save_file: bool,
    pub copy_to_clipboard: bool,
    /// Open the saved PNG in Zed via `window/showDocument`.
    pub open_after_snap: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            render: Options::default(),
            show_filename: true,
            output_dir: None,
            save_file: true,
            copy_to_clipboard: true,
            open_after_snap: false,
        }
    }
}

impl Settings {
    /// Parses settings, tolerating `null` and invalid values (falls back to defaults).
    pub fn from_value(value: Option<serde_json::Value>) -> (Self, Option<String>) {
        match value {
            None | Some(serde_json::Value::Null) => (Self::default(), None),
            Some(v) => match serde_json::from_value(v) {
                Ok(s) => (s, None),
                Err(e) => (
                    Self::default(),
                    Some(format!("snapcode: invalid settings ({e}); using defaults")),
                ),
            },
        }
    }

    pub fn output_dir(&self) -> PathBuf {
        match self.output_dir.as_deref() {
            Some(dir) if !dir.trim().is_empty() => expand_home(dir),
            _ => dirs::picture_dir()
                .or_else(dirs::home_dir)
                .unwrap_or_else(std::env::temp_dir)
                .join("snapcode"),
        }
    }
}

fn expand_home(path: &str) -> PathBuf {
    if let Some(rest) = path
        .strip_prefix("~/")
        .or_else(|| path.strip_prefix("~\\"))
        .or((path == "~").then_some(""))
    {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
    }
    PathBuf::from(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_flat_settings() {
        let (s, err) = Settings::from_value(Some(serde_json::json!({
            "theme": "Nord", "line_numbers": true, "copy_to_clipboard": false, "output_dir": "~/snaps"
        })));
        assert!(err.is_none());
        assert_eq!(s.render.theme, "Nord");
        assert!(s.render.line_numbers);
        assert!(!s.copy_to_clipboard);
        assert!(s.output_dir().ends_with("snaps"));
    }

    #[test]
    fn invalid_settings_fall_back() {
        let (s, err) = Settings::from_value(Some(serde_json::json!({ "padding": "lots" })));
        assert!(err.is_some());
        assert_eq!(s.render.padding, 48);
    }
}
