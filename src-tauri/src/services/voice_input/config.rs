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
pub struct TextOutput {
    pub enabled: bool,
    pub collapse_spaces: bool,
    pub prefix: String,
}
impl Default for TextOutput {
    fn default() -> Self {
        Self {
            enabled: false,
            collapse_spaces: true,
            prefix: String::new(),
        }
    }
}
impl TextOutput {
    pub fn format(&self, input: &str) -> String {
        let text =
            input.trim_matches(|c: char| (c.is_whitespace() && c != '\u{0085}') || c == '\u{feff}');
        if self.collapse_spaces {
            let mut space = false;
            let mut output = self.prefix.clone();
            for character in text.chars() {
                if matches!(character, ' ' | '\t') {
                    if !space {
                        output.push(' ');
                    }
                    space = true;
                } else {
                    space = false;
                    output.push(character);
                }
            }
            output
        } else {
            format!("{}{text}", self.prefix)
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Config {
    pub text_output: TextOutput,
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
            text_output: TextOutput::default(),
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
    pub builtin_text_tools_migrated: bool,
    pub format_invocations: u64,
    pub config: Config,
    pub days: BTreeMap<String, u64>,
    pub usage: Usage,
}

pub fn load(path: &Path) -> Result<Saved, String> {
    let mut saved: Saved = match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|_| "voice.error.config")?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Saved::default(),
        Err(_) => return Err("voice.error.config".into()),
    };
    if let Err(error) = migrate_text_output(path, &mut saved) {
        log::warn!("[VoiceInput] Legacy text preferences import failed: {error}");
    }
    saved.config.normalize_selection();
    saved.config.validate()?;
    saved.config.disable_without_channels();
    Ok(saved)
}

fn migrate_text_output(path: &Path, saved: &mut Saved) -> Result<(), String> {
    if !saved.builtin_text_tools_migrated {
        let mut next = saved.clone();
        let root = path
            .parent()
            .and_then(Path::parent)
            .ok_or("voice.error.config")?;
        let host = super::super::legacy_features::read_data(root, "host")?;
        let data = super::super::legacy_features::read_data(root, "echobird.text-tools")?;
        next.config.text_output.enabled =
            host["processors"]
                .as_array()
                .into_iter()
                .flatten()
                .any(|selection| {
                    super::super::legacy_features::selected(
                        selection,
                        "echobird.text-tools",
                        "clean-output",
                    )
                });
        if !data.is_null() {
            next.config.text_output.collapse_spaces = data["configuration"]["collapse-spaces"]
                .as_bool()
                .unwrap_or(true);
            next.config.text_output.prefix = data["configuration"]["prefix"]
                .as_str()
                .unwrap_or_default()
                .into();
            next.format_invocations = data["storage"]["invocations"].as_u64().unwrap_or_default();
        }
        next.builtin_text_tools_migrated = true;
        if !data.is_null() || !host.is_null() {
            save(path, &next)?;
        }
        *saved = next;
    }
    Ok(())
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

#[cfg(test)]
mod text_output_tests {
    use super::*;
    #[test]
    fn formatting_preserves_line_breaks_and_applies_prefix_after_trimming() {
        let options = TextOutput {
            enabled: false,
            collapse_spaces: true,
            prefix: "Note: ".into(),
        };
        assert_eq!(
            options.format(" \tHello  \tworld\n second  line \n"),
            "Note: Hello world\n second line"
        );
        let options = TextOutput {
            collapse_spaces: false,
            ..options
        };
        assert_eq!(options.format("  two  words  "), "Note: two  words");
        assert!(!TextOutput::default().enabled);
    }
    #[test]
    fn legacy_preferences_import_once_and_keep_original_files() {
        let root =
            std::env::temp_dir().join(format!("echobird-text-migration-{}", uuid::Uuid::new_v4()));
        let data = root.join("plugins/data");
        std::fs::create_dir_all(&data).unwrap();
        let host = data.join("host.json");
        let preferences = data.join("echobird.text-tools.json");
        std::fs::write(
            &host,
            r#"{"processors":[{"pluginId":"echobird.text-tools","id":"clean-output"}]}"#,
        )
        .unwrap();
        let original = r#"{"configuration":{"prefix":"> ","collapse-spaces":false},"storage":{"invocations":7}}"#;
        std::fs::write(&preferences, original).unwrap();
        let path = root.join("config/voice-input.json");
        let mut saved = load(&path).unwrap();
        assert!(saved.config.text_output.enabled);
        assert!(!saved.config.text_output.collapse_spaces);
        assert_eq!(saved.config.text_output.prefix, "> ");
        assert_eq!(saved.format_invocations, 7);
        saved.config.text_output.enabled = false;
        save(&path, &saved).unwrap();
        assert!(!load(&path).unwrap().config.text_output.enabled);
        assert_eq!(std::fs::read_to_string(preferences).unwrap(), original);
        std::fs::remove_dir_all(root).unwrap();
    }
}
