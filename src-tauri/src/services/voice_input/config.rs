use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::path::Path;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ApiOptions {
    pub protocol: String,
}

impl Default for ApiOptions {
    fn default() -> Self {
        Self {
            protocol: "openai".into(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase")]
pub struct Channel {
    pub kind: String,
    pub id: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Binding {
    pub code: String,
    pub modifiers: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Config {
    pub channels: Vec<Channel>,
    pub api_options: BTreeMap<String, ApiOptions>,
    pub microphone: Option<String>,
    pub shortcut_enabled: bool,
    pub hotkey_enabled: bool,
    pub hotkey_mode: String,
    pub shortcut: Binding,
    pub island_theme: String,
    pub island_palette: String,
    pub island_scale: u32,
    #[serde(alias = "optimizationModel")]
    pub selection_model: Option<String>,
    pub selection_translation_enabled: bool,
    pub selection_translation_language: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            channels: Vec::new(),
            api_options: BTreeMap::new(),
            microphone: None,
            shortcut_enabled: false,
            hotkey_enabled: false,
            hotkey_mode: "hold".into(),
            shortcut: Binding {
                code: "AltRight".into(),
                modifiers: Vec::new(),
            },
            island_theme: "auto".into(),
            island_palette: "white".into(),
            island_scale: 100,
            selection_model: None,
            selection_translation_enabled: false,
            selection_translation_language: "zh-Hans".into(),
        }
    }
}

pub(super) fn language_name(language: &str) -> Option<&'static str> {
    match language {
        "zh-Hans" => Some("Simplified Chinese (use simplified characters)"),
        "zh-Hant" => Some("Traditional Chinese (use traditional characters)"),
        "en" => Some("English"),
        "ja" => Some("Japanese"),
        "ko" => Some("Korean"),
        "fr" => Some("French"),
        "de" => Some("German"),
        "es" => Some("Spanish"),
        _ => None,
    }
}

impl Config {
    pub fn hotkey_active(&self) -> bool {
        self.shortcut_enabled && self.hotkey_enabled
    }

    pub fn selection_active(&self) -> bool {
        self.selection_translation_enabled
            && self
                .selection_model
                .as_ref()
                .is_some_and(|id| !id.is_empty())
    }

    pub fn normalize_selection(&mut self) {
        if self.selection_model.as_ref().is_none_or(|id| id.is_empty()) {
            self.selection_model = None;
            self.selection_translation_enabled = false;
        }
    }

    /// The island's own zoom, independent of the main window's interface scale.
    pub fn island_zoom(&self) -> f64 {
        f64::from(self.island_scale) / 100.0
    }

    pub fn disable_without_channels(&mut self) {
        if self.channels.is_empty() {
            self.shortcut_enabled = false;
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if !matches!(self.hotkey_mode.as_str(), "hold" | "toggle") {
            return Err("voice.error.config".into());
        }
        if language_name(&self.selection_translation_language).is_none() {
            return Err("voice.error.config".into());
        }
        if self.api_options.iter().any(|(id, options)| {
            id.is_empty()
                || id.len() > 512
                || !matches!(
                    options.protocol.as_str(),
                    "openai"
                        | "chatAudio"
                        | "mimo"
                        | "openrouter"
                        | "gemini"
                        | "doubao"
                        | "minimax"
                )
        }) {
            return Err("voice.error.config".into());
        }
        if self.channels.len() > 3
            || self.channels.iter().collect::<HashSet<_>>().len() != self.channels.len()
            || self.channels.iter().any(|channel| {
                channel.id.is_empty()
                    || channel.id.len() > 512
                    || !matches!(channel.kind.as_str(), "workbuddy" | "account" | "model")
                    || (channel.kind == "account"
                        && crate::services::account_proxy::voice::parse_selector(&channel.id)
                            .is_err())
                    || (channel.kind == "workbuddy"
                        && (channel.id.len() != 64
                            || !channel.id.bytes().all(|byte| byte.is_ascii_hexdigit())))
            })
        {
            return Err("voice.error.channels".into());
        }
        if !matches!(
            self.island_theme.as_str(),
            "auto" | "dark" | "light" | "system" | "iridescent"
        ) || !matches!(
            self.island_palette.as_str(),
            "white" | "aurora" | "ocean" | "sunset" | "iris"
        ) || !(70..=150).contains(&self.island_scale)
            || !self.island_scale.is_multiple_of(10)
        {
            return Err("voice.error.config".into());
        }
        let modifiers = &self.shortcut.modifiers;
        if self.shortcut.code.is_empty()
            || modifiers.iter().collect::<HashSet<_>>().len() != modifiers.len()
            || modifiers
                .iter()
                .any(|key| !matches!(key.as_str(), "Control" | "Alt" | "Shift" | "Meta"))
        {
            return Err("voice.error.shortcut".into());
        }
        Ok(())
    }
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Usage {
    pub characters: u64,
    pub duration_ms: u64,
}

impl Usage {
    pub fn record(&mut self, characters: u64, duration_ms: u64) {
        if characters > 0 && duration_ms > 0 {
            self.characters += characters;
            self.duration_ms += duration_ms;
        }
    }
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Saved {
    pub config: Config,
    pub days: BTreeMap<String, u64>,
    pub usage: Usage,
}

pub fn load(path: &Path) -> Result<Saved, String> {
    match std::fs::read(path) {
        Ok(bytes) => {
            let mut saved: Saved =
                serde_json::from_slice(&bytes).map_err(|_| "voice.error.config")?;
            saved.config.normalize_selection();
            saved.config.validate()?;
            saved.config.disable_without_channels();
            Ok(saved)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Saved::default()),
        Err(_) => Err("voice.error.config".into()),
    }
}

pub fn save(path: &Path, saved: &Saved) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|_| "voice.error.save")?;
    }
    let temporary = path.with_extension("tmp");
    let bytes = serde_json::to_vec_pretty(saved).map_err(|_| "voice.error.save")?;
    std::fs::write(&temporary, bytes)
        .and_then(|()| std::fs::rename(&temporary, path))
        .map_err(|_| "voice.error.save".into())
}
