//! "我的AI生涯" (My AI Career) data layer — cross-tool session history +
//! contribution heatmap. Ported from Coffee CLI's `server.rs` history
//! scanner, trimmed to six first-class tool families.
//!
//! Each family reads its on-disk session store DIRECTLY (independent of
//! EchoBird's tool detection — we just scan the well-known roots).
//! Desktop and CLI variants of a family share one session store, so they
//! fold into a single family here (e.g. EchoBird's `claudecode` +
//! `claudedesktop` tool ids both map to the `Claude` family →
//! `~/.claude/projects`).
//!
//! | Family   | Root                              | Shape                          |
//! |----------|-----------------------------------|--------------------------------|
//! | Claude   | `~/.claude/projects`              | JSONL, depth 2                 |
//! | Codex    | `~/.codex/sessions`               | JSONL rollout, depth 4         |
//! | OpenCode | `~/.local/share/opencode`         | SQLite (`opencode.db`)         |
//! | Hermes   | `<HERMES_HOME>/state.db`          | SQLite (`state.db`)            |
//! | MiMo     | `~/.local/share/mimocode`         | SQLite (`mimocode.db`)         |
//! | DeepSeek | `<DSH_HOME|~/.dsh>/sessions`     | zstd/JSONL event log, depth 3  |
//!
//! Two surfaces consume this: the per-family history list (paginated, one
//! family at a time — keeps the payload small) and the contribution heatmap
//! (all six families aggregated, 210-day lookback, on-disk count cache).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

mod codex_pages;
mod daily_tokens;
mod persistent_cache;
pub use daily_tokens::daily_tokens;
pub use daily_tokens::{DailyTokenEntry, DailyTokenResponse};

// ─── Tool families ───────────────────────────────────────────────────────

/// The first-class families. CLI + desktop variants fold into one. MiMo Code is
/// Xiaomi's OpenCode fork (same Drizzle/SQLite store, different data dir + db
/// name) — it rides the same reader as OpenCode. DeepSeek is DeepSeek Harness
/// (`dsh`): each session is one event log under `~/.dsh/sessions` (zstd by
/// default), read directly here — independent of EchoBird's dsh model config.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Family {
    Claude,
    Codex,
    OpenCode,
    Hermes,
    MiMo,
    DeepSeek,
}

impl Family {
    pub const ALL: [Family; 6] = [
        Family::Claude,
        Family::Codex,
        Family::OpenCode,
        Family::Hermes,
        Family::MiMo,
        Family::DeepSeek,
    ];

    /// Stable id used in the IPC payload + frontend family cards.
    pub fn as_id(self) -> &'static str {
        match self {
            Family::Claude => "claude",
            Family::Codex => "codex",
            Family::OpenCode => "opencode",
            Family::Hermes => "hermes",
            Family::MiMo => "mimo",
            Family::DeepSeek => "deepseek",
        }
    }

    pub fn from_id(s: &str) -> Option<Self> {
        match s {
            "claude" => Some(Family::Claude),
            "codex" => Some(Family::Codex),
            "opencode" => Some(Family::OpenCode),
            "hermes" => Some(Family::Hermes),
            "mimo" => Some(Family::MiMo),
            "deepseek" => Some(Family::DeepSeek),
            _ => None,
        }
    }

    /// JSONL scan depth for the file-walking families; `None` for the SQLite
    /// families (OpenCode, MiMo, Hermes) and DeepSeek (its own collector) that
    /// bypass the mtime-then-parse pipeline.
    fn jsonl_depth(self) -> Option<u8> {
        match self {
            Family::Claude => Some(2), // ~/.claude/projects/<hash>/<hash>.jsonl
            Family::Codex => Some(4),  // ~/.codex/sessions/<Y>/<M>/<D>/rollout-*.jsonl
            Family::OpenCode | Family::Hermes | Family::MiMo | Family::DeepSeek => None,
        }
    }

    /// On-disk session-store root for this family.
    fn root(self, home: &Path) -> PathBuf {
        match self {
            Family::Claude => home.join(".claude").join("projects"),
            Family::Codex => home.join(".codex").join("sessions"),
            Family::OpenCode => home.join(".local").join("share").join("opencode"),
            Family::Hermes => hermes_home(),
            // MiMo Code (Xiaomi's OpenCode fork) — `~/.local/share/mimocode`,
            // same `.local/share/<app>` pattern as OpenCode (db = mimocode.db).
            Family::MiMo => home.join(".local").join("share").join("mimocode"),
            // DeepSeek Harness — `<dsh home>/sessions` (the dsh-base bundle's
            // `session-persistence-jsonl` root; see dsh_home()).
            Family::DeepSeek => dsh_home().join("sessions"),
        }
    }
}

/// Resolve Hermes Agent's data root. macOS/Linux use `~/.hermes`; Windows
/// uses `%LOCALAPPDATA%\hermes` (the official installer's choice); an
/// absolute `$HERMES_HOME` overrides both. Mirrors Coffee CLI's
/// `tools::hermes::hermes_home`.
fn hermes_home() -> PathBuf {
    if let Ok(v) = std::env::var("HERMES_HOME") {
        let trimmed = v.trim();
        if !trimmed.is_empty() {
            let candidate = PathBuf::from(trimmed);
            if candidate.is_absolute() {
                return candidate;
            }
        }
    }
    #[cfg(windows)]
    {
        if let Some(local) = dirs::data_local_dir() {
            return local.join("hermes");
        }
    }
    dirs::home_dir().unwrap_or_default().join(".hermes")
}

/// Resolve DeepSeek Harness's data root: an absolute `$DSH_HOME` overrides,
/// else `~/.dsh`. Mirrors `resolveDshHome` in `@deepseek-ai/dsh-home-paths`.
/// Sessions live under `<home>/sessions` (the dsh-base bundle's
/// `session-persistence-jsonl` root), so `Family::root` joins that.
fn dsh_home() -> PathBuf {
    if let Ok(v) = std::env::var("DSH_HOME") {
        let trimmed = v.trim();
        if !trimmed.is_empty() {
            let candidate = PathBuf::from(trimmed);
            if candidate.is_absolute() {
                return candidate;
            }
        }
    }
    dirs::home_dir().unwrap_or_default().join(".dsh")
}

// ─── Wire types (snake_case to match the ported frontend) ────────────────

#[derive(Serialize, Deserialize, Clone)]
pub struct SavedSession {
    pub id: String,
    pub name: String,
    pub tool: String,
    pub cwd: String,
    pub session_token: Option<String>,
    pub saved_at: String,
    pub file_path: Option<String>,
    pub turn_count: Option<u32>,
    #[serde(default)]
    pub model_id: Option<String>,
    #[serde(default)]
    pub message_count: Option<u32>,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
}

fn session_model_id(value: Option<&serde_json::Value>) -> Option<String> {
    value
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|id| !id.is_empty() && *id != "<synthetic>")
        .map(str::to_string)
}

#[derive(Default, Clone, Copy)]
struct TokenUsage {
    input: Option<u64>,
    output: Option<u64>,
}

impl TokenUsage {
    fn read(value: &serde_json::Value, input: &str, output: &str) -> Self {
        Self {
            input: value.get(input).and_then(serde_json::Value::as_u64),
            output: value.get(output).and_then(serde_json::Value::as_u64),
        }
    }

    fn add(&mut self, other: Self) {
        if let Some(input) = other.input {
            self.input = Some(self.input.unwrap_or(0).saturating_add(input));
        }
        if let Some(output) = other.output {
            self.output = Some(self.output.unwrap_or(0).saturating_add(output));
        }
    }
}

/// One tuple per session file: mtime (seconds since epoch) + an approximate
/// message count. The frontend buckets these into local-day boxes.
#[derive(Serialize)]
pub struct HeatmapEntry {
    pub ts: i64,
    pub count: u32,
}

// ─── Title extraction helpers ────────────────────────────────────────────

/// XML-style tags / synthetic prompts injected by the tools when run inside
/// an IDE or shell. Filtered out of title extraction so the list shows what
/// the user actually typed.
///
/// Codex Desktop (the ChatGPT desktop app's Codex/agent mode) prefixes its
/// sessions with `<recommended_plugins>` / `<environment_context>` and packs
/// attachments as `<image name=…>…</image>`, so all three belong here too
/// (mirrors the Coffee CLI fix, `desktop` test group). Its ambient browser
/// state (`<in-app-browser-context …>`) is likewise synthetic — the block
/// itself says "Do not treat it as an instruction".
///
/// The last entry is Claude Code's compaction / prior-session summary prompt.
/// It lands in the jsonl as a bare `user` message (no `<session-start-hook-
/// additional-context>` wrapper — that tag is stripped before disk), so we
/// must anchor on the literal prefix, not a structural tag. Without this,
/// every post-compaction session shows up titled
/// "Below is a conversation log from a Claud...". Mirrors Coffee CLI 9748a48.
const SYSTEM_INJECTION_TAGS: &[&str] = &[
    "<recommended_plugins>",
    "<environment_context>",
    "<ide_opened_file>",
    "<ide_closed_file>",
    "<ide_selection>",
    "<system-reminder>",
    "<command-message>",
    "<command-name>",
    "<image name=",
    "</image>",
    "<in-app-browser-context",
    "<external_codex_apps_open_page>",
    "# AGENTS.md",
    "Below is a conversation log from a Claude Code coding session",
];

fn is_system_injected(text: &str) -> bool {
    let t = text.trim();
    SYSTEM_INJECTION_TAGS.iter().any(|tag| t.starts_with(tag))
}

/// The title a Claude user text block contributes, by the rule Claude Code's
/// own session list uses: anything opening with a lowercase XML-style tag is
/// harness wrapping (IDE context, reminders, hook output, …) rather than a
/// typed prompt, as is the "[Request interrupted by user]" marker. That one
/// rule covers wrappers no list has seen yet. A slash command is remembered as
/// a fallback; a `!` shell line reads as `! <command>`.
fn claude_prompt_title(text: &str, command_fallback: &mut String) -> Option<String> {
    use std::sync::OnceLock;
    static COMMAND: OnceLock<regex::Regex> = OnceLock::new();
    static BASH: OnceLock<regex::Regex> = OnceLock::new();
    static WRAPPER: OnceLock<regex::Regex> = OnceLock::new();
    let command_pattern =
        COMMAND.get_or_init(|| regex::Regex::new(r"<command-name>(.*?)</command-name>").unwrap());
    let bash_pattern =
        BASH.get_or_init(|| regex::Regex::new(r"^<bash-input>([\s\S]*?)</bash-input>").unwrap());
    let wrapper_pattern = WRAPPER.get_or_init(|| {
        regex::Regex::new(r"^(?:<[a-z][\w-]*[\s>/]|\[Request interrupted by user[^\]]*\])").unwrap()
    });
    let text = text.replace(['\r', '\n'], " ");
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    // Only a row that opens with a tag is the CLI's own slash-command or shell record; a typed prompt
    // that merely mentions those tags stays a prompt. Older CLIs put <command-message> before the name.
    if let Some(command) = command_pattern
        .captures(text)
        .filter(|_| text.starts_with("<command-"))
    {
        if command_fallback.is_empty() {
            *command_fallback = command[1].trim().to_string();
        }
        return None;
    }
    if let Some(shell) = bash_pattern.captures(text) {
        return Some(format!("! {}", shell[1].trim()));
    }
    (!wrapper_pattern.is_match(text) && !is_system_injected(text)).then(|| text.to_string())
}

/// Replace newlines with spaces while preserving the complete title.
fn make_title(raw: &str) -> String {
    raw.replace('\n', " ")
}

fn mtime_millis(path: &Path) -> String {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_millis().to_string())
        .unwrap_or_default()
}

fn turns_from_messages(total_messages: u32) -> u32 {
    if total_messages > 0 {
        std::cmp::max(1, total_messages.div_ceil(2))
    } else {
        0
    }
}

// ─── Per-family parsers ──────────────────────────────────────────────────

/// Generic agent JSONL parser (Claude Code). One JSON object per line; pulls
/// `sessionId` / `cwd` off any row, counts user+assistant messages, and uses
/// the first real user message as the title. Files with no real user message
/// are internal compaction/summary sub-tasks and are not user-visible sessions.
#[cfg(test)]
fn parse_agent_jsonl(file_path: &Path, family: Family) -> Option<SavedSession> {
    parse_agent_jsonl_checked(file_path, family).ok().flatten()
}

fn parse_agent_jsonl_checked(
    file_path: &Path,
    family: Family,
) -> Result<Option<SavedSession>, String> {
    use std::io::BufRead;
    let file = std::fs::File::open(file_path).map_err(|e| e.to_string())?;
    let reader = std::io::BufReader::new(file);

    let mut session_id = file_path
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    let mut cwd = String::new();
    let mut title = String::new();
    let mut total_messages = 0u32;
    let mut message_count = 0u32;
    let mut model_id = None;
    let mut seen_messages = HashSet::new();
    let mut real_user_messages = 0u32;
    let mut usage_by_message = HashMap::new();
    // Titles Claude Code writes into the transcript itself: `custom-title`
    // (a rename, also written by Claude's desktop app) and `ai-title`
    // (generated). The last row of each kind wins.
    let mut custom_title = String::new();
    let mut ai_title = String::new();
    // A slash command (`<command-name>/x</command-name>`) names the session
    // only when no typed prompt follows it.
    let mut command_fallback = String::new();

    for line in reader.lines() {
        let line = line.map_err(|e| e.to_string())?;
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&line) else {
            continue;
        };
        if let Some(s) = value.get("sessionId").and_then(|v| v.as_str()) {
            if !s.is_empty() {
                session_id = s.to_string();
            }
        }
        if let Some(c) = value.get("cwd").and_then(|v| v.as_str()) {
            if cwd.is_empty() && !c.is_empty() {
                cwd = c.to_string();
            }
        }
        let row_title = |key: &str| {
            value
                .get(key)
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|t| !t.is_empty())
                .map(str::to_string)
        };
        match value.get("type").and_then(|v| v.as_str()).unwrap_or("") {
            "custom-title" => {
                if let Some(t) = row_title("customTitle") {
                    custom_title = t;
                }
                continue;
            }
            "ai-title" => {
                if let Some(t) = row_title("aiTitle") {
                    ai_title = t;
                }
                continue;
            }
            _ => {}
        }

        let mut msg_obj = value.get("message").and_then(|v| v.as_object());
        if msg_obj.is_none() {
            if let Some(payload) = value.get("payload").and_then(|v| v.as_object()) {
                if payload.get("type").and_then(|v| v.as_str()) == Some("message") {
                    msg_obj = Some(payload);
                }
            }
        }
        let Some(msg_obj) = msg_obj else { continue };
        let role = msg_obj.get("role").and_then(|v| v.as_str()).unwrap_or("");
        if role == "assistant" {
            if let Some(model) = session_model_id(msg_obj.get("model")) {
                model_id = Some(model);
            }
            if let Some(usage) = msg_obj.get("usage") {
                let key = msg_obj
                    .get("id")
                    .or_else(|| value.get("uuid"))
                    .and_then(|v| v.as_str())
                    .map(str::to_string)
                    .unwrap_or_else(|| format!("row-{}", usage_by_message.len()));
                usage_by_message.insert(
                    key,
                    TokenUsage::read(usage, "input_tokens", "output_tokens"),
                );
            }
        }
        if role == "user" || role == "assistant" {
            total_messages += 1;
            let tool_result_only = role == "user"
                && msg_obj
                    .get("content")
                    .and_then(|v| v.as_array())
                    .is_some_and(|blocks| {
                        !blocks.is_empty()
                            && blocks.iter().all(|block| {
                                block.get("type").and_then(|v| v.as_str()) == Some("tool_result")
                            })
                    });
            let synthetic_assistant = role == "assistant"
                && msg_obj
                    .get("model")
                    .and_then(|v| v.as_str())
                    .is_some_and(|model| model.trim() == "<synthetic>");
            let identity = msg_obj
                .get("id")
                .or_else(|| value.get("uuid"))
                .and_then(|v| v.as_str())
                .filter(|id| !id.is_empty());
            if !tool_result_only
                && !synthetic_assistant
                && identity.map_or(true, |id| {
                    seen_messages.insert((role.to_string(), id.to_string()))
                })
            {
                message_count += 1;
            }
        }
        // Rows the CLI itself marks as not typed by the user are not user input.
        let flagged = |key: &str| value.get(key).and_then(|v| v.as_bool()) == Some(true);
        if role != "user" || flagged("isMeta") || flagged("isCompactSummary") {
            continue;
        }
        let texts: Vec<&str> = match msg_obj.get("content") {
            Some(serde_json::Value::String(text)) => vec![text.as_str()],
            // Only text blocks can be a prompt; a tool result riding in the row is skipped.
            Some(serde_json::Value::Array(blocks)) => blocks
                .iter()
                .filter(|b| {
                    matches!(
                        b.get("type").and_then(|v| v.as_str()),
                        Some("text" | "input_text")
                    )
                })
                .filter_map(|b| b.get("text").and_then(|v| v.as_str()))
                .collect(),
            _ => Vec::new(),
        };
        let mut command = String::new();
        let prompt = texts
            .into_iter()
            .find_map(|text| claude_prompt_title(text, &mut command));
        // One rule decides both whether the user typed anything (the compaction-sub-task filter below)
        // and what names the session, so a kept session never falls back to a placeholder.
        if prompt.is_some() || !command.is_empty() {
            real_user_messages += 1;
        }
        if command_fallback.is_empty() {
            command_fallback = command;
        }
        if let Some(prompt) = prompt.filter(|_| title.is_empty()) {
            title = prompt;
        }
    }

    // Fallback cwd from the encoded project-folder name (`C--Users--x` → `C:\Users\x`).
    if cwd.is_empty() {
        if let Some(folder) = file_path
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
        {
            if folder.contains("--") {
                let mut parts = folder.split("--");
                if let Some(drive) = parts.next() {
                    let rest: Vec<&str> = parts.collect();
                    cwd = if cfg!(target_os = "windows") {
                        format!("{}:\\{}", drive, rest.join("\\"))
                    } else {
                        format!("/{}/{}", drive, rest.join("/"))
                    };
                }
            }
        }
    }

    if real_user_messages == 0 {
        return Ok(None);
    }

    // A rename wins over a generated title, and both over the first prompt,
    // so the row reads as it does in Claude Code's own session list.
    if !custom_title.is_empty() {
        title = custom_title;
    } else if !ai_title.is_empty() {
        title = ai_title;
    }
    if title.is_empty() {
        title = command_fallback;
    }
    if title.is_empty() {
        title = "Claude Code Session".to_string();
    }

    let id = family.as_id();
    let mut usage = TokenUsage::default();
    for sample in usage_by_message.into_values() {
        usage.add(sample);
    }
    Ok(Some(SavedSession {
        id: format!("{}_native_{}", id, session_id),
        name: title,
        tool: id.to_string(),
        cwd,
        session_token: Some(session_id),
        saved_at: mtime_millis(file_path),
        file_path: Some(file_path.to_string_lossy().into_owned()),
        turn_count: Some(turns_from_messages(total_messages)),
        model_id,
        message_count: Some(message_count),
        input_tokens: usage.input,
        output_tokens: usage.output,
    }))
}

/// Whether a Codex `session_meta` payload describes an internal sub-agent
/// rollout rather than a user-created top-level session. Codex (incl. Codex
/// Desktop) writes one rollout JSONL per spawned sub-agent, which re-inherits
/// the parent's first user message and would otherwise show up as a duplicate
/// top-level card. Two conclusive markers (see Codex's `SessionMeta` in
/// codex-rs/protocol/src/protocol.rs):
///
///   - `source` is an object `{"subagent": ...}` (`SessionSource::SubAgent`;
///     user sessions serialize `source` as a string like "cli"/"vscode").
///     The parent thread id lives nested under
///     `source.subagent.thread_spawn.parent_thread_id`, NOT as a top-level
///     field, so a top-level `parent_thread_id` lookup would miss real
///     sub-agent rollouts. Also catches older rollouts that predate
///     `thread_source`.
///   - `thread_source == "subagent"` (`ThreadSource::Subagent`).
///
/// `forked_from_id` is deliberately NOT a marker: it identifies user-initiated
/// fork/resume sessions, which are legit top-level user sessions.
fn is_codex_subagent_session(payload: &serde_json::Value) -> bool {
    if payload
        .get("source")
        .and_then(|v| v.as_object())
        .is_some_and(|obj| obj.contains_key("subagent"))
    {
        return true;
    }
    payload
        .get("thread_source")
        .and_then(|v| v.as_str())
        .is_some_and(|s| s.eq_ignore_ascii_case("subagent"))
}

/// Strip Codex Desktop's synthetic "Files mentioned by the user"
/// preamble from a user `input_text` block, returning the user's real
/// request text for title display.
///
/// Codex Desktop (the ChatGPT desktop app's Codex/agent mode;
/// `session_meta.originator == "Codex Desktop"`) packs attached-file
/// references and the user's actual question into a single block.
/// Two marker shapes exist (see the Coffee CLI fix, `desktop` test group):
///
/// ```text
/// ## My request for Codex:   (old — product name in the marker)
/// ## My request:             (new — no product name)
/// ```
///
/// Without stripping, the history title becomes the meaningless
/// "# Files mentioned by the user: ## <file>..." preamble instead of
/// the user's real first question. We split on the request marker and
/// return what follows it (old shape: after the product name + colon);
/// if the block has the preamble but no request marker (user attached
/// files with no accompanying text), we return empty so the caller
/// treats it like any other system injection and keeps scanning.
/// Blocks without the preamble are returned unchanged.
fn strip_codex_desktop_file_preamble(text: &str) -> &str {
    const PREAMBLE: &str = "# Files mentioned by the user";
    if !text.contains(PREAMBLE) {
        return text;
    }
    // Old format: `## My request for <product>:` (e.g. "## My request for Codex:").
    const OLD_MARKER: &str = "## My request for";
    if let Some(idx) = text.find(OLD_MARKER) {
        let after = &text[idx + OLD_MARKER.len()..];
        // Skip the product name (e.g. "Codex") and the colon that ends
        // the marker, then any leading whitespace.
        return match after.find(':') {
            Some(colon) => after[colon + 1..].trim_start(),
            None => after.trim_start(),
        };
    }
    // New format: `## My request:` (no product name).
    const NEW_MARKER: &str = "## My request:";
    if let Some(idx) = text.find(NEW_MARKER) {
        return text[idx + NEW_MARKER.len()..].trim_start();
    }
    "" // files-only message, no real text -> skip
}

/// Codex rollout JSONL: first row is `session_meta` (carries id + cwd;
/// `originator` is `"Codex Desktop"` for the ChatGPT desktop app's
/// Codex/agent mode, `"codex-tui"` for the terminal CLI; both share the
/// same row schema). Sub-agent rollouts are filtered out here. Subsequent
/// `response_item` / `user_message` rows hold the conversation.
#[cfg(test)]
fn parse_codex_session_jsonl(file_path: &Path) -> Option<SavedSession> {
    parse_codex_session_jsonl_checked(file_path).ok().flatten()
}

#[cfg(test)]
fn parse_codex_session_jsonl_checked(file_path: &Path) -> Result<Option<SavedSession>, String> {
    parse_codex_pages_checked(&[file_path.to_path_buf()])
}

fn parse_codex_pages_checked(files: &[PathBuf]) -> Result<Option<SavedSession>, String> {
    let file_path = &files[0];
    use std::io::BufRead;

    let mut session_id = file_path
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    let mut cwd = String::new();
    let mut title = String::new();
    let mut total_messages = 0u32;
    let mut model_id = None;
    let mut usage = daily_tokens::CodexUsage::default();
    let mut metadata_seen = false;

    for path in files {
        usage.begin_file(path);
        let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
        let reader = std::io::BufReader::new(file);
        for line in reader.lines() {
            let line = line.map_err(|e| e.to_string())?;
            let Ok(value) = serde_json::from_str::<serde_json::Value>(&line) else {
                continue;
            };
            usage.consume(&value);
            let row_type = value.get("type").and_then(|v| v.as_str()).unwrap_or("");
            let Some(payload) = value.get("payload") else {
                continue;
            };

            if row_type == "turn_context" {
                if let Some(model) = session_model_id(payload.get("model")) {
                    model_id = Some(model);
                }
            } else if row_type == "thread_settings_applied"
                || (row_type == "event_msg"
                    && payload.get("type").and_then(|v| v.as_str())
                        == Some("thread_settings_applied"))
            {
                if let Some(model) = session_model_id(payload.pointer("/thread_settings/model"))
                    .or_else(|| session_model_id(payload.pointer("/settings/model")))
                {
                    model_id = Some(model);
                }
            }

            if row_type == "session_meta" {
                if metadata_seen {
                    continue;
                }
                metadata_seen = true;
                // Codex (incl. Codex Desktop) writes a separate rollout JSONL for
                // every sub-agent it spawns; these inherit the parent's first user
                // message and would surface as duplicate top-level cards. See
                // `is_codex_subagent_session` for the marker rationale.
                // `forked_from_id` is NOT a marker (user fork/resume = top-level).
                if is_codex_subagent_session(payload) {
                    return Ok(None);
                }
                if let Some(id) = payload.get("id").and_then(|v| v.as_str()) {
                    if !id.is_empty() {
                        session_id = id.to_string();
                    }
                }
                if let Some(c) = payload.get("cwd").and_then(|v| v.as_str()) {
                    if !c.is_empty() {
                        cwd = c.to_string();
                    }
                }
                continue;
            }

            let payload_type = payload.get("type").and_then(|v| v.as_str()).unwrap_or("");
            let is_msg = (row_type == "response_item" && payload_type == "message")
                || row_type == "user_message";
            if !is_msg {
                continue;
            }
            let role = payload.get("role").and_then(|v| v.as_str()).unwrap_or("");
            if role == "user" || role == "assistant" {
                total_messages += 1;
            }
            if role != "user" || !title.is_empty() {
                continue;
            }
            let Some(arr) = payload.get("content").and_then(|v| v.as_array()) else {
                continue;
            };
            for block in arr {
                let bt = block.get("type").and_then(|v| v.as_str()).unwrap_or("");
                if bt != "input_text" && bt != "text" {
                    continue;
                }
                if let Some(raw) = block.get("text").and_then(|v| v.as_str()) {
                    // Codex Desktop packs attached files + the real question into
                    // one `input_text` block prefixed with
                    // "# Files mentioned by the user"; strip that preamble so the
                    // title is the user's actual request. Returns "" for
                    // files-only blocks (no text).
                    let text = strip_codex_desktop_file_preamble(raw);
                    if text.is_empty() || is_system_injected(text) {
                        continue;
                    }
                    title = make_title(text);
                    break;
                }
            }
        }
    }

    if title.is_empty() {
        title = "Codex Session".to_string();
    }
    let all_pages = usage.has_all_pages();
    let usage = if all_pages {
        usage.total_usage()
    } else {
        TokenUsage::default()
    };

    Ok(Some(SavedSession {
        id: format!("codex_native_{}", session_id),
        name: title,
        tool: "codex".to_string(),
        cwd,
        session_token: Some(session_id),
        saved_at: files
            .iter()
            .map(|path| mtime_millis(path))
            .max()
            .unwrap_or_default(),
        file_path: Some(file_path.to_string_lossy().into_owned()),
        turn_count: all_pages.then_some(turns_from_messages(total_messages)),
        model_id,
        message_count: all_pages.then_some(total_messages),
        input_tokens: usage.input,
        output_tokens: usage.output,
    }))
}

/// Codex appends a row to this index whenever a thread is named or renamed.
/// The last valid name for each id wins, including user-edited titles.
fn parse_codex_thread_names(index: &str) -> HashMap<String, String> {
    let mut names = HashMap::new();
    for line in index.lines() {
        let Ok(row) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        let field = |key: &str| {
            row.get(key)
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|v| !v.is_empty())
        };
        if let (Some(id), Some(name)) = (field("id"), field("thread_name")) {
            names.insert(id.to_string(), name.to_string());
        }
    }
    names
}

/// Read native titles beside the scanned sessions directory on every page
/// load: a rename can update the index without changing the rollout file.
fn apply_codex_thread_names(history_dir: &Path, sessions: &mut [SavedSession]) {
    let Some(profile_dir) = history_dir.parent() else {
        return;
    };
    let Ok(index) = std::fs::read_to_string(profile_dir.join("session_index.jsonl")) else {
        return;
    };
    let names = parse_codex_thread_names(&index);
    for session in sessions.iter_mut().filter(|s| s.tool == "codex") {
        if let Some(name) = session.session_token.as_ref().and_then(|id| names.get(id)) {
            session.name = name.clone();
        }
    }
}

// ─── Directory walking ───────────────────────────────────────────────────

/// Recursively collect `*.jsonl` files up to `depth` directory levels,
/// tagging each with its family. Mirrors Coffee CLI's
/// `collect_jsonl_paths_with_mtime`.
fn collect_jsonl_paths(
    dir: PathBuf,
    depth: u8,
    family: Family,
    out: &mut Vec<(SystemTime, PathBuf, Family)>,
) -> std::io::Result<()> {
    if depth == 0 {
        return Ok(());
    }
    let Some(entries) = read_directory(&dir)? else {
        return Ok(());
    };
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        let Some(metadata) = path_metadata(&path)? else {
            continue;
        };
        if metadata.is_file() {
            if path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
                let mtime = metadata.modified()?;
                out.push((mtime, path, family));
            }
        } else if metadata.is_dir() {
            collect_jsonl_paths(path, depth - 1, family, out)?;
        }
    }
    Ok(())
}

fn read_directory(path: &Path) -> std::io::Result<Option<std::fs::ReadDir>> {
    match std::fs::read_dir(path) {
        Ok(entries) => Ok(Some(entries)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

fn path_metadata(path: &Path) -> std::io::Result<Option<std::fs::Metadata>> {
    match std::fs::metadata(path) {
        Ok(metadata) => Ok(Some(metadata)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

// ─── OpenCode (SQLite) ───────────────────────────────────────────────────
//
// OpenCode's authoritative session store is `opencode.db` (SQLite, FULL
// history). ⚠️ There is ALSO a `storage/session/` + `storage/message/` JSON
// dir, but it is VESTIGIAL — only a couple of legacy sessions linger there
// (verified 2026-06-15: 2 JSON sessions vs 99 in the db). Do NOT "optimise" by
// reading the JSON store to drop rusqlite — it silently loses ~all OpenCode
// history. The bundled-SQLite weight (~0.6 MB in the NSIS installer) is the
// price of reading the real store.

/// MiMo Code's db. Authoritatively `~/.local/share/mimocode/mimocode.db` (its
/// OpenCode fork's `.local/share/<app>` root); an older/alt install may use
/// `~/.config/mimocode` — fall back to it if the primary is absent.
fn mimo_db_path(home: &Path) -> PathBuf {
    let primary = home
        .join(".local")
        .join("share")
        .join("mimocode")
        .join("mimocode.db");
    if primary.is_file() {
        return primary;
    }
    home.join(".config").join("mimocode").join("mimocode.db")
}

/// Normalize V1, V2, and partially migrated OpenCode stores. A V2 session owns
/// its id even when empty or archived, so stale V1 rows must not reappear.
fn drizzle_sessions_sql(conn: &rusqlite::Connection) -> String {
    let table_exists = |table: &str| {
        conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)",
            [table],
            |row| row.get::<_, bool>(0),
        )
        .unwrap_or(false)
    };
    let has_v2_sessions = table_exists("session_v2");
    let has_v2_messages = table_exists("session_message");
    let v2_count = if has_v2_messages {
        "(SELECT COUNT(*) FROM session_message WHERE session_id = s.id AND type IN ('user', 'assistant'))"
    } else {
        "0"
    };
    let v1_count = if table_exists("message") {
        "(SELECT COUNT(*) FROM message WHERE session_id = s.id)"
    } else {
        "0"
    };
    let mut sources = Vec::new();
    for table in ["session_v2", "session"] {
        if !table_exists(table) {
            continue;
        }
        let count = if table == "session_v2" {
            v2_count.to_string()
        } else if has_v2_messages {
            format!(
                "CASE WHEN EXISTS(SELECT 1 FROM session_message WHERE session_id = s.id) \
                 THEN {v2_count} ELSE {v1_count} END"
            )
        } else {
            v1_count.to_string()
        };
        let filter = if table == "session" && has_v2_sessions {
            "WHERE NOT EXISTS(SELECT 1 FROM session_v2 WHERE id = s.id)"
        } else {
            ""
        };
        let subpath = if conn
            .prepare(&format!("SELECT path FROM {table} LIMIT 0"))
            .is_ok()
        {
            "s.path"
        } else {
            "NULL"
        };
        sources.push(format!(
            "SELECT s.id, s.title, s.directory, s.time_updated, s.time_archived, \
             s.parent_id, {count} AS msg_count, {subpath} AS subpath FROM {table} s {filter}"
        ));
    }
    sources.join(" UNION ALL ")
}

/// One page of a Drizzle/SQLite tool's sessions, newest first. OpenCode V1/V2
/// and its MiMo Code fork use `time_updated` ms and `time_archived`.
/// `tool`/`label` set the
/// SavedSession id-prefix + `tool` tag and the fallback title.
///
/// `parent_id IS NULL` excludes sub-agent sessions: OpenCode writes one row
/// per spawned sub-agent (parallel / task tool) with `parent_id` pointing at
/// the parent. Its own desktop excludes those from the root list
/// (`isNull(parent_id)`) and loads them on-demand from the parent's
/// timeline — sub-agents can't be independently resumed, so hiding them
/// matches the canonical UX instead of flattening children into the list.
#[cfg(test)]
fn drizzle_history_page(
    db_path: &Path,
    tool: &str,
    label: &str,
    offset: usize,
    limit: usize,
) -> Vec<SavedSession> {
    drizzle_history_page_checked(db_path, tool, label, offset, limit).unwrap_or_default()
}

fn drizzle_history_page_checked(
    db_path: &Path,
    tool: &str,
    label: &str,
    offset: usize,
    limit: usize,
) -> Result<Vec<SavedSession>, String> {
    if path_metadata(db_path)
        .map_err(|error| error.to_string())?
        .is_none()
    {
        return Ok(Vec::new());
    }
    let conn = rusqlite::Connection::open_with_flags(
        db_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|error| error.to_string())?;
    let query = format!(
        "WITH sessions AS ({}) \
         SELECT s.id, s.title, s.directory, s.time_updated, s.msg_count, s.subpath \
         FROM sessions s WHERE s.time_archived IS NULL AND s.parent_id IS NULL \
         ORDER BY s.time_updated DESC, s.id ASC LIMIT ?1 OFFSET ?2",
        drizzle_sessions_sql(&conn)
    );
    let mut stmt = conn.prepare(&query).map_err(|error| error.to_string())?;
    let db_str = db_path.to_string_lossy().into_owned();
    let rows = stmt
        .query_map([limit as i64, offset as i64], |row| {
            let id: String = row.get(0)?;
            let title: String = row
                .get::<_, Option<String>>(1)?
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| format!("{} Session", label));
            let directory: String = row.get::<_, Option<String>>(2)?.unwrap_or_default();
            let time_updated = row.get::<_, Option<i64>>(3)?.unwrap_or(0);
            let msg_count = row.get::<_, Option<i64>>(4)?.unwrap_or(0);
            let subpath: Option<String> = row.get(5)?;
            let cwd = match subpath.filter(|path| !path.is_empty() && path != ".") {
                Some(path) => Path::new(&directory)
                    .join(path)
                    .to_string_lossy()
                    .into_owned(),
                None => directory,
            };
            let (usage, model_id) = drizzle_session_details_checked(&conn, &id)?;
            Ok(SavedSession {
                id: format!("{}_native_{}", tool, id),
                name: title,
                tool: tool.to_string(),
                cwd,
                session_token: Some(id),
                saved_at: time_updated.to_string(),
                file_path: Some(db_str.clone()),
                turn_count: Some(std::cmp::max(1, msg_count / 2) as u32),
                model_id,
                message_count: u32::try_from(msg_count).ok(),
                input_tokens: usage.input,
                output_tokens: usage.output,
            })
        })
        .map_err(|error| error.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())
}

fn sqlite_has_column(
    conn: &rusqlite::Connection,
    table: &str,
    column: &str,
) -> rusqlite::Result<bool> {
    let mut statement = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let rows = statement.query_map([], |row| row.get::<_, String>(1))?;
    for name in rows {
        if name? == column {
            return Ok(true);
        }
    }
    Ok(false)
}

fn drizzle_session_details_checked(
    conn: &rusqlite::Connection,
    session_id: &str,
) -> rusqlite::Result<(TokenUsage, Option<String>)> {
    let has_v2_session = if sqlite_has_column(conn, "session_v2", "id")? {
        conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM session_v2 WHERE id = ?1)",
            [session_id],
            |row| row.get::<_, bool>(0),
        )?
    } else {
        false
    };
    let has_v2_messages = if sqlite_has_column(conn, "session_message", "session_id")? {
        conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM session_message WHERE session_id = ?1)",
            [session_id],
            |row| row.get::<_, bool>(0),
        )?
    } else {
        false
    };
    let table = if has_v2_session || has_v2_messages {
        "session_message"
    } else {
        "message"
    };
    if !sqlite_has_column(conn, table, "data")? {
        return Ok((TokenUsage::default(), None));
    }
    let boundary = drizzle_usage_boundary(conn, table);
    let order = if sqlite_has_column(conn, table, "seq")? {
        "m.seq, m.id"
    } else if sqlite_has_column(conn, table, "time_created")? {
        "m.time_created, m.id"
    } else {
        "m.rowid"
    };
    let role = if table == "session_message" && sqlite_has_column(conn, table, "type")? {
        "m.type"
    } else {
        "NULL"
    };
    let mut statement = conn.prepare(&format!(
        "SELECT m.data, {role} FROM {table} m WHERE m.session_id = ?1{boundary} ORDER BY {order}"
    ))?;
    let rows = statement.query_map([session_id], |row| {
        Ok((
            row.get::<_, Option<String>>(0)?,
            row.get::<_, Option<String>>(1)?,
        ))
    })?;
    let mut usage = TokenUsage::default();
    let mut model_id = None;
    for row in rows {
        let (data, role) = row?;
        let Some(data) = data else { continue };
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(&data) {
            if role
                .as_deref()
                .or_else(|| value.get("role").and_then(|v| v.as_str()))
                == Some("assistant")
            {
                if let Some(model) = session_model_id(value.pointer("/model/id"))
                    .or_else(|| session_model_id(value.get("modelID")))
                    .or_else(|| session_model_id(value.get("modelId")))
                {
                    model_id = Some(model);
                }
            }
            if let Some(tokens) = value.get("tokens") {
                let sample = TokenUsage::read(tokens, "input", "output");
                if table != "message" || !is_unmeasured_drizzle_zero(&value, sample) {
                    usage.add(sample);
                }
            }
        }
    }
    Ok((usage, model_id))
}

fn is_unmeasured_drizzle_zero(value: &serde_json::Value, usage: TokenUsage) -> bool {
    value.get("role").and_then(serde_json::Value::as_str) == Some("assistant")
        && usage.input == Some(0)
        && usage.output == Some(0)
        && value
            .get("finish")
            .and_then(serde_json::Value::as_str)
            .filter(|finish| !finish.is_empty())
            .is_none()
}

fn drizzle_usage_boundary(conn: &rusqlite::Connection, message_table: &str) -> String {
    if conn
        .prepare(&format!("SELECT time_created FROM {message_table} LIMIT 0"))
        .is_err()
    {
        return String::new();
    }
    let has_v2_sessions = conn.prepare("SELECT id FROM session_v2 LIMIT 0").is_ok();
    let mut sources = Vec::new();
    for table in ["session_v2", "session"] {
        if conn
            .prepare(&format!("SELECT time_created FROM {table} LIMIT 0"))
            .is_ok()
        {
            let migration = if table == "session" && has_v2_sessions {
                " WHERE NOT EXISTS(SELECT 1 FROM session_v2 WHERE id=session.id)"
            } else {
                ""
            };
            sources.push(format!("SELECT id,time_created FROM {table}{migration}"));
        }
    }
    if sources.is_empty() {
        String::new()
    } else {
        format!(" AND (m.time_created IS NULL OR m.time_created<=0 OR m.time_created>=COALESCE((SELECT time_created FROM ({}) WHERE id=m.session_id),0))", sources.join(" UNION ALL "))
    }
}

/// Drizzle/SQLite heatmap entries (timestamp + message count per session) past
/// the cutoff. `time_updated` is milliseconds; we emit seconds. Shared by
/// OpenCode + its MiMo Code fork.
#[cfg(test)]
fn collect_drizzle_heatmap_entries(db_path: &Path, cutoff_secs: i64, out: &mut Vec<HeatmapEntry>) {
    collect_drizzle_heatmap_entries_checked(db_path, cutoff_secs, out).unwrap();
}

fn collect_drizzle_heatmap_entries_checked(
    db_path: &Path,
    cutoff_secs: i64,
    out: &mut Vec<HeatmapEntry>,
) -> Result<(), String> {
    if path_metadata(db_path)
        .map_err(|error| error.to_string())?
        .is_none()
    {
        return Ok(());
    }
    let conn = rusqlite::Connection::open_with_flags(
        db_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|error| error.to_string())?;
    let cutoff_ms = cutoff_secs.saturating_mul(1000);
    let query = format!(
        "WITH sessions AS ({}) \
         SELECT s.time_updated, s.msg_count FROM sessions s \
         WHERE s.time_archived IS NULL AND s.time_updated >= ?1",
        drizzle_sessions_sql(&conn)
    );
    let mut stmt = conn.prepare(&query).map_err(|error| error.to_string())?;
    let rows = stmt
        .query_map([cutoff_ms], |row| {
            let ts_ms: i64 = row.get(0)?;
            let count: i64 = row.get(1)?;
            Ok((ts_ms, count))
        })
        .map_err(|error| error.to_string())?;
    for row in rows {
        let (ts_ms, count) = row.map_err(|error| error.to_string())?;
        if count > 0 {
            out.push(HeatmapEntry {
                ts: ts_ms / 1000,
                count: count as u32,
            });
        }
    }
    Ok(())
}

// ─── Message counting (heatmap) ──────────────────────────────────────────

/// Cheap line-count for JSONL files; every non-empty line is one "turn".
/// Capped at 32 MiB so a runaway session can't stall the scan.
#[cfg(test)]
fn count_jsonl_message_lines(path: &Path) -> u32 {
    count_jsonl_message_lines_checked(path).unwrap_or(0)
}

fn count_jsonl_message_lines_checked(path: &Path) -> std::io::Result<u32> {
    use std::io::{BufRead, BufReader, Read};
    let file = std::fs::File::open(path)?;
    const MAX_BYTES: u64 = 32 * 1024 * 1024;
    let mut br = BufReader::new(file.take(MAX_BYTES));
    let mut buf: Vec<u8> = Vec::with_capacity(512);
    let mut count = 0u32;
    loop {
        let n = br.read_until(b'\n', &mut buf)?;
        if n == 0 {
            break;
        }
        if buf.iter().any(|&b| !b.is_ascii_whitespace()) {
            count = count.saturating_add(1);
        }
        buf.clear();
    }
    Ok(count)
}

// ─── Hermes (SQLite) ─────────────────────────────────────────────────────
//
// Hermes (like OpenCode) keeps every session in a SQLite db — `state.db`
// (sessions + messages tables + FTS5 search). The `sessions/` dir is only a
// gateway routing index (`sessions.json`) / optional JSONL exports, NOT the
// session store, so we read the db. `started_at` is epoch SECONDS (float);
// visible user/assistant counts are read separately from the session counters.

/// Best display text out of a decoded JSON message-content value: a bare
/// string, the first `{type:"text", text:…}` block of an array, or an object's
/// `text` / `content` field. `None` if no non-empty text is found.
fn first_json_text(v: &serde_json::Value) -> Option<String> {
    match v {
        serde_json::Value::String(s) => {
            let s = s.trim();
            (!s.is_empty()).then(|| s.to_string())
        }
        serde_json::Value::Array(arr) => arr.iter().find_map(|el| match el.as_object() {
            Some(obj) => {
                if obj.get("type").and_then(|t| t.as_str()).unwrap_or("text") != "text" {
                    return None;
                }
                obj.get("text")
                    .and_then(|t| t.as_str())
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string)
            }
            None => first_json_text(el),
        }),
        serde_json::Value::Object(obj) => obj
            .get("text")
            .and_then(|t| t.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .or_else(|| obj.get("content").and_then(first_json_text)),
        _ => None,
    }
}

/// Decode a Hermes `messages.content` cell to display text. Hermes stores a
/// plain string for scalar text, or `"\x00json:"` + JSON for structured /
/// multimodal content (per its `_encode_content`). Mirror that: strip the
/// prefix and pull text out of the JSON, else use the raw string.
fn hermes_decode_text(content: &str) -> String {
    if let Some(rest) = content.strip_prefix("\u{0}json:") {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(rest) {
            if let Some(t) = first_json_text(&v) {
                return t;
            }
        }
    }
    content.to_string()
}

fn hermes_session_visibility(conn: &rusqlite::Connection) -> rusqlite::Result<&'static str> {
    Ok(if sqlite_has_column(conn, "sessions", "hidden")? {
        "archived = 0 AND COALESCE(hidden, 0) = 0"
    } else {
        "archived = 0"
    })
}

/// Modern display rows include compacted history, but not rewound or model-only
/// rows. Each indexed display position prefers the live, then newest, copy.
fn hermes_message_visibility(conn: &rusqlite::Connection) -> rusqlite::Result<Option<String>> {
    for column in ["id", "active", "compacted", "display_order"] {
        if !sqlite_has_column(conn, "messages", column)? {
            return Ok(None);
        }
    }
    let mut visible = "(m.active = 1 OR m.compacted = 1)".to_string();
    if sqlite_has_column(conn, "messages", "display_metadata")? {
        visible.push_str(
            " AND COALESCE(json_extract(CASE WHEN json_valid(m.display_metadata) \
             THEN m.display_metadata ELSE '{}' END, '$.model_only'), 0) = 0",
        );
    }
    let candidate_visible = visible.replace("m.", "candidate.");
    Ok(Some(format!(
        "{visible} AND (m.display_order IS NULL OR NOT EXISTS (\
            SELECT 1 FROM messages candidate \
            WHERE candidate.session_id = m.session_id \
            AND candidate.display_order = m.display_order AND {candidate_visible} \
            AND (candidate.active > m.active OR \
                (candidate.active = m.active AND candidate.id > m.id))))"
    )))
}

/// First non-empty user-message text for a session — Hermes' title-preview
/// source. Mirrors its SQL: earliest `role='user'` message that has content.
fn hermes_first_user_text_checked(
    conn: &rusqlite::Connection,
    session_id: &str,
    message_visibility: Option<&str>,
) -> rusqlite::Result<Option<String>> {
    use rusqlite::OptionalExtension;
    if !conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='messages')",
        [],
        |row| row.get::<_, bool>(0),
    )? {
        return Ok(None);
    }
    let order = if message_visibility.is_some() {
        "COALESCE(m.display_order, m.id), m.id"
    } else {
        "m.timestamp, m.id"
    };
    let visibility = message_visibility.unwrap_or("1");
    let query = format!(
        "SELECT m.content FROM messages m \
         WHERE m.session_id = ?1 AND m.role = 'user' AND m.content IS NOT NULL \
         AND {visibility} ORDER BY {order} LIMIT 1"
    );
    let content: Option<String> = conn
        .query_row(&query, [session_id], |row| row.get(0))
        .optional()?
        .flatten();
    let Some(content) = content else {
        return Ok(None);
    };
    let text = hermes_decode_text(&content);
    let text = text.trim();
    Ok((!text.is_empty()).then(|| text.to_string()))
}

/// One page of Hermes sessions, newest first, from `state.db`. The displayed
/// name mirrors Hermes' own `_build_session_title` fallback chain (explicit
/// `title` → first user-message preview → cwd basename → "New thread") — most
/// sessions get an auto-generated `title` after their first turn, and untitled
/// ones show the first user message rather than a generic label.
#[cfg(test)]
fn hermes_history_page(db_path: &Path, offset: usize, limit: usize) -> Vec<SavedSession> {
    hermes_history_page_checked(db_path, offset, limit).unwrap_or_default()
}

fn hermes_history_page_checked(
    db_path: &Path,
    offset: usize,
    limit: usize,
) -> Result<Vec<SavedSession>, String> {
    if path_metadata(db_path)
        .map_err(|error| error.to_string())?
        .is_none()
    {
        return Ok(Vec::new());
    }
    let conn = rusqlite::Connection::open_with_flags(
        db_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|error| error.to_string())?;
    let db_str = db_path.to_string_lossy().into_owned();
    let session_visibility = hermes_session_visibility(&conn).map_err(|error| error.to_string())?;
    let message_visibility = hermes_message_visibility(&conn).map_err(|error| error.to_string())?;

    // Collect the page first so the prepared statement is dropped before we run
    // the per-session title-preview queries on the same connection.
    struct Raw {
        id: String,
        title: String,
        cwd: String,
        started_at: f64,
        msg_count: Option<i64>,
        conversation_count: Option<u32>,
        model_id: Option<String>,
        usage: TokenUsage,
    }
    let raws: Vec<Raw> = {
        let usage_columns = if sqlite_has_column(&conn, "sessions", "input_tokens")
            .map_err(|error| error.to_string())?
            && sqlite_has_column(&conn, "sessions", "output_tokens")
                .map_err(|error| error.to_string())?
        {
            "input_tokens, output_tokens"
        } else {
            "NULL, NULL"
        };
        let model_column =
            if sqlite_has_column(&conn, "sessions", "model").map_err(|error| error.to_string())? {
                "model"
            } else {
                "NULL"
            };
        let conversation_count = if sqlite_has_column(&conn, "messages", "session_id")
            .map_err(|error| error.to_string())?
            && sqlite_has_column(&conn, "messages", "role").map_err(|error| error.to_string())?
        {
            format!(
                "(SELECT COUNT(*) FROM messages m WHERE m.session_id = sessions.id \
                 AND m.role IN ('user', 'assistant') AND {})",
                message_visibility.as_deref().unwrap_or("1")
            )
        } else {
            "NULL".to_string()
        };
        let query = format!(
            "SELECT id, title, cwd, started_at, message_count, {usage_columns}, {model_column}, {conversation_count} \
                     FROM sessions \
                     WHERE {session_visibility} \
                     ORDER BY started_at DESC \
                     LIMIT ?1 OFFSET ?2"
        );
        let mut stmt = conn.prepare(&query).map_err(|error| error.to_string())?;
        let rows = stmt
            .query_map([limit as i64, offset as i64], |row| {
                Ok(Raw {
                    id: row.get(0)?,
                    title: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                    cwd: row.get::<_, Option<String>>(2)?.unwrap_or_default(),
                    started_at: row.get::<_, Option<f64>>(3)?.unwrap_or(0.0),
                    msg_count: row.get(4)?,
                    conversation_count: row
                        .get::<_, Option<i64>>(8)?
                        .and_then(|count| u32::try_from(count).ok()),
                    model_id: row.get(7)?,
                    usage: TokenUsage {
                        input: row
                            .get::<_, Option<i64>>(5)?
                            .and_then(|v| u64::try_from(v).ok()),
                        output: row
                            .get::<_, Option<i64>>(6)?
                            .and_then(|v| u64::try_from(v).ok()),
                    },
                })
            })
            .map_err(|error| error.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?
    };
    let has_model_usage = ["session_id", "model", "last_seen"]
        .into_iter()
        .map(|column| sqlite_has_column(&conn, "session_model_usage", column))
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|error| error.to_string())?
        .into_iter()
        .all(|present| present);
    let main_task = if has_model_usage
        && sqlite_has_column(&conn, "session_model_usage", "task")
            .map_err(|error| error.to_string())?
    {
        " AND COALESCE(task, '') = ''"
    } else {
        ""
    };

    raws.into_iter()
        .map(|r| {
            let explicit = r.title.trim();
            let name = if !explicit.is_empty() {
                make_title(explicit)
            } else {
                hermes_first_user_text_checked(&conn, &r.id, message_visibility.as_deref())
                    .map_err(|error| error.to_string())?
                    .map(|t| make_title(&t))
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| {
                        let leaf = Path::new(&r.cwd)
                            .file_name()
                            .and_then(|s| s.to_str())
                            .unwrap_or("");
                        if leaf.is_empty() {
                            "Hermes Session".to_string()
                        } else {
                            leaf.to_string()
                        }
                    })
            };
            let mut model_id = r.model_id.as_deref().map(str::trim)
                .filter(|id| !id.is_empty()).map(str::to_string);
            if has_model_usage {
                let query = format!(
                    "SELECT model FROM session_model_usage WHERE session_id = ?1{main_task} ORDER BY last_seen DESC"
                );
                let mut statement = conn.prepare(&query).map_err(|error| error.to_string())?;
                let models = statement.query_map([&r.id], |row| row.get::<_, Option<String>>(0))
                    .map_err(|error| error.to_string())?;
                for model in models {
                    let model = model.map_err(|error| error.to_string())?;
                    if let Some(model) = model.as_deref().map(str::trim).filter(|id| !id.is_empty()) {
                        model_id = Some(model.to_string());
                        break;
                    }
                }
            }
            Ok(SavedSession {
                id: format!("hermes_native_{}", r.id),
                name,
                tool: "hermes".to_string(),
                cwd: r.cwd,
                // started_at is epoch seconds; the frontend wants ms.
                saved_at: ((r.started_at * 1000.0) as i64).to_string(),
                file_path: Some(db_str.clone()),
                turn_count: Some(turns_from_messages(r.msg_count.unwrap_or(0).max(0) as u32)),
                session_token: Some(r.id),
                model_id,
                message_count: r.conversation_count,
                input_tokens: r.usage.input,
                output_tokens: r.usage.output,
            })
        })
        .collect()
}

/// Hermes heatmap entries: one per session past the cutoff — its `started_at`
/// day + `message_count`. `started_at` is epoch seconds (float); we emit seconds.
fn collect_hermes_heatmap_entries_checked(
    db_path: &Path,
    cutoff_secs: i64,
    out: &mut Vec<HeatmapEntry>,
) -> Result<(), String> {
    if path_metadata(db_path)
        .map_err(|error| error.to_string())?
        .is_none()
    {
        return Ok(());
    }
    let conn = rusqlite::Connection::open_with_flags(
        db_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|error| error.to_string())?;
    let visibility = hermes_session_visibility(&conn).map_err(|error| error.to_string())?;
    let query = format!(
        "SELECT started_at, message_count FROM sessions \
         WHERE {visibility} AND started_at >= ?1"
    );
    let mut stmt = conn.prepare(&query).map_err(|error| error.to_string())?;
    let rows = stmt
        .query_map([cutoff_secs as f64], |row| {
            let started_at: f64 = row.get(0)?;
            let count: i64 = row.get::<_, Option<i64>>(1)?.unwrap_or(0);
            Ok((started_at, count))
        })
        .map_err(|error| error.to_string())?;
    for row in rows {
        let (started_at, count) = row.map_err(|error| error.to_string())?;
        if count > 0 {
            out.push(HeatmapEntry {
                ts: started_at as i64,
                count: count as u32,
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod sqlite_cache_read_tests {
    use super::*;

    fn fixture_dir() -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("echobird-sqlite-cache-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn missing_databases_are_empty_but_unreadable_databases_are_errors() {
        let dir = fixture_dir();
        let path = dir.join("missing.db");
        assert!(
            drizzle_history_page_checked(&path, "opencode", "OpenCode", 0, 20)
                .unwrap()
                .is_empty()
        );
        assert!(hermes_history_page_checked(&path, 0, 20)
            .unwrap()
            .is_empty());
        let mut heatmap = Vec::new();
        collect_drizzle_heatmap_entries_checked(&path, 0, &mut heatmap).unwrap();
        collect_hermes_heatmap_entries_checked(&path, 0, &mut heatmap).unwrap();
        assert!(!path.exists());
        assert!(heatmap.is_empty());
        std::fs::write(&path, b"not a sqlite database").unwrap();
        let before = std::fs::read(&path).unwrap();
        assert!(drizzle_history_page_checked(&path, "opencode", "OpenCode", 0, 20).is_err());
        assert!(hermes_history_page_checked(&path, 0, 20).is_err());
        assert!(collect_drizzle_heatmap_entries_checked(&path, 0, &mut heatmap).is_err());
        assert!(collect_hermes_heatmap_entries_checked(&path, 0, &mut heatmap).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), before);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn sqlite_row_failures_do_not_publish_partial_history_or_usage() {
        let dir = fixture_dir();
        let path = dir.join("fixture.db");
        let connection = rusqlite::Connection::open(&path).unwrap();
        connection.execute_batch(
            "CREATE TABLE session (id TEXT, title TEXT, directory TEXT, time_updated, time_archived INTEGER, parent_id TEXT);
             CREATE TABLE message (id TEXT, session_id TEXT, data);
             INSERT INTO session VALUES ('fixture', 'Fixture task', '/fixture', 1000, NULL, NULL);
             CREATE TABLE sessions (id TEXT, title TEXT, cwd TEXT, started_at, message_count INTEGER, archived INTEGER);
             INSERT INTO sessions VALUES ('fixture', 'Fixture task', '/fixture', 1000, 2, 0);"
        ).unwrap();
        assert_eq!(
            drizzle_history_page_checked(&path, "opencode", "OpenCode", 0, 20)
                .unwrap()
                .len(),
            1
        );
        assert_eq!(hermes_history_page_checked(&path, 0, 20).unwrap().len(), 1);
        connection
            .execute_batch("UPDATE sessions SET title='';")
            .unwrap();
        assert_eq!(
            hermes_history_page_checked(&path, 0, 20).unwrap()[0].name,
            "fixture"
        );
        connection
            .execute_batch("CREATE TABLE messages (session_id TEXT, content TEXT);")
            .unwrap();
        assert!(hermes_history_page_checked(&path, 0, 20).is_err());
        connection.execute_batch("DROP TABLE messages;").unwrap();
        connection
            .execute_batch("INSERT INTO message VALUES ('unknown', 'fixture', NULL);")
            .unwrap();
        let unknown = drizzle_history_page_checked(&path, "opencode", "OpenCode", 0, 20).unwrap();
        assert_eq!(
            (unknown[0].input_tokens, unknown[0].output_tokens),
            (None, None)
        );
        connection
            .execute_batch("INSERT INTO message VALUES ('usage', 'fixture', 123);")
            .unwrap();
        assert!(drizzle_history_page_checked(&path, "opencode", "OpenCode", 0, 20).is_err());
        connection.execute_batch("DELETE FROM message; UPDATE session SET time_updated='invalid'; UPDATE sessions SET started_at='invalid';").unwrap();
        let mut heatmap = Vec::new();
        assert!(drizzle_history_page_checked(&path, "opencode", "OpenCode", 0, 20).is_err());
        assert!(hermes_history_page_checked(&path, 0, 20).is_err());
        assert!(collect_drizzle_heatmap_entries_checked(&path, 0, &mut heatmap).is_err());
        assert!(collect_hermes_heatmap_entries_checked(&path, 0, &mut heatmap).is_err());
        drop(connection);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn readonly_history_reloads_wal_changes_without_a_main_database_fingerprint_change() {
        let dir = fixture_dir();
        let path = dir.join("wal.db");
        let connection = rusqlite::Connection::open(&path).unwrap();
        connection.execute_batch(
            "PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0;
             CREATE TABLE session (id TEXT, title TEXT, directory TEXT, time_updated INTEGER, time_archived INTEGER, parent_id TEXT);
             CREATE TABLE message (id TEXT, session_id TEXT, data TEXT);
             INSERT INTO session VALUES ('fixture', 'Before', '/fixture', 1000, NULL, NULL);
             INSERT INTO message VALUES ('before', 'fixture', '{\"role\":\"assistant\",\"finish\":\"stop\",\"tokens\":{\"input\":10,\"output\":2}}');"
        ).unwrap();
        let before = drizzle_history_page_checked(&path, "opencode", "OpenCode", 0, 20).unwrap();
        assert_eq!(
            (before[0].name.as_str(), before[0].input_tokens),
            ("Before", Some(10))
        );
        let metadata = std::fs::metadata(&path).unwrap();
        connection.execute_batch(
            "UPDATE session SET title='After', time_updated=2000;
             INSERT INTO message VALUES ('after', 'fixture', '{\"role\":\"assistant\",\"finish\":\"stop\",\"tokens\":{\"input\":20,\"output\":3}}');"
        ).unwrap();
        let after_metadata = std::fs::metadata(&path).unwrap();
        assert_eq!(metadata.len(), after_metadata.len());
        assert_eq!(
            metadata.modified().unwrap(),
            after_metadata.modified().unwrap()
        );
        let after = drizzle_history_page_checked(&path, "opencode", "OpenCode", 0, 20).unwrap();
        assert_eq!(
            (
                after[0].name.as_str(),
                after[0].input_tokens,
                after[0].output_tokens
            ),
            ("After", Some(30), Some(5))
        );
        let mut heatmap = Vec::new();
        collect_drizzle_heatmap_entries_checked(&path, 0, &mut heatmap).unwrap();
        assert_eq!((heatmap[0].ts, heatmap[0].count), (2, 2));
        drop(connection);
        std::fs::remove_dir_all(dir).unwrap();
    }
}

// ─── DeepSeek Harness (dsh) ──────────────────────────────────────────────
//
// DeepSeek Harness persists each session as an append-only event log under
// `<dshHome>/sessions/<project-key>/<session-id>/session.v4.jsonl[.zstd]`
// (or the older `session.jsonl[.zstd]`). The default compression is
// checksummed concatenated Zstandard frames; plain `.jsonl` is also supported.
// The first line is the session header (`{"type":"session","version":4,
// "id":...,"cwd":...,"createdAt":...}`); each following line is a
// `SessionEvent` (`type`, monotonic `seq`, epoch-ms `time`, `data`). We read
// the store directly like the other families — independent of EchoBird's dsh
// model config.

/// Collect every DeepSeek Harness session log under `root`, i.e. all
/// `<project>/<session>/session.v4.jsonl[.zstd]` or legacy logs (depth 3).
/// Prefer v4 when both versions remain in the same session directory.
fn collect_dsh_paths(
    root: &Path,
    family: Family,
    out: &mut Vec<(SystemTime, PathBuf, Family)>,
) -> std::io::Result<()> {
    let Some(projects) = read_directory(root)? else {
        return Ok(());
    };
    for project in projects {
        let project = project?;
        let p = project.path();
        if !path_metadata(&p)?.is_some_and(|metadata| metadata.is_dir()) {
            continue;
        }
        let Some(sessions) = read_directory(&p)? else {
            continue;
        };
        for session in sessions {
            let session = session?;
            let sp = session.path();
            if !path_metadata(&sp)?.is_some_and(|metadata| metadata.is_dir()) {
                continue;
            }
            let Some(files) = read_directory(&sp)? else {
                continue;
            };
            let mut selected: Option<(u8, SystemTime, PathBuf)> = None;
            for f in files {
                let f = f?;
                let path = f.path();
                let Some(metadata) = path_metadata(&path)? else {
                    continue;
                };
                if !metadata.is_file() {
                    continue;
                }
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                let priority = match name {
                    "session.v4.jsonl" | "session.v4.jsonl.zstd" => 2,
                    "session.jsonl" | "session.jsonl.zstd" => 1,
                    _ => continue,
                };
                let mtime = metadata.modified()?;
                if selected.as_ref().map_or(true, |(current, time, _)| {
                    priority > *current || (priority == *current && mtime > *time)
                }) {
                    selected = Some((priority, mtime, path));
                }
            }
            if let Some((_, mtime, path)) = selected {
                out.push((mtime, path, family));
            }
        }
    }
    Ok(())
}

/// Decompress a DeepSeek Harness session-log artifact: `.zstd` files are
/// concatenated checksummed Zstandard frames; `.jsonl` is raw UTF-8 text.
/// libzstd's streaming decoder continues across frame boundaries, so one
/// `read_to_end` yields the whole log (header + every append batch).
fn dsh_decode_checked(path: &Path) -> std::io::Result<Vec<u8>> {
    let raw = std::fs::read(path)?;
    if path.extension().and_then(|e| e.to_str()) == Some("zstd") {
        use std::io::Read;
        let mut decoder = zstd::stream::read::Decoder::new(raw.as_slice())?;
        let mut out = Vec::new();
        decoder.read_to_end(&mut out)?;
        Ok(out)
    } else {
        Ok(raw)
    }
}

/// First non-empty text out of a dsh `UserMessage.content` — a plain string
/// or a `ContentBlock[]` (reuses the Hermes `first_json_text` walker).
fn dsh_first_text(v: Option<&serde_json::Value>) -> Option<String> {
    v.and_then(first_json_text)
}

/// Display title for a dsh session: the harness's own latest `session/title`
/// event wins (it is authoritative, latest-wins snapshot); fall back to the
/// first real `user/message` text. Empty (no real user input) → generic label.
fn dsh_title_from_events(events: &[serde_json::Value]) -> String {
    let mut title: Option<String> = None;
    let mut first_user: Option<String> = None;
    for ev in events {
        let t = ev.get("type").and_then(|v| v.as_str()).unwrap_or("");
        if t == "session/title" {
            if let Some(s) = ev.pointer("/data/title").and_then(|v| v.as_str()) {
                if !s.trim().is_empty() {
                    title = Some(s.to_string());
                }
            }
        } else if t == "user/message" && first_user.is_none() {
            if let Some(text) = dsh_first_text(ev.pointer("/data/content")) {
                if !is_system_injected(&text) {
                    first_user = Some(text);
                }
            }
        }
    }
    title
        .or(first_user)
        .unwrap_or_else(|| "DeepSeek Session".to_string())
}

/// Approximate turn count: one per `turn/start` event (each opens a model-loop
/// turn; a crash may leave one unclosed, which is still one real turn).
fn dsh_turn_count(events: &[serde_json::Value]) -> u32 {
    events
        .iter()
        .filter(|ev| ev.get("type").and_then(|v| v.as_str()) == Some("turn/start"))
        .count() as u32
}

fn dsh_owns_usage(header: &serde_json::Value, event: &serde_json::Value) -> Option<bool> {
    let seed_length = match header.get("seedLength") {
        Some(length) => length.as_u64()?,
        None => 0,
    };
    Some(seed_length == 0 || event.get("seq")?.as_u64()? >= seed_length)
}

/// Parse one DeepSeek Harness session log into a `SavedSession`. The header
/// line carries id + cwd; events are scanned for the title + turn count.
/// `saved_at` is the artifact mtime (last append), matching the other JSONL
/// families and the mtime-keyed heatmap cache.
#[cfg(test)]
fn parse_dsh_session(path: &Path) -> Option<SavedSession> {
    parse_dsh_session_checked(path).ok().flatten()
}

fn parse_dsh_session_checked(path: &Path) -> Result<Option<SavedSession>, String> {
    let bytes = dsh_decode_checked(path).map_err(|e| e.to_string())?;
    let text = std::str::from_utf8(&bytes).map_err(|e| e.to_string())?;
    let mut lines = text.lines();
    let Some(first) = lines.next() else {
        return Ok(None);
    };
    let header: serde_json::Value = serde_json::from_str(first).map_err(|e| e.to_string())?;
    let cwd = header
        .get("cwd")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let session_id = header
        .get("id")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| {
            path.parent()
                .and_then(|d| d.file_name())
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default()
        });

    let events: Vec<serde_json::Value> = lines
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();

    // Created-but-never-appended sessions materialize as a bare header frame
    // and leave no events behind; drop them like Claude/Codex drop sessions
    // with no real user message (nothing to show in the history list).
    if events.is_empty() {
        return Ok(None);
    }

    let title = dsh_title_from_events(&events);
    let turn_count = dsh_turn_count(&events);
    let mut model_id = None;
    let mut message_count = 0u32;
    let mut usage_by_step = HashMap::new();
    for event in &events {
        let kind = event.get("type").and_then(|v| v.as_str()).unwrap_or("");
        let Some(data) = event.get("data") else {
            continue;
        };
        if kind == "user/message" || kind == "assistant/message" {
            message_count += 1;
        }
        if kind == "assistant/message" {
            if let Some(model) =
                session_model_id(data.pointer("/message/source/replayState/response/responseModel"))
                    .or_else(|| {
                        session_model_id(data.pointer("/message/source/replayState/response/model"))
                    })
                    .or_else(|| session_model_id(data.pointer("/message/source/model")))
            {
                model_id = Some(model);
            }
        }
        let sample = match kind {
            "assistant/message" => data.get("usage"),
            "assistant/chunk"
                if data.pointer("/chunk/type").and_then(|v| v.as_str()) == Some("usage") =>
            {
                data.pointer("/chunk/usage")
            }
            _ => None,
        };
        if let (Some(sample), Some(turn), Some(step)) = (
            sample,
            data.get("turn").and_then(|v| v.as_u64()),
            data.get("step").and_then(|v| v.as_u64()),
        ) {
            if dsh_owns_usage(&header, event) != Some(true) {
                continue;
            }
            usage_by_step.insert(
                (turn, step),
                TokenUsage::read(sample, "inputTokens", "outputTokens"),
            );
        }
    }
    let mut usage = TokenUsage::default();
    for sample in usage_by_step.into_values() {
        usage.add(sample);
    }

    Ok(Some(SavedSession {
        id: format!("deepseek_native_{}", session_id),
        name: make_title(&title),
        tool: "deepseek".to_string(),
        cwd,
        session_token: Some(session_id),
        saved_at: mtime_millis(path),
        file_path: Some(path.to_string_lossy().into_owned()),
        turn_count: (turn_count > 0).then_some(turn_count),
        model_id,
        message_count: Some(message_count),
        input_tokens: usage.input,
        output_tokens: usage.output,
    }))
}

/// Heatmap count for a dsh session log: number of event lines (decompressing
/// zstd first, capped like `count_jsonl_message_lines`). The header line is
/// excluded — it is storage metadata, not a message.
#[cfg(test)]
fn count_dsh_event_lines(path: &Path) -> u32 {
    count_dsh_event_lines_checked(path).unwrap_or(0)
}

fn count_dsh_event_lines_checked(path: &Path) -> std::io::Result<u32> {
    const MAX_BYTES: u64 = 32 * 1024 * 1024;
    if std::fs::metadata(path)?.len() > MAX_BYTES {
        return Ok(0);
    }
    let bytes = if path.extension().and_then(|e| e.to_str()) == Some("zstd") {
        let bytes = dsh_decode_checked(path)?;
        if bytes.len() as u64 > MAX_BYTES {
            return Ok(0);
        }
        bytes
    } else {
        std::fs::read(path)?
    };
    Ok(bytes
        .split(|&b| b == b'\n')
        .skip(1) // header
        .filter(|line| !line.iter().all(|&b| b.is_ascii_whitespace()))
        .count() as u32)
}

/// Total on-disk size of every DeepSeek session artifact under `root`
/// (stat-only; zstd files stay compressed — the byte size is just a proxy for
/// content volume like the other families).
fn sum_dsh_sizes(root: &Path, cap: u64, total: &mut u64) -> std::io::Result<()> {
    let mut candidates = Vec::new();
    collect_dsh_paths(root, Family::DeepSeek, &mut candidates)?;
    for (_, p, _) in candidates {
        if let Some(m) = path_metadata(&p)? {
            *total = total.saturating_add(m.len().min(cap));
        }
    }
    Ok(())
}

// ─── Heatmap count cache ─────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Clone)]
struct CachedCount {
    mtime: i64,
    count: u32,
    #[serde(default)]
    size: u64,
}

#[derive(Serialize, Deserialize)]
struct CachedSession {
    stamp: persistent_cache::FileStamp,
    #[serde(default)]
    pages: Vec<(PathBuf, persistent_cache::FileStamp)>,
    session: Option<SavedSession>,
}

fn session_cache() -> &'static Mutex<HashMap<PathBuf, CachedSession>> {
    static CACHE: OnceLock<Mutex<HashMap<PathBuf, CachedSession>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

#[cfg(test)]
fn parse_session_cached(path: &Path, family: Family) -> Option<SavedSession> {
    parse_session_cached_in(session_cache(), path, family)
        .ok()
        .flatten()
}

#[cfg(test)]
fn parse_session_cached_in(
    cache: &Mutex<HashMap<PathBuf, CachedSession>>,
    path: &Path,
    family: Family,
) -> Result<Option<SavedSession>, String> {
    parse_session_pages_cached_in(cache, &[path.to_path_buf()], family)
}

fn parse_session_pages_cached_in(
    cache: &Mutex<HashMap<PathBuf, CachedSession>>,
    paths: &[PathBuf],
    family: Family,
) -> Result<Option<SavedSession>, String> {
    let path = &paths[0];
    let Some(pages) = codex_pages::stamps(paths)? else {
        if let Ok(mut entries) = cache.lock() {
            entries.remove(path);
        }
        return Ok(None);
    };
    let stamp = pages[0].1.clone();
    if let Ok(cache) = cache.lock() {
        if let Some(entry) = cache.get(path) {
            if entry.stamp == stamp && entry.pages == pages {
                return Ok(entry.session.clone());
            }
        }
    }
    #[cfg(test)]
    SESSION_PARSE_COUNT.with(|count| count.set(count.get() + 1));
    let result = match family {
        Family::Claude => parse_agent_jsonl_checked(path, family),
        Family::Codex => parse_codex_pages_checked(paths),
        Family::DeepSeek => parse_dsh_session_checked(path),
        _ => Ok(None),
    };
    let mut guard = cache
        .lock()
        .map_err(|_| "session cache lock failed".to_string())?;
    let Some(after) = codex_pages::stamps(paths)? else {
        guard.remove(path);
        return Ok(None);
    };
    let session = result?;
    if after == pages {
        guard.insert(
            path.to_path_buf(),
            CachedSession {
                stamp,
                pages,
                session: session.clone(),
            },
        );
    }
    Ok(session)
}

#[cfg(test)]
thread_local! {
    static SESSION_PARSE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

fn parsed_cache_path(name: &str) -> PathBuf {
    crate::utils::platform::echobird_dir()
        .join("cache")
        .join(name)
}

fn hydrate_session_cache() {
    static HYDRATED: OnceLock<()> = OnceLock::new();
    HYDRATED.get_or_init(|| {
        if let Ok(mut cache) = session_cache().lock() {
            for (path, value) in
                persistent_cache::read(&parsed_cache_path("ai-career-sessions-v2.json"))
            {
                cache.entry(path).or_insert(value);
            }
        }
    });
}

fn flush_session_cache() {
    if let Ok(cache) = session_cache().lock() {
        let _ = persistent_cache::write(&parsed_cache_path("ai-career-sessions-v2.json"), &cache);
    }
}

fn count_cache_path() -> PathBuf {
    crate::utils::platform::echobird_dir()
        .join("cache")
        .join("ai-career-heatmap-counts.json")
}

fn read_count_cache(path: &Path) -> HashMap<String, CachedCount> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|c| serde_json::from_str(&c).ok())
        .unwrap_or_default()
}

fn write_count_cache(path: &Path, map: &HashMap<String, CachedCount>) {
    if let Ok(json) = serde_json::to_string(map) {
        let _ = persistent_cache::replace(path, json.as_bytes());
    }
}

// ─── Public entry points (called by ai_career_commands) ──────────────────

/// One page of a single family's session history, newest first. Per-family
/// scanning keeps the payload small; the frontend pages in more on scroll.
pub fn family_history(
    family: Family,
    offset: usize,
    limit: usize,
) -> Result<Vec<SavedSession>, String> {
    static REQUEST: Mutex<()> = Mutex::new(());
    let _request = REQUEST.lock().map_err(|e| e.to_string())?;
    let home = dirs::home_dir().ok_or_else(|| "home directory unavailable".to_string())?;
    hydrate_session_cache();
    let result = family_history_at_checked(&home, family, offset, limit);
    if result.is_ok() {
        flush_session_cache();
    }
    result
}

#[cfg(test)]
fn family_history_at(
    home: &Path,
    family: Family,
    offset: usize,
    limit: usize,
) -> Vec<SavedSession> {
    family_history_at_checked(home, family, offset, limit).unwrap()
}

fn family_history_at_checked(
    home: &Path,
    family: Family,
    offset: usize,
    limit: usize,
) -> Result<Vec<SavedSession>, String> {
    if limit == 0 {
        return Ok(Vec::new());
    }

    // OpenCode + MiMo (its fork) + Hermes keep sessions in a SQLite db, not flat
    // files. OpenCode/MiMo share one reader (`drizzle_*`); Hermes has its own.
    if family == Family::OpenCode {
        return drizzle_history_page_checked(
            &Family::OpenCode.root(home).join("opencode.db"),
            "opencode",
            "OpenCode",
            offset,
            limit,
        );
    }
    if family == Family::MiMo {
        return drizzle_history_page_checked(&mimo_db_path(home), "mimo", "MiMo", offset, limit);
    }
    if family == Family::Hermes {
        return hermes_history_page_checked(
            &Family::Hermes.root(home).join("state.db"),
            offset,
            limit,
        );
    }
    family_history_files_at(&family.root(home), family, offset, limit, session_cache())
}

fn family_history_files_at(
    root: &Path,
    family: Family,
    offset: usize,
    limit: usize,
    cache: &Mutex<HashMap<PathBuf, CachedSession>>,
) -> Result<Vec<SavedSession>, String> {
    // DeepSeek Harness keeps one event log per session (`session.jsonl.zstd`
    // by default) — collect + parse those directly.
    let mut candidates: Vec<(SystemTime, PathBuf, Family)> = Vec::new();
    match family {
        Family::Claude | Family::Codex => {
            if let Some(depth) = family.jsonl_depth() {
                collect_jsonl_paths(root.to_path_buf(), depth, family, &mut candidates)
                    .map_err(|e| e.to_string())?;
            }
        }
        Family::DeepSeek => {
            collect_dsh_paths(root, family, &mut candidates).map_err(|e| e.to_string())?
        }
        Family::OpenCode | Family::Hermes | Family::MiMo => unreachable!(),
    }

    candidates.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    let mut sessions = Vec::new();
    let mut valid = 0;
    let groups = if family == Family::Codex {
        codex_pages::groups(&candidates)?
    } else {
        candidates
            .iter()
            .map(|(_, path, _)| vec![path.clone()])
            .collect()
    };
    let cache_keys: HashSet<_> = groups
        .iter()
        .filter_map(|paths| paths.first().cloned())
        .collect();
    for paths in groups {
        if let Some(session) = parse_session_pages_cached_in(cache, &paths, family)? {
            if valid >= offset {
                sessions.push(session);
            }
            valid += 1;
            if sessions.len() == limit {
                break;
            }
        }
    }
    if let Ok(mut cache) = cache.lock() {
        cache.retain(|path, _| !path.starts_with(root) || cache_keys.contains(path));
    }
    if family == Family::Codex {
        apply_codex_thread_names(root, &mut sessions);
    }
    Ok(sessions)
}

/// Contribution-heatmap entries across all six families, 210-day lookback.
/// Past session files are immutable once their mtime settles, so per-file
/// counts are cached on disk and skipped on subsequent scans.
pub fn message_heatmap(selected_family: Option<Family>) -> Result<Vec<HeatmapEntry>, String> {
    let home = dirs::home_dir().ok_or_else(|| "home directory unavailable".to_string())?;
    message_heatmap_at_checked(&home, selected_family, &count_cache_path())
}

#[cfg(test)]
fn message_heatmap_at(
    home: &Path,
    selected_family: Option<Family>,
    cache_path: &Path,
) -> Vec<HeatmapEntry> {
    message_heatmap_at_checked(home, selected_family, cache_path).unwrap()
}

fn message_heatmap_at_checked(
    home: &Path,
    selected_family: Option<Family>,
    cache_path: &Path,
) -> Result<Vec<HeatmapEntry>, String> {
    static REQUEST: Mutex<()> = Mutex::new(());
    let _request = REQUEST
        .lock()
        .map_err(|_| "count cache lock failed".to_string())?;
    const LOOKBACK_SECS: u64 = 210 * 86_400;
    let now = SystemTime::now();
    let cutoff = now
        .checked_sub(Duration::from_secs(LOOKBACK_SECS))
        .unwrap_or(UNIX_EPOCH);

    // Collect file candidates for the JSONL families only (OpenCode, MiMo, and
    // Hermes are separate SQLite passes below).
    let mut candidates: Vec<(SystemTime, PathBuf, Family)> = Vec::new();
    for family in Family::ALL {
        if selected_family.is_some_and(|selected| selected != family) {
            continue;
        }
        match family {
            Family::Claude | Family::Codex => {
                if let Some(depth) = family.jsonl_depth() {
                    collect_jsonl_paths(family.root(home), depth, family, &mut candidates)
                        .map_err(|e| e.to_string())?;
                }
            }
            Family::DeepSeek => {
                collect_dsh_paths(&family.root(home), family, &mut candidates)
                    .map_err(|e| e.to_string())?;
            }
            Family::OpenCode | Family::Hermes | Family::MiMo => {}
        }
    }

    let mut cache = read_count_cache(cache_path);
    let mut cache_dirty = false;
    let mut keep: HashSet<String> = HashSet::new();
    let mut out: Vec<HeatmapEntry> = Vec::with_capacity(candidates.len());

    for (mtime, path, family) in &candidates {
        if *mtime < cutoff {
            continue;
        }
        let ts = mtime
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let key = path.to_string_lossy().into_owned();
        let Some(metadata) = path_metadata(path).map_err(|e| e.to_string())? else {
            continue;
        };
        keep.insert(key.clone());
        let size = metadata.len();
        let stamp = mtime
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos() as i64)
            .unwrap_or(0);

        let count = match cache.get(&key) {
            Some(entry) if entry.mtime == stamp && entry.size == size => entry.count,
            _ => {
                // DeepSeek logs are zstd by default — count via its own
                // decompress-then-count path; everyone else counts lines.
                let result = match family {
                    Family::DeepSeek => count_dsh_event_lines_checked(path),
                    _ => count_jsonl_message_lines_checked(path),
                };
                let c = match result {
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                        keep.remove(&key);
                        continue;
                    }
                    result => result.map_err(|e| e.to_string())?,
                };
                let Some(after) = path_metadata(path).map_err(|e| e.to_string())? else {
                    keep.remove(&key);
                    continue;
                };
                if after.modified().map_err(|e| e.to_string())? == *mtime && after.len() == size {
                    cache.insert(
                        key.clone(),
                        CachedCount {
                            mtime: stamp,
                            count: c,
                            size,
                        },
                    );
                    cache_dirty = true;
                }
                c
            }
        };
        if count > 0 {
            out.push(HeatmapEntry { ts, count });
        }
    }

    // Prune entries for files that vanished from disk.
    let before = cache.len();
    cache.retain(|key, _| {
        selected_family.is_some_and(|family| !Path::new(key).starts_with(family.root(home)))
            || keep.contains(key)
    });
    if cache.len() != before {
        cache_dirty = true;
    }

    // SQLite second pass — OpenCode, MiMo (its fork), and Hermes keep sessions
    // in a SQLite db (not flat files). Each collector opens its db read-only and
    // returns early if it's missing, so no is_file() guard is needed.
    let cutoff_secs = cutoff
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    if selected_family.map_or(true, |family| family == Family::OpenCode) {
        collect_drizzle_heatmap_entries_checked(
            &Family::OpenCode.root(home).join("opencode.db"),
            cutoff_secs,
            &mut out,
        )?;
    }
    if selected_family.map_or(true, |family| family == Family::MiMo) {
        collect_drizzle_heatmap_entries_checked(&mimo_db_path(home), cutoff_secs, &mut out)?;
    }
    if selected_family.map_or(true, |family| family == Family::Hermes) {
        collect_hermes_heatmap_entries_checked(
            &Family::Hermes.root(home).join("state.db"),
            cutoff_secs,
            &mut out,
        )?;
    }

    if cache_dirty {
        write_count_cache(cache_path, &cache);
    }
    Ok(out)
}

/// Rough "≈ N tokens" estimate: sum the on-disk byte size of every session
/// file across the six families (capped per file), which the frontend
/// divides by a bytes-per-token ratio. Deliberately approximate — it measures
/// content volume, so it works even when a provider doesn't report real usage
/// (third-party models often log 0 tokens). Stat-only (no file reads), cheap.
pub fn estimate_token_bytes(selected_family: Option<Family>) -> Result<u64, String> {
    let home = dirs::home_dir().ok_or_else(|| "home directory unavailable".to_string())?;
    estimate_token_bytes_at_checked(&home, selected_family).map_err(|e| e.to_string())
}

#[cfg(test)]
fn estimate_token_bytes_at(home: &Path, selected_family: Option<Family>) -> u64 {
    estimate_token_bytes_at_checked(home, selected_family).unwrap()
}

fn estimate_token_bytes_at_checked(
    home: &Path,
    selected_family: Option<Family>,
) -> std::io::Result<u64> {
    const MAX_PER_FILE: u64 = 32 * 1024 * 1024;
    let mut total: u64 = 0;
    for family in Family::ALL {
        if selected_family.is_some_and(|selected| selected != family) {
            continue;
        }
        match family {
            Family::Hermes => {
                // Hermes keeps all sessions in one SQLite db; its file size
                // stands in for the family's content volume.
                if let Some(m) = path_metadata(&family.root(home).join("state.db"))? {
                    total = total.saturating_add(m.len());
                }
            }
            Family::Claude | Family::Codex => {
                if let Some(depth) = family.jsonl_depth() {
                    sum_jsonl_sizes(family.root(home), depth, MAX_PER_FILE, &mut total)?;
                }
            }
            Family::OpenCode => {
                // One shared DB holds every OpenCode session; its file size
                // stands in for the family's content volume.
                if let Some(m) = path_metadata(&family.root(home).join("opencode.db"))? {
                    total = total.saturating_add(m.len());
                }
            }
            Family::MiMo => {
                // MiMo Code's SQLite db (its OpenCode fork's `mimocode.db`).
                if let Some(m) = path_metadata(&mimo_db_path(home))? {
                    total = total.saturating_add(m.len());
                }
            }
            Family::DeepSeek => {
                // One event-log file per DeepSeek session (`session.jsonl.zstd`
                // by default); their sizes stand in for the family's volume.
                sum_dsh_sizes(&family.root(home), MAX_PER_FILE, &mut total)?;
            }
        }
    }
    Ok(total)
}

fn sum_jsonl_sizes(dir: PathBuf, depth: u8, cap: u64, total: &mut u64) -> std::io::Result<()> {
    if depth == 0 {
        return Ok(());
    }
    let Some(entries) = read_directory(&dir)? else {
        return Ok(());
    };
    for e in entries {
        let e = e?;
        let p = e.path();
        let Some(m) = path_metadata(&p)? else {
            continue;
        };
        if m.is_file() {
            if p.extension().and_then(|x| x.to_str()) == Some("jsonl") {
                *total = total.saturating_add(m.len().min(cap));
            }
        } else if m.is_dir() {
            sum_jsonl_sizes(p, depth - 1, cap, total)?;
        }
    }
    Ok(())
}

fn resolve_session_file(path: &Path) -> Result<PathBuf, String> {
    if !path.is_absolute() {
        return Err("Session path must be absolute".to_string());
    }
    path.canonicalize()
        .map_err(|e| format!("Cannot resolve session path: {e}"))
}

pub fn reveal_session_file(path: &Path) -> Result<(), String> {
    let path = resolve_session_file(path)?;
    #[cfg(target_os = "windows")]
    {
        let value = path.to_string_lossy();
        let value = if let Some(unc) = value.strip_prefix(r"\\?\UNC\") {
            format!(r"\\{unc}")
        } else {
            value.strip_prefix(r"\\?\").unwrap_or(&value).to_string()
        };
        crate::utils::process::command("explorer.exe")
            .arg("/select,")
            .arg(value)
            .spawn()
            .map_err(|e| format!("Cannot open file manager: {e}"))?;
    }
    #[cfg(target_os = "macos")]
    {
        crate::utils::process::command("open")
            .arg("-R")
            .arg(&path)
            .spawn()
            .map_err(|e| format!("Cannot open file manager: {e}"))?;
    }
    #[cfg(target_os = "linux")]
    {
        let directory = if path.is_dir() {
            path.as_path()
        } else {
            path.parent().ok_or("Session path has no parent")?
        };
        crate::utils::process::command("xdg-open")
            .arg(directory)
            .spawn()
            .map_err(|e| format!("Cannot open file manager: {e}"))?;
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        return Err("File manager is unavailable on this platform".to_string());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;

    fn fixture_dir() -> PathBuf {
        let path = std::env::temp_dir().join(format!("echobird_career_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    fn write_jsonl(path: &Path, rows: &[serde_json::Value]) {
        let contents = rows
            .iter()
            .map(|row| format!("{row}\n"))
            .collect::<String>();
        std::fs::write(path, contents).unwrap();
    }

    #[test]
    fn session_metadata_reads_old_wire_data_without_guessing_messages() {
        let session: SavedSession = serde_json::from_value(serde_json::json!({
            "id":"fixture", "name":"Task", "tool":"codex", "cwd":"/fixture",
            "saved_at":"1000", "turn_count":3
        }))
        .unwrap();
        assert_eq!(session.model_id, None);
        assert_eq!(session.message_count, None);
        assert_eq!(session.turn_count, Some(3));
    }

    #[test]
    fn claude_metadata_deduplicates_messages_and_keeps_last_real_model() {
        let dir = fixture_dir();
        let path = dir.join("claude.jsonl");
        write_jsonl(
            &path,
            &[
                serde_json::json!({"uuid":"u1","message":{"role":"user","content":"Real task"}}),
                serde_json::json!({"message":{"id":"a1","role":"assistant","model":"claude-old","content":[{"type":"text","text":"First part"}]}}),
                serde_json::json!({"message":{"id":"a1","role":"assistant","model":"claude-old","content":[{"type":"tool_use","id":"tool-1"}]}}),
                serde_json::json!({"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"tool-1","content":"ok"}]}}),
                serde_json::json!({"message":{"id":"a2","role":"assistant","model":" claude-new ","content":"Final answer"}}),
                serde_json::json!({"message":{"id":"synthetic","role":"assistant","model":"<synthetic>","content":"Summary"}}),
                serde_json::json!({"message":{"role":"tool","model":"not-a-chat-model","content":"Internal"}}),
                serde_json::json!({"message":{"role":"system","content":"Internal"}}),
            ],
        );
        let session = parse_agent_jsonl(&path, Family::Claude).unwrap();
        assert_eq!(session.model_id.as_deref(), Some("claude-new"));
        assert_eq!(session.message_count, Some(3));
        assert_eq!(
            session.turn_count,
            Some(3),
            "existing turn estimate is unchanged"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn codex_metadata_tracks_context_and_settings_without_duplicate_event_messages() {
        let dir = fixture_dir();
        let path = dir.join("rollout.jsonl");
        let mut rows = vec![
            serde_json::json!({"type":"session_meta","payload":{"id":"fixture","cwd":"/fixture","source":"cli"}}),
            serde_json::json!({"type":"turn_context","payload":{"model":"gpt-old"}}),
            serde_json::json!({"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"Real task"}]}}),
            serde_json::json!({"type":"event_msg","payload":{"type":"user_message","message":"Real task"}}),
            serde_json::json!({"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"Answer"}]}}),
            serde_json::json!({"type":"response_item","payload":{"type":"function_call_output","output":"Internal"}}),
            serde_json::json!({"type":"response_item","payload":{"type":"message","role":"system","content":[]}}),
        ];
        write_jsonl(&path, &rows);
        let session = parse_codex_session_jsonl(&path).unwrap();
        assert_eq!(session.model_id.as_deref(), Some("gpt-old"));
        assert_eq!(session.message_count, Some(2));
        rows.push(serde_json::json!({"type":"thread_settings_applied","payload":{"settings":{"model":"gpt-new"}}}));
        rows.push(serde_json::json!({"type":"event_msg","payload":{"type":"thread_settings_applied","thread_settings":{"model":"gpt-latest"}}}));
        rows.push(serde_json::json!({"type":"turn_context","payload":{"model":"  "}}));
        rows.push(serde_json::json!({"type":"response_item","payload":{"type":"message","role":"assistant","content":[]}}));
        write_jsonl(&path, &rows);
        let session = parse_codex_session_jsonl(&path).unwrap();
        assert_eq!(session.model_id.as_deref(), Some("gpt-latest"));
        assert_eq!(session.message_count, Some(3));
        assert_eq!(session.turn_count, Some(2));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn dsh_metadata_counts_complete_messages_and_prefers_served_model() {
        let dir = fixture_dir();
        let rows = [
            serde_json::json!({"type":"session","version":0,"id":"fixture","cwd":"/fixture"}),
            serde_json::json!({"type":"turn/start","data":{"turn":0}}),
            serde_json::json!({"type":"user/message","data":{"content":[{"type":"text","text":"Real task"}]}}),
            serde_json::json!({"type":"assistant/chunk","data":{"chunk":{"type":"assistant/message","model":"wrong-streamed-model"}}}),
            serde_json::json!({"type":"assistant/message","data":{"message":{"source":{"model":"auto","replayState":{"response":{"responseModel":"deepseek-v4-pro"}}}}}}),
            serde_json::json!({"type":"tool/message","data":{"model":"wrong-tool-model"}}),
            serde_json::json!({"type":"assistant/message","data":{"message":{"source":{"model":"requested-model","replayState":{"response":{"model":"deepseek-v4-flash"}}}}}}),
            serde_json::json!({"type":"assistant/message","data":{"message":{"source":{"model":" "}}}}),
            serde_json::json!({"type":"turn/end","data":{"turn":0}}),
        ];
        let plain = dir.join("session.jsonl");
        write_jsonl(&plain, &rows);
        let compressed = dir.join("session.jsonl.zstd");
        std::fs::write(
            &compressed,
            zstd::stream::encode_all(std::fs::read(&plain).unwrap().as_slice(), 1).unwrap(),
        )
        .unwrap();
        for path in [&plain, &compressed] {
            let session = parse_dsh_session(path).unwrap();
            assert_eq!(session.model_id.as_deref(), Some("deepseek-v4-flash"));
            assert_eq!(session.message_count, Some(4));
            assert_eq!(session.turn_count, Some(1));
        }
        write_jsonl(&plain, &rows[..4]);
        let session = parse_dsh_session(&plain).unwrap();
        assert_eq!(session.model_id, None);
        assert_eq!(session.message_count, Some(1));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn claude_usage_replaces_streamed_samples_and_preserves_zero_and_missing() {
        let dir = fixture_dir();
        let path = dir.join("session.jsonl");
        let user = serde_json::json!({"message":{"role":"user","content":"Fixture question"}});
        write_jsonl(
            &path,
            &[
                user.clone(),
                serde_json::json!({"message":{"id":"a","role":"assistant","usage":{"input_tokens":10,"output_tokens":0,"cache_read_input_tokens":900}}}),
                serde_json::json!({"message":{"id":"a","role":"assistant","usage":{"input_tokens":10,"output_tokens":5}}}),
                serde_json::json!({"message":{"id":"b","role":"assistant","usage":{"input_tokens":20,"output_tokens":2}}}),
            ],
        );
        let session = parse_agent_jsonl(&path, Family::Claude).unwrap();
        assert_eq!(
            (session.input_tokens, session.output_tokens),
            (Some(30), Some(7))
        );
        write_jsonl(
            &path,
            &[
                user.clone(),
                serde_json::json!({"message":{"role":"assistant","usage":{"input_tokens":0,"output_tokens":0}}}),
            ],
        );
        let session = parse_agent_jsonl(&path, Family::Claude).unwrap();
        assert_eq!(
            (session.input_tokens, session.output_tokens),
            (Some(0), Some(0))
        );
        write_jsonl(&path, &[user]);
        let session = parse_agent_jsonl(&path, Family::Claude).unwrap();
        assert_eq!((session.input_tokens, session.output_tokens), (None, None));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn codex_usage_preserves_initial_cumulative_snapshot_without_double_counting() {
        let dir = fixture_dir();
        let path = dir.join("rollout.jsonl");
        write_jsonl(
            &path,
            &[
                serde_json::json!({"type":"session_meta","payload":{"id":"fixture","cwd":"/fixture","source":"cli"}}),
                serde_json::json!({"type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":10,"output_tokens":2}}}}),
                serde_json::json!({"type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":10,"output_tokens":2}}}}),
                serde_json::json!({"type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":25,"output_tokens":7},"last_token_usage":{"input_tokens":15,"output_tokens":5}}}}),
                serde_json::json!({"type":"event_msg","payload":{"type":"token_count","info":null}}),
            ],
        );
        let session = parse_codex_session_jsonl(&path).unwrap();
        assert_eq!(
            (session.input_tokens, session.output_tokens),
            (Some(25), Some(7))
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn deepseek_usage_replaces_chunk_with_final_sample_for_each_step() {
        let dir = fixture_dir();
        let path = dir.join("session.jsonl");
        write_jsonl(
            &path,
            &[
                serde_json::json!({"type":"session","id":"fixture","cwd":"/fixture"}),
                serde_json::json!({"type":"assistant/chunk","data":{"turn":0,"step":0,"chunk":{"type":"usage","usage":{"inputTokens":10,"outputTokens":2}}}}),
                serde_json::json!({"type":"assistant/message","data":{"turn":0,"step":0,"usage":{"inputTokens":10,"outputTokens":5,"cacheReadTokens":900}}}),
                serde_json::json!({"type":"assistant/message","data":{"turn":0,"step":1,"usage":{"inputTokens":20,"outputTokens":3}}}),
            ],
        );
        let session = parse_dsh_session(&path).unwrap();
        assert_eq!(
            (session.input_tokens, session.output_tokens),
            (Some(30), Some(8))
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn jsonl_pages_filter_internal_sessions_before_paging_and_cache_parsing() {
        let home = fixture_dir();
        let root = home.join(".codex/sessions");
        std::fs::create_dir_all(&root).unwrap();
        let stamp = UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        for index in 0..6 {
            let path = root.join(format!("{index}.jsonl"));
            let source = if index % 2 == 0 {
                serde_json::json!({"subagent":{}})
            } else {
                serde_json::json!("cli")
            };
            write_jsonl(
                &path,
                &[
                    serde_json::json!({"type":"session_meta","payload":{"id":format!("s{index}"),"cwd":"/fixture","source":source}}),
                    serde_json::json!({"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":format!("Question {index}")}]}}),
                ],
            );
            std::fs::File::options()
                .write(true)
                .open(&path)
                .unwrap()
                .set_modified(stamp)
                .unwrap();
        }
        let first = family_history_at(&home, Family::Codex, 0, 2);
        let second = family_history_at(&home, Family::Codex, 2, 2);
        assert_eq!(
            first
                .iter()
                .map(|s| s.session_token.as_deref().unwrap())
                .collect::<Vec<_>>(),
            ["s1", "s3"]
        );
        assert_eq!(
            second
                .iter()
                .map(|s| s.session_token.as_deref().unwrap())
                .collect::<Vec<_>>(),
            ["s5"]
        );
        assert!(family_history_at(&home, Family::Codex, 3, 2).is_empty());
        let path = root.join("1.jsonl");
        session_cache()
            .lock()
            .unwrap()
            .get_mut(&path)
            .unwrap()
            .session
            .as_mut()
            .unwrap()
            .name = "Cache hit marker".to_string();
        assert_eq!(
            family_history_at(&home, Family::Codex, 0, 2)[0].name,
            "Cache hit marker"
        );
        let original = std::fs::read_to_string(&path).unwrap();
        let changed = original.replace("Question 1", "Modified 1");
        assert_eq!(original.len(), changed.len());
        std::fs::write(&path, changed).unwrap();
        std::fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(stamp + Duration::from_millis(1))
            .unwrap();
        assert_eq!(
            family_history_at(&home, Family::Codex, 0, 2)[0].name,
            "Modified 1"
        );
        // Changing length invalidates a cache entry even if the mtime is preserved.
        session_cache()
            .lock()
            .unwrap()
            .get_mut(&path)
            .unwrap()
            .session
            .as_mut()
            .unwrap()
            .name = "Stale marker".to_string();
        std::fs::write(&path, format!("{original}\n")).unwrap();
        std::fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(stamp + Duration::from_millis(1))
            .unwrap();
        assert_eq!(
            parse_session_cached(&path, Family::Codex).unwrap().name,
            "Question 1"
        );
        std::fs::remove_file(&path).unwrap();
        assert!(family_history_at(&home, Family::Codex, 0, 10)
            .iter()
            .all(|s| s.session_token.as_deref() != Some("s1")));
        assert!(!session_cache().lock().unwrap().contains_key(&path));
        std::fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn family_stats_filter_sources_and_preserve_other_families_count_cache() {
        let home = fixture_dir();
        let claude = Family::Claude
            .root(&home)
            .join("project")
            .join("fixture.jsonl");
        let codex = Family::Codex.root(&home).join("fixture.jsonl");
        for path in [&claude, &codex] {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        }
        std::fs::write(&claude, "{}\n{}\n").unwrap();
        std::fs::write(&codex, "{}\n{}\n{}\n").unwrap();
        let cache_path = home.join("counts.json");
        assert_eq!(
            message_heatmap_at(&home, Some(Family::Codex), &cache_path)
                .iter()
                .map(|entry| entry.count)
                .collect::<Vec<_>>(),
            [3]
        );
        assert_eq!(
            message_heatmap_at(&home, Some(Family::Claude), &cache_path)
                .iter()
                .map(|entry| entry.count)
                .collect::<Vec<_>>(),
            [2]
        );
        assert!(read_count_cache(&cache_path).contains_key(codex.to_str().unwrap()));
        assert_eq!(estimate_token_bytes_at(&home, Some(Family::Claude)), 6);
        assert_eq!(estimate_token_bytes_at(&home, Some(Family::Codex)), 9);
        std::fs::remove_file(&claude).unwrap();
        assert!(message_heatmap_at(&home, Some(Family::Claude), &cache_path).is_empty());
        assert!(read_count_cache(&cache_path).contains_key(codex.to_str().unwrap()));
        assert!(!read_count_cache(&cache_path).contains_key(claude.to_str().unwrap()));
        std::fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn persisted_sessions_reuse_summaries_then_follow_changes_and_deletions() {
        let dir = fixture_dir();
        let root = dir.join("sessions");
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("fixture.jsonl");
        write_jsonl(
            &path,
            &[
                serde_json::json!({"type":"session_meta","payload":{"id":"fixture","cwd":"/fixture","source":"cli"}}),
                serde_json::json!({"type":"turn_context","payload":{"model":"initial-model"}}),
                serde_json::json!({"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"Original title"}]}}),
                serde_json::json!({"type":"response_item","payload":{"type":"message","role":"assistant","content":[]}}),
                serde_json::json!({"type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":10,"output_tokens":2}}}}),
            ],
        );
        let cache = Mutex::new(HashMap::new());
        SESSION_PARSE_COUNT.with(|count| count.set(0));
        let first = family_history_files_at(&root, Family::Codex, 0, 20, &cache).unwrap();
        assert_eq!(first[0].input_tokens, Some(10));
        assert_eq!(first[0].model_id.as_deref(), Some("initial-model"));
        assert_eq!(first[0].message_count, Some(2));
        assert_eq!(SESSION_PARSE_COUNT.with(|count| count.get()), 1);
        let cache_path = dir.join("cache.json");
        persistent_cache::write(&cache_path, &cache.lock().unwrap()).unwrap();
        let reloaded = Mutex::new(persistent_cache::read(&cache_path));
        SESSION_PARSE_COUNT.with(|count| count.set(0));
        assert_eq!(
            family_history_files_at(&root, Family::Codex, 0, 20, &reloaded).unwrap()[0].name,
            "Original title"
        );
        assert_eq!(SESSION_PARSE_COUNT.with(|count| count.get()), 0);
        {
            let cache = reloaded.lock().unwrap();
            let cached = cache.get(&path).unwrap().session.as_ref().unwrap();
            assert_eq!(cached.model_id.as_deref(), Some("initial-model"));
            assert_eq!(cached.message_count, Some(2));
        }
        // Index names are applied after cache reuse, so native renames stay fresh.
        std::fs::write(
            dir.join("session_index.jsonl"),
            "{\"id\":\"fixture\",\"thread_name\":\"Renamed thread\"}\n",
        )
        .unwrap();
        assert_eq!(
            family_history_files_at(&root, Family::Codex, 0, 20, &reloaded).unwrap()[0].name,
            "Renamed thread"
        );
        assert_eq!(SESSION_PARSE_COUNT.with(|count| count.get()), 0);
        let original = std::fs::read_to_string(&path).unwrap();
        let appended = serde_json::json!({"type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":15,"output_tokens":3}}}});
        let settings = serde_json::json!({"type":"event_msg","payload":{"type":"thread_settings_applied","thread_settings":{"model":"latest-model"}}});
        let message = serde_json::json!({"type":"response_item","payload":{"type":"message","role":"assistant","content":[]}});
        std::fs::write(
            &path,
            format!("{original}{settings}\n{message}\n{appended}\n"),
        )
        .unwrap();
        assert_eq!(
            family_history_files_at(&root, Family::Codex, 0, 20, &reloaded).unwrap()[0]
                .input_tokens,
            Some(15)
        );
        assert_eq!(SESSION_PARSE_COUNT.with(|count| count.get()), 1);
        {
            let cache = reloaded.lock().unwrap();
            let refreshed = cache.get(&path).unwrap().session.as_ref().unwrap();
            assert_eq!(refreshed.model_id.as_deref(), Some("latest-model"));
            assert_eq!(refreshed.message_count, Some(3));
        }
        let renamed = root.join("renamed.jsonl");
        std::fs::rename(&path, &renamed).unwrap();
        assert_eq!(
            family_history_files_at(&root, Family::Codex, 0, 20, &reloaded)
                .unwrap()
                .len(),
            1
        );
        assert!(!reloaded.lock().unwrap().contains_key(&path));
        assert_eq!(SESSION_PARSE_COUNT.with(|count| count.get()), 2);
        std::fs::remove_file(&renamed).unwrap();
        assert!(
            family_history_files_at(&root, Family::Codex, 0, 20, &reloaded)
                .unwrap()
                .is_empty()
        );
        assert!(reloaded.lock().unwrap().is_empty());
        persistent_cache::write(&cache_path, &reloaded.lock().unwrap()).unwrap();
        assert!(persistent_cache::read::<CachedSession>(&cache_path).is_empty());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn maintenance_codex_pages_form_one_session_before_list_pagination() {
        let dir = fixture_dir();
        let root = dir.join("sessions");
        std::fs::create_dir_all(&root).unwrap();
        for (file, prompt, paginated) in [
            ("rollout-1-thread.jsonl", "Original title", false),
            ("rollout-2-thread_page.jsonl", "Next question", true),
        ] {
            write_jsonl(
                &root.join(file),
                &[
                    serde_json::json!({"type":"session_meta","payload":{"id":"thread","cwd":"/fixture","history_mode":if paginated {"paginated"}else{"full"},"history_base":if paginated {serde_json::json!({"thread_id":"thread"})}else{serde_json::Value::Null}}}),
                    serde_json::json!({"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":prompt}]}}),
                    serde_json::json!({"type":"response_item","payload":{"type":"message","role":"assistant","content":[]}}),
                ],
            );
        }
        let cache = Mutex::new(HashMap::new());
        let sessions = family_history_files_at(&root, Family::Codex, 0, 20, &cache).unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].name, "Original title");
        assert_eq!(sessions[0].message_count, Some(4));
        assert!(family_history_files_at(&root, Family::Codex, 1, 20, &cache)
            .unwrap()
            .is_empty());
        write_jsonl(
            &root.join("rollout-3-thread_page.jsonl"),
            &[
                serde_json::json!({"type":"session_meta","payload":{"id":"thread","history_mode":"paginated"}}),
                serde_json::json!({"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"Third question"}]}}),
            ],
        );
        let changed = family_history_files_at(&root, Family::Codex, 0, 20, &cache).unwrap();
        assert_eq!(changed.len(), 1);
        assert_eq!(changed[0].message_count, Some(5));
        assert_eq!(changed[0].name, "Original title");
        std::fs::remove_file(root.join("rollout-1-thread.jsonl")).unwrap();
        let missing = family_history_files_at(&root, Family::Codex, 0, 20, &cache).unwrap();
        assert_eq!(missing.len(), 1);
        assert_eq!(missing[0].message_count, None);
        assert_eq!(missing[0].input_tokens, None);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn maintenance_native_fork_pages_follow_page_ids_and_detect_a_missing_middle() {
        let dir = fixture_dir();
        let thread = "11111111-1111-4111-8111-111111111111";
        let second = "22222222-2222-4222-8222-222222222222";
        let third = "33333333-3333-4333-8333-333333333333";
        let paths = [
            dir.join(format!("rollout-1-{thread}.jsonl")),
            dir.join(format!("rollout-2-{thread}_{second}.jsonl")),
            dir.join(format!("rollout-3-{thread}_{third}.jsonl")),
        ];
        for (index, (total, last)) in [(10, 10), (30, 20), (60, 30)].into_iter().enumerate() {
            let mut meta = serde_json::json!({"id":thread,"history_mode":"paginated","forked_from_id":"parent"});
            if index == 0 {
                meta["subagent_history_start_ordinal"] = serde_json::json!(5);
            } else {
                meta["history_base"] =
                    serde_json::json!({"thread_id":if index == 1 {thread}else{second}});
            }
            write_jsonl(
                &paths[index],
                &[
                    serde_json::json!({"type":"session_meta","payload":meta}),
                    serde_json::json!({"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":format!("question {index}")}]}}),
                    serde_json::json!({"type":"event_msg","ordinal":if index==0 {5}else{1},"timestamp":"2026-10-15T12:00:00Z","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":total,"output_tokens":total},"last_token_usage":{"input_tokens":last,"output_tokens":last}}}}),
                ],
            );
        }
        let cache = Mutex::new(HashMap::new());
        let sessions = family_history_files_at(&dir, Family::Codex, 0, 20, &cache).unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].message_count, Some(3));
        assert_eq!(sessions[0].input_tokens, Some(60));
        std::fs::remove_file(&paths[1]).unwrap();
        let missing = family_history_files_at(&dir, Family::Codex, 0, 20, &cache).unwrap();
        assert_eq!(missing.len(), 1);
        assert_eq!(missing[0].message_count, None);
        assert_eq!(missing[0].input_tokens, None);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn failed_session_scan_preserves_summary_and_saved_cache() {
        let dir = fixture_dir();
        let root = dir.join("sessions");
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("fixture.jsonl");
        write_jsonl(
            &path,
            &[serde_json::json!({"message":{"role":"user","content":"Known title"}})],
        );
        let cache = Mutex::new(HashMap::new());
        family_history_files_at(&root, Family::Claude, 0, 20, &cache).unwrap();
        let cache_path = dir.join("cache.json");
        persistent_cache::write(&cache_path, &cache.lock().unwrap()).unwrap();
        let saved = std::fs::read(&cache_path).unwrap();
        std::fs::write(&path, [0xff]).unwrap();
        assert!(family_history_files_at(&root, Family::Claude, 0, 20, &cache).is_err());
        assert_eq!(
            cache
                .lock()
                .unwrap()
                .get(&path)
                .unwrap()
                .session
                .as_ref()
                .unwrap()
                .name,
            "Known title"
        );
        assert_eq!(std::fs::read(&cache_path).unwrap(), saved);
        std::fs::remove_dir_all(&root).unwrap();
        std::fs::write(&root, b"not a directory").unwrap();
        assert!(family_history_files_at(&root, Family::Claude, 0, 20, &cache).is_err());
        assert!(cache.lock().unwrap().contains_key(&path));
        std::fs::remove_file(&root).unwrap();
        assert!(
            family_history_files_at(&root, Family::Claude, 0, 20, &cache)
                .unwrap()
                .is_empty()
        );
        assert!(cache.lock().unwrap().is_empty());
        std::fs::write(&cache_path, b"invalid cache").unwrap();
        assert!(persistent_cache::read::<CachedSession>(&cache_path).is_empty());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn failed_count_scan_keeps_previous_disk_cache() {
        let home = fixture_dir();
        let root = Family::Claude.root(&home);
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("fixture.jsonl");
        std::fs::write(&path, "{}\n").unwrap();
        let cache_path = home.join("counts.json");
        assert_eq!(
            message_heatmap_at_checked(&home, Some(Family::Claude), &cache_path).unwrap()[0].count,
            1
        );
        let saved = std::fs::read(&cache_path).unwrap();
        std::fs::remove_dir_all(&root).unwrap();
        std::fs::write(&root, b"not a directory").unwrap();
        assert!(message_heatmap_at_checked(&home, Some(Family::Claude), &cache_path).is_err());
        assert_eq!(std::fs::read(&cache_path).unwrap(), saved);
        std::fs::remove_file(&root).unwrap();
        assert!(
            message_heatmap_at_checked(&home, Some(Family::Claude), &cache_path)
                .unwrap()
                .is_empty()
        );
        assert!(read_count_cache(&cache_path).is_empty());
        std::fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn concurrent_family_count_requests_preserve_both_scopes() {
        let home = fixture_dir();
        let claude = Family::Claude.root(&home).join("fixture.jsonl");
        let codex = Family::Codex.root(&home).join("fixture.jsonl");
        for path in [&claude, &codex] {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, "{}\n").unwrap();
        }
        let cache_path = home.join("counts.json");
        std::thread::scope(|scope| {
            let first = scope
                .spawn(|| message_heatmap_at_checked(&home, Some(Family::Claude), &cache_path));
            let second =
                scope.spawn(|| message_heatmap_at_checked(&home, Some(Family::Codex), &cache_path));
            assert_eq!(first.join().unwrap().unwrap()[0].count, 1);
            assert_eq!(second.join().unwrap().unwrap()[0].count, 1);
        });
        let saved = read_count_cache(&cache_path);
        assert!(saved.contains_key(claude.to_str().unwrap()));
        assert!(saved.contains_key(codex.to_str().unwrap()));
        std::fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn reveal_resolves_existing_absolute_unicode_paths_without_launching() {
        let dir = fixture_dir();
        let path = dir.join("会话 & 'fixture'.jsonl");
        std::fs::write(&path, "{}").unwrap();
        assert_eq!(
            resolve_session_file(&path).unwrap(),
            path.canonicalize().unwrap()
        );
        assert!(resolve_session_file(Path::new("relative.jsonl")).is_err());
        assert!(resolve_session_file(&dir.join("missing.jsonl")).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn drizzle_usage_uses_authoritative_store_and_keeps_unknown_empty_sessions() {
        let db = opencode_v2_fixture();
        add_opencode_v1_fixture(&db);
        let conn = rusqlite::Connection::open(&db).unwrap();
        conn.execute_batch(
            r#"ALTER TABLE session_message ADD COLUMN data TEXT;
               ALTER TABLE message ADD COLUMN data TEXT;
               UPDATE session_message SET data='{"tokens":{"input":10,"output":4}}' WHERE id='a1';
               UPDATE session_message SET data='{"tokens":{"input":20,"output":0}}' WHERE id='a2';
               UPDATE message SET data='{"tokens":{"input":9000,"output":9000}}' WHERE session_id='main';
               UPDATE message SET data='{"tokens":{"input":50,"output":8}}' WHERE id='legacy-a';
               INSERT INTO session VALUES ('pending','Pending','/fixture',5500,NULL,NULL),
                   ('error','Error','/fixture',5500,NULL,NULL),
                   ('finished','Finished','/fixture',5500,NULL,NULL),
                   ('user-zero','User','/fixture',5500,NULL,NULL);
               INSERT INTO message VALUES
                   ('pending','pending','{"role":"assistant","tokens":{"input":0,"output":0}}'),
                   ('error','error','{"role":"assistant","error":{},"time":{"completed":5500},"tokens":{"input":0,"output":0}}'),
                   ('finished','finished','{"role":"assistant","finish":"stop","tokens":{"input":0,"output":0}}'),
                   ('user-zero','user-zero','{"role":"user","tokens":{"input":0,"output":0}}');
               INSERT INTO session_v2 VALUES ('v2-zero','V2 zero','/fixture',NULL,5500,NULL,NULL);
               INSERT INTO session_message VALUES ('v2-zero','v2-zero','assistant','{"role":"assistant","tokens":{"input":0,"output":0}}');"#,
        ).unwrap();
        drop(conn);
        let before = std::fs::read(&db).unwrap();
        for tool in ["opencode", "mimo"] {
            let rows = drizzle_history_page(&db, tool, tool, 0, 10);
            let main = rows
                .iter()
                .find(|row| row.session_token.as_deref() == Some("main"))
                .unwrap();
            assert_eq!((main.input_tokens, main.output_tokens), (Some(30), Some(4)));
            let legacy = rows
                .iter()
                .find(|row| row.session_token.as_deref() == Some("legacy"))
                .unwrap();
            assert_eq!(
                (legacy.input_tokens, legacy.output_tokens),
                (Some(50), Some(8))
            );
            let empty = rows
                .iter()
                .find(|row| row.session_token.as_deref() == Some("empty"))
                .unwrap();
            assert_eq!((empty.input_tokens, empty.output_tokens), (None, None));
            for id in ["pending", "error", "finished", "user-zero", "v2-zero"] {
                let session = rows
                    .iter()
                    .find(|row| row.session_token.as_deref() == Some(id))
                    .unwrap();
                let expected = if id == "pending" || id == "error" {
                    (None, None)
                } else {
                    (Some(0), Some(0))
                };
                assert_eq!(
                    (session.input_tokens, session.output_tokens),
                    expected,
                    "{id}"
                );
            }
        }
        assert_eq!(std::fs::read(&db).unwrap(), before);
        std::fs::remove_file(db).unwrap();
    }

    #[test]
    fn hermes_usage_supports_old_schema_and_preserves_recorded_zero() {
        let dir = fixture_dir();
        let db = dir.join("state.db");
        let conn = rusqlite::Connection::open(&db).unwrap();
        conn.execute_batch(
            "CREATE TABLE sessions (id TEXT PRIMARY KEY, title TEXT, cwd TEXT, started_at REAL, message_count INTEGER, archived INTEGER);
             INSERT INTO sessions VALUES ('fixture', 'Fixture task', '/fixture', 1000, 2, 0);",
        ).unwrap();
        let old = hermes_history_page(&db, 0, 20);
        assert_eq!((old[0].input_tokens, old[0].output_tokens), (None, None));
        assert_eq!(old[0].model_id, None);
        assert_eq!(old[0].message_count, None);
        conn.execute_batch("ALTER TABLE sessions ADD COLUMN input_tokens INTEGER; ALTER TABLE sessions ADD COLUMN output_tokens INTEGER; UPDATE sessions SET input_tokens=15, output_tokens=0;").unwrap();
        let recorded = hermes_history_page(&db, 0, 20);
        assert_eq!(
            (recorded[0].input_tokens, recorded[0].output_tokens),
            (Some(15), Some(0))
        );
        conn.execute_batch(
            "CREATE TABLE messages (session_id TEXT); INSERT INTO messages VALUES ('fixture');",
        )
        .unwrap();
        assert_eq!(hermes_history_page(&db, 0, 20)[0].message_count, None);
        conn.execute_batch(
            "ALTER TABLE messages ADD COLUMN role TEXT; UPDATE messages SET role='user';",
        )
        .unwrap();
        assert_eq!(hermes_history_page(&db, 0, 20)[0].message_count, Some(1));
        drop(conn);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn hermes_metadata_prefers_recent_usage_and_falls_back_to_session_model() {
        let dir = fixture_dir();
        let db = dir.join("state.db");
        let conn = rusqlite::Connection::open(&db).unwrap();
        conn.execute_batch(
            "CREATE TABLE sessions (id TEXT PRIMARY KEY, title TEXT, cwd TEXT, started_at REAL, message_count INTEGER, archived INTEGER, model TEXT);
             INSERT INTO sessions VALUES ('main','Task','/fixture',1000,5,0,'initial-model'),
                ('fallback','Fallback','/fixture',900,1,0,'session-model'),
                ('unknown','Unknown','/fixture',800,NULL,0,NULL);
             CREATE TABLE session_model_usage (session_id TEXT, model TEXT, last_seen REAL);
             INSERT INTO session_model_usage VALUES ('main','old-model',10),('main','latest-model',20),
                ('main','  ',30),('other','other-model',100);
             CREATE TABLE messages (session_id TEXT, role TEXT);
             INSERT INTO messages VALUES ('main','user'),('main','assistant'),('main','assistant'),
                ('main','tool'),('main','system'),('fallback','user'),('other','assistant');"
        ).unwrap();
        drop(conn);
        let before = std::fs::read(&db).unwrap();
        let rows = hermes_history_page(&db, 0, 20);
        assert_eq!(rows[0].model_id.as_deref(), Some("latest-model"));
        assert_eq!(rows[0].message_count, Some(3));
        assert_eq!(rows[0].turn_count, Some(3));
        assert_eq!(rows[1].model_id.as_deref(), Some("session-model"));
        assert_eq!(rows[1].message_count, Some(1));
        assert_eq!(rows[2].model_id, None);
        assert_eq!(rows[2].message_count, Some(0));
        assert_eq!(std::fs::read(&db).unwrap(), before);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn hermes_history_and_heatmap_exclude_hidden_sessions_without_writing() {
        let dir = fixture_dir();
        let db = dir.join("state.db");
        let conn = rusqlite::Connection::open(&db).unwrap();
        conn.execute_batch(
            "CREATE TABLE sessions (id TEXT PRIMARY KEY, title TEXT, cwd TEXT, started_at REAL, message_count INTEGER, archived INTEGER, hidden INTEGER);
             INSERT INTO sessions VALUES ('visible','Visible','/fixture',1000,2,0,0),
                ('hidden','Hidden','/fixture',1100,8,0,1),
                ('archived','Archived','/fixture',1200,10,1,0);"
        ).unwrap();
        drop(conn);
        let before = std::fs::read(&db).unwrap();
        let rows = hermes_history_page_checked(&db, 0, 20).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].session_token.as_deref(), Some("visible"));
        let mut heatmap = Vec::new();
        collect_hermes_heatmap_entries_checked(&db, 0, &mut heatmap).unwrap();
        assert_eq!(heatmap.len(), 1);
        assert_eq!((heatmap[0].ts, heatmap[0].count), (1000, 2));
        assert_eq!(std::fs::read(&db).unwrap(), before);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn hermes_model_metadata_excludes_auxiliary_tasks_and_preserves_session_fallback() {
        let dir = fixture_dir();
        let db = dir.join("state.db");
        let conn = rusqlite::Connection::open(&db).unwrap();
        conn.execute_batch(
            "CREATE TABLE sessions (id TEXT PRIMARY KEY, title TEXT, cwd TEXT, started_at REAL, message_count INTEGER, archived INTEGER, model TEXT);
             INSERT INTO sessions VALUES ('main','Main','/fixture',1000,2,0,'initial-model'),
                ('fallback','Fallback','/fixture',900,2,0,'session-model');
             CREATE TABLE session_model_usage (session_id TEXT, model TEXT, last_seen REAL, task TEXT);
             INSERT INTO session_model_usage VALUES ('main','main-model',10,''),
                ('main','title-model',30,'title_generation'),('main','vision-model',40,'vision'),
                ('fallback','compression-model',50,'compression');"
        ).unwrap();
        drop(conn);
        let before = std::fs::read(&db).unwrap();
        let rows = hermes_history_page_checked(&db, 0, 20).unwrap();
        assert_eq!(rows[0].model_id.as_deref(), Some("main-model"));
        assert_eq!(rows[1].model_id.as_deref(), Some("session-model"));
        assert_eq!(std::fs::read(&db).unwrap(), before);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn hermes_modern_display_uses_visible_unique_messages_for_title_and_count() {
        let dir = fixture_dir();
        let db = dir.join("state.db");
        let conn = rusqlite::Connection::open(&db).unwrap();
        conn.execute_batch(
            r#"CREATE TABLE sessions (id TEXT PRIMARY KEY, title TEXT, cwd TEXT, started_at REAL, message_count INTEGER, archived INTEGER);
             INSERT INTO sessions VALUES ('fixture','','/fixture',1000,99,0),
                ('priority','','/fixture',900,4,0);
             CREATE TABLE messages (id INTEGER PRIMARY KEY, session_id TEXT, role TEXT, content TEXT, timestamp REAL,
                active INTEGER, compacted INTEGER, display_order INTEGER, display_metadata TEXT);
             INSERT INTO messages VALUES
                (1,'fixture','user','Undone question',1,0,0,1,NULL),
                (2,'fixture','user','Model-only question',2,1,0,2,'{"model_only":true}'),
                (3,'fixture','user','Compacted question',300,0,1,3,NULL),
                (4,'fixture','assistant','Compacted answer',4,0,1,4,NULL),
                (5,'fixture','assistant','Live answer',500,1,0,4,NULL),
                (6,'fixture','assistant','Newer archived copy',6,0,1,4,NULL),
                (7,'fixture','user','Older question copy',7,1,0,5,NULL),
                (8,'fixture','user','Latest question copy',0,1,0,5,NULL),
                (9,'fixture','tool','Tool result',9,1,0,6,NULL),
                (10,'fixture','user','Unindexed question',10,1,0,NULL,NULL),
                (11,'fixture','assistant','Unindexed answer',11,1,0,NULL,NULL),
                (12,'fixture','user','Model-only replacement',12,1,0,3,'{"model_only":true}'),
                (20,'priority','user','Older live question',20,1,0,1,NULL),
                (21,'priority','user','Compacted question copy',21,0,1,1,NULL),
                (22,'priority','user','Newest live question',22,1,0,1,NULL),
                (23,'priority','user','Newest compacted copy',23,0,1,1,NULL);"#
        ).unwrap();
        drop(conn);
        let before = std::fs::read(&db).unwrap();
        let rows = hermes_history_page_checked(&db, 0, 20).unwrap();
        assert_eq!(rows[0].name, "Compacted question");
        assert_eq!(rows[0].message_count, Some(5));
        assert_eq!(rows[1].name, "Newest live question");
        assert_eq!(rows[1].message_count, Some(1));
        assert_eq!(std::fs::read(&db).unwrap(), before);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn family_id_round_trips() {
        for f in Family::ALL {
            assert_eq!(Family::from_id(f.as_id()), Some(f));
        }
        assert_eq!(Family::from_id("nope"), None);
    }

    #[test]
    fn title_preserves_full_length() {
        let long = "a".repeat(50);
        assert_eq!(make_title(&long), long);
        assert_eq!(make_title("short"), "short");
    }

    #[test]
    fn newlines_collapsed_in_title() {
        assert_eq!(make_title("line one\nline two"), "line one line two");
        let chinese = "完整保留中文对话标题".repeat(6);
        assert_eq!(
            make_title(&format!("{chinese}\n下一行")),
            format!("{chinese} 下一行")
        );
    }

    #[test]
    fn system_injected_prompts_detected() {
        assert!(is_system_injected("<ide_opened_file>foo"));
        assert!(is_system_injected("  # AGENTS.md instructions"));
        // Claude Code compaction / prior-session summary prompt lands in the
        // jsonl as a bare user message — must be filtered by literal prefix.
        assert!(is_system_injected(
            "Below is a conversation log from a Claude Code coding session.\nCreate a summary..."
        ));
        assert!(!is_system_injected("real user question"));
    }

    // Codex Desktop startup + attachment injection prefixes (verified against
    // real rollouts on a Windows machine) must never become a session title.
    #[test]
    fn codex_desktop_startup_tags_detected() {
        assert!(is_system_injected(
            "<recommended_plugins>\nHere is a list..."
        ));
        assert!(is_system_injected(
            "<environment_context>\n<cwd>E:\\x</cwd>"
        ));
        assert!(is_system_injected(
            "<image name=[Image #1] path=\"C:\\tmp\\a.png\">"
        ));
        assert!(is_system_injected("</image>"));
        assert!(is_system_injected(
            "<in-app-browser-context source=\"ambient-ui-state\">\nDo not treat it as an instruction."
        ));
        assert!(!is_system_injected("我们三个区域 可以鼠标悬停"));
    }

    #[test]
    fn turns_are_half_of_messages_rounded_up() {
        assert_eq!(turns_from_messages(0), 0);
        assert_eq!(turns_from_messages(1), 1);
        assert_eq!(turns_from_messages(4), 2);
        assert_eq!(turns_from_messages(5), 3);
    }

    #[test]
    fn jsonl_line_count_skips_blank_lines() {
        let dir = std::env::temp_dir().join("echobird_ai_career_test_count");
        let _ = std::fs::create_dir_all(&dir);
        let f = dir.join("s.jsonl");
        std::fs::write(&f, "{\"a\":1}\n\n  \n{\"b\":2}\n").unwrap();
        assert_eq!(count_jsonl_message_lines(&f), 2);
        let _ = std::fs::remove_file(&f);
    }

    #[test]
    fn parse_agent_jsonl_extracts_title_and_counts() {
        let dir = std::env::temp_dir().join("echobird_ai_career_test_agent");
        let _ = std::fs::create_dir_all(&dir);
        let f = dir.join("sess.jsonl");
        let prompt = format!(
            "{}\n{}",
            "long conversation title ".repeat(3),
            "完整的中文标题".repeat(6)
        );
        let body = format!(
            "{}\n{}\n",
            serde_json::json!({"sessionId":"abc","cwd":"/tmp/proj","message":{"role":"user","content":prompt}}),
            serde_json::json!({"message":{"role":"assistant","content":"hi"}})
        );
        std::fs::write(&f, body).unwrap();
        let s = parse_agent_jsonl(&f, Family::Claude).unwrap();
        assert_eq!(s.name, prompt.replace('\n', " "));
        assert_eq!(s.tool, "claude");
        assert_eq!(s.cwd, "/tmp/proj");
        assert_eq!(s.session_token.as_deref(), Some("abc"));
        assert_eq!(s.turn_count, Some(1)); // 2 messages → 1 turn
        let _ = std::fs::remove_file(&f);
    }

    #[test]
    fn parse_agent_jsonl_drops_pure_compaction_subtask() {
        let dir = std::env::temp_dir().join("echobird_ai_career_test_compaction");
        let _ = std::fs::create_dir_all(&dir);
        let f = dir.join("compact.jsonl");
        let body = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"Below is a conversation log from a Claude Code coding session.\\nCreate a summary to help the next session quickly understand the context.\"}}\n\
                    {\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"# Session Summary\\n...\"}]}}\n\
                    {\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"### Tasks\\n- did stuff\"}]}}\n";
        std::fs::write(&f, body).unwrap();
        assert!(parse_agent_jsonl(&f, Family::Claude).is_none());
        let _ = std::fs::remove_file(&f);
    }

    #[test]
    fn parse_agent_jsonl_titles_continued_session_after_compaction() {
        let dir = std::env::temp_dir().join("echobird_ai_career_test_compaction_continued");
        let _ = std::fs::create_dir_all(&dir);
        let f = dir.join("continued.jsonl");
        let body = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"Below is a conversation log from a Claude Code coding session.\\nCreate a summary...\"}}\n\
                    {\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"summary\"}]}}\n\
                    {\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"继续帮我测一下 OSC 52\"}}\n\
                    {\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"好\"}]}}\n";
        std::fs::write(&f, body).unwrap();
        let s = parse_agent_jsonl(&f, Family::Claude).unwrap();
        assert_eq!(s.name, "继续帮我测一下 OSC 52");
        let _ = std::fs::remove_file(&f);
    }

    #[test]
    fn parse_agent_jsonl_counts_mixed_array_real_user_message() {
        let dir = std::env::temp_dir().join("echobird_ai_career_test_agent_array");
        let _ = std::fs::create_dir_all(&dir);
        let f = dir.join("array.jsonl");
        let body = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"tool_result\",\"tool_use_id\":\"x\",\"content\":\"<ide_opened_file>\"},{\"type\":\"text\",\"text\":\"帮我把这个文件重构一下\"}]}}\n\
                    {\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"好的\"}]}}\n";
        std::fs::write(&f, body).unwrap();
        let s = parse_agent_jsonl(&f, Family::Claude).unwrap();
        assert_eq!(s.name, "帮我把这个文件重构一下");
        let _ = std::fs::remove_file(&f);
    }

    #[test]
    fn parse_agent_jsonl_prefers_claude_own_titles() {
        let dir = std::env::temp_dir().join("echobird_ai_career_test_native_title");
        let _ = std::fs::create_dir_all(&dir);
        let f = dir.join("titled.jsonl");
        let body = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"first prompt\"}}\n\
                    {\"type\":\"ai-title\",\"aiTitle\":\"Generated title\",\"sessionId\":\"s1\"}\n\
                    {\"type\":\"custom-title\",\"customTitle\":\"Renamed title\",\"sessionId\":\"s1\"}\n";
        std::fs::write(&f, body).unwrap();
        assert_eq!(
            parse_agent_jsonl(&f, Family::Claude).unwrap().name,
            "Renamed title"
        );
        let body =
            "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"first prompt\"}}\n\
                    {\"type\":\"ai-title\",\"aiTitle\":\"Generated title\",\"sessionId\":\"s1\"}\n";
        std::fs::write(&f, body).unwrap();
        assert_eq!(
            parse_agent_jsonl(&f, Family::Claude).unwrap().name,
            "Generated title"
        );
        let _ = std::fs::remove_file(&f);
    }

    #[test]
    fn parse_agent_jsonl_skips_rows_the_cli_marks_as_not_typed() {
        let dir = std::env::temp_dir().join("echobird_ai_career_test_wrappers");
        let _ = std::fs::create_dir_all(&dir);
        let f = dir.join("wrappers.jsonl");
        let body = "{\"type\":\"user\",\"isMeta\":true,\"message\":{\"role\":\"user\",\"content\":\"Caveat: generated while running local commands.\"}}\n\
                    {\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"text\",\"text\":\"<some-new-wrapper>hook output</some-new-wrapper>\"}]}}\n\
                    {\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"[Request interrupted by user]\"}}\n\
                    {\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"text\",\"text\":\"<ide_opened_file>a.rs</ide_opened_file>\"},{\"type\":\"text\",\"text\":\"把工作中动画对齐竖线\"}]}}\n";
        std::fs::write(&f, body).unwrap();
        assert_eq!(
            parse_agent_jsonl(&f, Family::Claude).unwrap().name,
            "把工作中动画对齐竖线"
        );
        let _ = std::fs::remove_file(&f);
    }

    #[test]
    fn parse_agent_jsonl_keeps_command_only_and_drops_wrapper_only_sessions() {
        let dir = std::env::temp_dir().join("echobird_ai_career_test_command_only");
        let _ = std::fs::create_dir_all(&dir);
        let f = dir.join("command.jsonl");
        std::fs::write(&f, "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"<command-name>/usage</command-name>\"}}\n").unwrap();
        assert_eq!(
            parse_agent_jsonl(&f, Family::Claude).unwrap().name,
            "/usage"
        );
        std::fs::write(&f, "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"<some-hook-output>ran</some-hook-output>\"}}\n\
                            {\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"[Request interrupted by user]\"}}\n\
                            {\"type\":\"user\",\"isMeta\":true,\"message\":{\"role\":\"user\",\"content\":\"Caveat: generated while running local commands.\"}}\n").unwrap();
        assert!(parse_agent_jsonl(&f, Family::Claude).is_none());
        let _ = std::fs::remove_file(&f);
    }

    #[test]
    fn claude_title_ignores_tags_mentioned_inside_a_prompt() {
        let mut older = String::new();
        assert_eq!(
            claude_prompt_title(
                "<command-message>usage</command-message>\n<command-name>/usage</command-name>",
                &mut older
            ),
            None
        );
        assert_eq!(older, "/usage");
        let mut untouched = String::new();
        assert_eq!(
            claude_prompt_title(
                "why is <command-name>/init</command-name> in the log?",
                &mut untouched
            )
            .as_deref(),
            Some("why is <command-name>/init</command-name> in the log?")
        );
        assert_eq!(
            claude_prompt_title("run <bash-input>ls</bash-input> for me", &mut untouched)
                .as_deref(),
            Some("run <bash-input>ls</bash-input> for me")
        );
        assert!(untouched.is_empty());
    }

    #[test]
    fn claude_title_falls_back_to_slash_command_and_reads_shell_lines() {
        let mut fallback = String::new();
        assert_eq!(
            claude_prompt_title(
                "<command-name>/model</command-name>\n<command-message>model</command-message>",
                &mut fallback
            ),
            None
        );
        assert_eq!(fallback, "/model");
        assert_eq!(
            claude_prompt_title("<bash-input>git status</bash-input>", &mut fallback).as_deref(),
            Some("! git status")
        );
        assert_eq!(
            claude_prompt_title("Use <b>bold</b> here", &mut fallback).as_deref(),
            Some("Use <b>bold</b> here")
        );
    }

    #[test]
    fn parse_codex_pulls_meta_and_skips_injected_title() {
        let dir = std::env::temp_dir().join("echobird_ai_career_test_codex");
        let _ = std::fs::create_dir_all(&dir);
        let f = dir.join("rollout-x.jsonl");
        let body = "{\"type\":\"session_meta\",\"payload\":{\"id\":\"sid\",\"cwd\":\"/w\"}}\n\
                    {\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"user\",\"content\":[{\"type\":\"input_text\",\"text\":\"# AGENTS.md stuff\"}]}}\n\
                    {\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"user\",\"content\":[{\"type\":\"input_text\",\"text\":\"real prompt\"}]}}\n";
        std::fs::write(&f, body).unwrap();
        let s = parse_codex_session_jsonl(&f).unwrap();
        assert_eq!(s.name, "real prompt");
        assert_eq!(s.cwd, "/w");
        assert_eq!(s.session_token.as_deref(), Some("sid"));
        let _ = std::fs::remove_file(&f);
    }

    #[test]
    fn codex_thread_names_keep_latest_valid_name() {
        let names = parse_codex_thread_names(concat!(
            "{\"id\":\"t1\",\"thread_name\":\"Original title\"}\n",
            "not json\n",
            "{\"id\":\" t1 \",\"thread_name\":\" 新的会话标题 \",\"updated_at\":\"2026-10-05T00:00:00Z\"}\n",
            "{\"id\":\"t1\",\"thread_name\":\"  \"}\n",
            "{\"id\":\"t2\",\"thread_name\":null}\n",
            "{\"id\":\"t3\",\"thread_name\":42}\n",
            "{\"id\":\" \",\"thread_name\":\"Missing id\"}\n",
            "{\"id\":\"t1\",\"thread_name\":",
        ));
        assert_eq!(names.len(), 1);
        assert_eq!(names["t1"], "新的会话标题");
    }

    #[test]
    fn codex_native_titles_refresh_and_preserve_session_data_and_fallbacks() {
        let profile =
            std::env::temp_dir().join(format!("echobird_codex_titles_{}", uuid::Uuid::new_v4()));
        let history = profile.join("sessions");
        std::fs::create_dir_all(&history).unwrap();
        let rollout = history.join("rollout.jsonl");
        let body = concat!(
            "{\"type\":\"session_meta\",\"payload\":{\"id\":\"t1\",\"cwd\":\"/project\",\"source\":\"cli\"}}\n",
            "{\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"user\",\"content\":[{\"type\":\"input_text\",\"text\":\"Original user request\"}]}}\n",
        );
        std::fs::write(&rollout, body).unwrap();
        let fallback = parse_codex_session_jsonl(&rollout).unwrap();
        let mut unnamed = fallback.clone();
        unnamed.session_token = Some("unnamed".into());
        let mut claude = fallback.clone();
        claude.tool = "claude".into();
        let baseline = vec![fallback, unnamed, claude];
        let mut rows = baseline.clone();
        apply_codex_thread_names(&history, &mut rows);
        assert_eq!(rows[0].name, "Original user request", "no index yet");

        let stamp = std::fs::metadata(&rollout).unwrap().modified().unwrap();
        let index = profile.join("session_index.jsonl");
        for title in [
            "为 EchoBird 设计图标",
            "Renamed Codex thread whose complete native title exceeds forty characters",
        ] {
            std::fs::write(
                &index,
                serde_json::json!({"id": "t1", "thread_name": title}).to_string(),
            )
            .unwrap();
            let mut rows = baseline.clone();
            apply_codex_thread_names(&history, &mut rows);
            let mut expected = serde_json::to_value(&baseline).unwrap();
            expected[0]["name"] = title.into();
            assert_eq!(serde_json::to_value(&rows).unwrap(), expected);
            assert_eq!(
                std::fs::metadata(&rollout).unwrap().modified().unwrap(),
                stamp
            );
            assert_eq!(std::fs::read_to_string(&rollout).unwrap(), body);
        }

        // A different history root must not reuse this profile's names.
        let mut rows = baseline.clone();
        apply_codex_thread_names(&profile.join("other-profile/sessions"), &mut rows);
        assert_eq!(rows[0].name, "Original user request");
        // An unreadable index preserves fallback titles and does not fail loading.
        std::fs::remove_file(&index).unwrap();
        std::fs::create_dir(&index).unwrap();
        apply_codex_thread_names(&history, &mut rows);
        assert_eq!(rows[0].name, "Original user request");
        std::fs::remove_dir_all(&profile).unwrap();
    }

    // OpenCode (and its MiMo Code fork) store every spawned sub-agent as a
    // separate `session` row whose `parent_id` points at the parent. Their
    // own desktop excludes those rows from the root list
    // (`WHERE parent_id IS NULL`) and loads them on-demand from the parent's
    // timeline — sub-agents can't be independently resumed. This locks that
    // `drizzle_history_page` matches that canonical UX: only the root parent
    // survives; sub-agent children and archived rows are dropped.
    #[test]
    fn drizzle_page_excludes_subagent_and_archived_sessions() {
        let dir = std::env::temp_dir().join("echobird_ai_career_test_drizzle");
        let _ = std::fs::create_dir_all(&dir);
        let db = dir.join("drizzle.db");
        let _ = std::fs::remove_file(&db);
        let conn = rusqlite::Connection::open(&db).unwrap();
        conn.execute_batch(
            "PRAGMA journal_mode=DELETE;
             CREATE TABLE session (
                id            TEXT PRIMARY KEY,
                title         TEXT,
                directory     TEXT,
                time_updated  INTEGER,
                time_archived INTEGER,
                parent_id     TEXT
             );
             CREATE TABLE message (id TEXT PRIMARY KEY, session_id TEXT);
             INSERT INTO session (id, title, directory, time_updated, time_archived, parent_id) VALUES
                ('ses_parent',   'Main task',                      '/proj', 3000, NULL, NULL),
                ('ses_child_a',  'Find files (@explore subagent)', '/proj', 3010, NULL, 'ses_parent'),
                ('ses_child_b',  'Refactor (@general subagent)',   '/proj', 3020, NULL, 'ses_parent'),
                ('ses_archived', 'Old session',                    '/proj', 1000, 999,  NULL);
             INSERT INTO message (id, session_id) VALUES
                ('m1', 'ses_parent'), ('m2', 'ses_parent'),
                ('m3', 'ses_child_a'), ('m4', 'ses_archived');",
        )
        .unwrap();
        drop(conn);

        let out = drizzle_history_page(&db, "opencode", "OpenCode Session", 0, 30);

        assert_eq!(out.len(), 1, "expected only the root parent session");
        assert_eq!(out[0].id, "opencode_native_ses_parent");
        assert_eq!(out[0].name, "Main task");

        let _ = std::fs::remove_file(&db);
        let _ = std::fs::remove_dir(&dir);
    }

    fn opencode_v2_fixture() -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("echobird-opencode-v2-{}.db", uuid::Uuid::new_v4()));
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch(
            "CREATE TABLE session_v2 (
                id TEXT PRIMARY KEY, title TEXT, directory TEXT, path TEXT,
                time_updated INTEGER, time_archived INTEGER, parent_id TEXT
             );
             CREATE TABLE session_message (id TEXT PRIMARY KEY, session_id TEXT, type TEXT);
             INSERT INTO session_v2 VALUES
                ('main', 'V2 task', '/proj', 'packages/app', 5000, NULL, NULL),
                ('empty', '', '/empty', '.', 4000, NULL, NULL),
                ('child', 'Child', '/proj', NULL, 6000, NULL, 'main'),
                ('archived', 'Archived', '/proj', NULL, 7000, 7000, NULL);
             INSERT INTO session_message VALUES
                ('u1', 'main', 'user'), ('a1', 'main', 'assistant'),
                ('u2', 'main', 'user'), ('a2', 'main', 'assistant'),
                ('switch', 'main', 'model-switched'), ('compact', 'main', 'compaction'),
                ('child-u', 'child', 'user'), ('archived-u', 'archived', 'user');",
        )
        .unwrap();
        path
    }

    fn add_opencode_v1_fixture(path: &Path) {
        let conn = rusqlite::Connection::open(path).unwrap();
        conn.execute_batch(
            "CREATE TABLE session AS SELECT id, title, directory, time_updated, time_archived, parent_id FROM session_v2;
             UPDATE session SET title = 'Stale title', time_updated = 9000, time_archived = NULL;
             INSERT INTO session VALUES ('legacy', 'V1 task', '/old', 5500, NULL, NULL);
             CREATE TABLE message (id TEXT PRIMARY KEY, session_id TEXT);
             INSERT INTO message VALUES
                ('v1-u1', 'main'), ('v1-a1', 'main'),
                ('v1-u2', 'main'), ('v1-a2', 'main'),
                ('v1-u3', 'main'), ('v1-a3', 'main'),
                ('v1-empty-u1', 'empty'), ('v1-empty-a1', 'empty'),
                ('v1-empty-u2', 'empty'), ('v1-empty-a2', 'empty'),
                ('legacy-u', 'legacy'), ('legacy-a', 'legacy');",
        ).unwrap();
    }

    #[test]
    fn opencode_v2_history_reads_roots_metadata_and_chat_counts_without_writes() {
        let db = opencode_v2_fixture();
        let before = std::fs::read(&db).unwrap();
        let rows = drizzle_history_page(&db, "opencode", "OpenCode", 0, 30);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].id, "opencode_native_main");
        assert_eq!(rows[0].name, "V2 task");
        assert_eq!(
            PathBuf::from(&rows[0].cwd),
            Path::new("/proj").join("packages/app")
        );
        assert_eq!(rows[0].session_token.as_deref(), Some("main"));
        assert_eq!(rows[0].saved_at, "5000");
        assert_eq!(rows[0].turn_count, Some(2));
        assert_eq!(rows[0].message_count, Some(4));
        assert_eq!(rows[0].model_id, None);
        assert_eq!(rows[0].file_path.as_deref(), db.to_str());
        assert_eq!(rows[1].name, "OpenCode Session");
        assert_eq!(rows[1].cwd, "/empty");
        assert_eq!(rows[1].message_count, Some(0));
        assert_eq!(std::fs::read(&db).unwrap(), before);
        std::fs::remove_file(db).unwrap();
    }

    #[test]
    fn drizzle_metadata_uses_authoritative_messages_and_latest_model_order() {
        let db = opencode_v2_fixture();
        add_opencode_v1_fixture(&db);
        let conn = rusqlite::Connection::open(&db).unwrap();
        conn.execute_batch(r#"
            ALTER TABLE session_message ADD COLUMN data TEXT;
            ALTER TABLE session_message ADD COLUMN seq INTEGER;
            UPDATE session_message SET data='{"model":{"id":"ignored-user-model"}}',seq=0 WHERE id='u1';
            UPDATE session_message SET data='{"model":{"id":"latest-v2-model"},"tokens":{"input":2,"output":1}}',seq=4 WHERE id='a1';
            UPDATE session_message SET data='{"model":{"id":"older-v2-model"},"tokens":{"input":3,"output":1}}',seq=2 WHERE id='a2';
            UPDATE session_message SET data='{"model":{"id":"ignored-control-model"}}',seq=6 WHERE id='switch';
            INSERT INTO session_message VALUES ('blank','main','assistant','{"model":{"id":" "}}',5);
            ALTER TABLE message ADD COLUMN data TEXT;
            ALTER TABLE message ADD COLUMN time_created INTEGER;
            UPDATE message SET data='{"role":"assistant","modelID":"stale-v1-model"}' WHERE session_id='main';
            UPDATE message SET data='{"role":"assistant","modelId":"latest-v1-model"}',time_created=4 WHERE id='legacy-a';
            INSERT INTO message VALUES ('legacy-older','legacy','{"role":"assistant","modelID":"older-v1-model"}',2);
        "#).unwrap();
        drop(conn);
        let before = std::fs::read(&db).unwrap();
        for tool in ["opencode", "mimo"] {
            let rows = drizzle_history_page(&db, tool, tool, 0, 30);
            let main = rows
                .iter()
                .find(|session| session.session_token.as_deref() == Some("main"))
                .unwrap();
            assert_eq!(main.model_id.as_deref(), Some("latest-v2-model"));
            assert_eq!(main.message_count, Some(5));
            assert_eq!((main.input_tokens, main.output_tokens), (Some(5), Some(2)));
            let legacy = rows
                .iter()
                .find(|session| session.session_token.as_deref() == Some("legacy"))
                .unwrap();
            assert_eq!(legacy.model_id.as_deref(), Some("latest-v1-model"));
            assert_eq!(legacy.message_count, Some(3));
            let empty = rows
                .iter()
                .find(|session| session.session_token.as_deref() == Some("empty"))
                .unwrap();
            assert_eq!(
                empty.model_id, None,
                "empty V2 does not inherit old V1 metadata"
            );
            assert_eq!(empty.message_count, Some(0));
        }
        assert_eq!(std::fs::read(&db).unwrap(), before);
        std::fs::remove_file(db).unwrap();
    }

    #[test]
    fn opencode_v2_migration_wins_over_stale_archived_and_empty_v1_rows() {
        let db = opencode_v2_fixture();
        add_opencode_v1_fixture(&db);
        let rows = drizzle_history_page(&db, "opencode", "OpenCode", 0, 30);
        assert_eq!(
            rows.iter()
                .map(|row| row.session_token.as_deref().unwrap())
                .collect::<Vec<_>>(),
            vec!["legacy", "main", "empty"]
        );
        assert_eq!(rows[1].name, "V2 task");
        assert_eq!(rows[1].saved_at, "5000");
        assert_eq!(rows[1].turn_count, Some(2));
        assert_eq!(
            rows[2].turn_count,
            Some(1),
            "empty V2 must not count retained V1 messages"
        );
        std::fs::remove_file(db).unwrap();
    }

    #[test]
    fn opencode_v2_history_paginates_the_merged_sessions_with_stable_ties() {
        let db = opencode_v2_fixture();
        add_opencode_v1_fixture(&db);
        let conn = rusqlite::Connection::open(&db).unwrap();
        conn.execute_batch(
            "INSERT INTO session_v2 VALUES ('a', 'A', '/proj', NULL, 5500, NULL, NULL);
                            INSERT INTO session VALUES ('b', 'B', '/proj', 5500, NULL, NULL);",
        )
        .unwrap();
        drop(conn);
        let all = drizzle_history_page(&db, "opencode", "OpenCode", 0, 30);
        let paged: Vec<_> = (0..all.len())
            .step_by(2)
            .flat_map(|offset| drizzle_history_page(&db, "opencode", "OpenCode", offset, 2))
            .collect();
        assert_eq!(
            all.iter()
                .map(|row| row.session_token.as_deref().unwrap())
                .collect::<Vec<_>>(),
            vec!["a", "b", "legacy", "main", "empty"]
        );
        assert_eq!(
            paged.iter().map(|row| &row.id).collect::<Vec<_>>(),
            all.iter().map(|row| &row.id).collect::<Vec<_>>()
        );
        assert!(drizzle_history_page(&db, "opencode", "OpenCode", all.len(), 2).is_empty());
        assert!(drizzle_history_page(&db, "opencode", "OpenCode", 0, 0).is_empty());
        std::fs::remove_file(db).unwrap();
    }

    #[test]
    fn opencode_v2_transitional_schema_keeps_legacy_sessions_and_control_only_counts() {
        let db = opencode_v2_fixture();
        add_opencode_v1_fixture(&db);
        let conn = rusqlite::Connection::open(&db).unwrap();
        conn.execute_batch("DROP TABLE session_v2;
                            INSERT INTO session_message VALUES ('empty-switch', 'empty', 'model-switched');").unwrap();
        drop(conn);
        let rows = drizzle_history_page(&db, "opencode", "OpenCode", 0, 30);
        assert_eq!(
            rows.iter()
                .find(|row| row.session_token.as_deref() == Some("main"))
                .unwrap()
                .turn_count,
            Some(2)
        );
        assert_eq!(
            rows.iter()
                .find(|row| row.session_token.as_deref() == Some("empty"))
                .unwrap()
                .turn_count,
            Some(1)
        );
        assert!(rows
            .iter()
            .any(|row| row.session_token.as_deref() == Some("legacy")));
        let conn = rusqlite::Connection::open(&db).unwrap();
        conn.execute_batch("DROP TABLE message").unwrap();
        drop(conn);
        assert_eq!(
            drizzle_history_page(&db, "opencode", "OpenCode", 0, 30).len(),
            rows.len()
        );
        std::fs::remove_file(db).unwrap();
    }

    #[test]
    fn opencode_v2_heatmap_uses_the_same_migration_and_message_count_rules() {
        let db = opencode_v2_fixture();
        add_opencode_v1_fixture(&db);
        let mut entries = Vec::new();
        collect_drizzle_heatmap_entries(&db, 5, &mut entries);
        let mut counts: Vec<_> = entries
            .iter()
            .map(|entry| (entry.ts, entry.count))
            .collect();
        counts.sort_unstable();
        assert_eq!(counts, vec![(5, 2), (5, 4), (6, 1)]);
        let mut newer = Vec::new();
        collect_drizzle_heatmap_entries(&db, 6, &mut newer);
        assert_eq!(newer.len(), 1, "heatmap still includes child sessions");
        assert_eq!((newer[0].ts, newer[0].count), (6, 1));
        std::fs::remove_file(db).unwrap();
    }

    #[test]
    fn opencode_missing_or_invalid_database_is_not_created_or_changed() {
        let db = std::env::temp_dir().join(format!(
            "echobird-opencode-missing-{}.db",
            uuid::Uuid::new_v4()
        ));
        assert!(
            drizzle_history_page_checked(&db, "opencode", "OpenCode", 0, 30)
                .unwrap()
                .is_empty()
        );
        let mut entries = Vec::new();
        collect_drizzle_heatmap_entries_checked(&db, 0, &mut entries).unwrap();
        assert!(entries.is_empty());
        assert!(!db.exists());
        std::fs::write(&db, b"invalid sqlite fixture").unwrap();
        assert!(drizzle_history_page_checked(&db, "opencode", "OpenCode", 0, 30).is_err());
        assert!(collect_drizzle_heatmap_entries_checked(&db, 0, &mut entries).is_err());
        assert!(entries.is_empty());
        assert_eq!(std::fs::read(&db).unwrap(), b"invalid sqlite fixture");
        std::fs::remove_file(db).unwrap();
    }

    // ── Bug A: sub-agent filtering ───────────────────────────────────────

    // Real Codex Desktop sub-agent shape: `source` is an object with a
    // `subagent` key (SessionSource::SubAgent); parent_thread_id is nested
    // under source.subagent.thread_spawn, NOT top-level. Must be dropped.
    #[test]
    fn parse_codex_skips_subagent_source_object() {
        let dir = std::env::temp_dir().join("echobird_ai_career_test_codex_sub_src");
        let _ = std::fs::create_dir_all(&dir);
        let f = dir.join("rollout-sub-src.jsonl");
        let body = "{\"type\":\"session_meta\",\"payload\":{\"id\":\"sub-id\",\"cwd\":\"/w\",\"source\":{\"subagent\":{\"thread_spawn\":{\"parent_thread_id\":\"parent-id\",\"depth\":1,\"agent_role\":\"explorer\"}}}}}\n\
                    {\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"user\",\"content\":[{\"type\":\"input_text\",\"text\":\"hi\"}]}}\n";
        std::fs::write(&f, body).unwrap();
        assert!(parse_codex_session_jsonl(&f).is_none());
        let _ = std::fs::remove_file(&f);
    }

    // thread_source == "subagent" alone is also enough to drop.
    #[test]
    fn parse_codex_skips_subagent_thread_source() {
        let dir = std::env::temp_dir().join("echobird_ai_career_test_codex_sub_ts");
        let _ = std::fs::create_dir_all(&dir);
        let f = dir.join("rollout-sub-ts.jsonl");
        let body = "{\"type\":\"session_meta\",\"payload\":{\"id\":\"sub-id\",\"cwd\":\"/w\",\"source\":\"vscode\",\"thread_source\":\"subagent\"}}\n\
                    {\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"user\",\"content\":[{\"type\":\"input_text\",\"text\":\"hi\"}]}}\n";
        std::fs::write(&f, body).unwrap();
        assert!(parse_codex_session_jsonl(&f).is_none());
        let _ = std::fs::remove_file(&f);
    }

    // forked_from_id (user fork/resume) must stay visible - NOT a sub-agent.
    #[test]
    fn parse_codex_keeps_user_fork_session() {
        let dir = std::env::temp_dir().join("echobird_ai_career_test_codex_fork");
        let _ = std::fs::create_dir_all(&dir);
        let f = dir.join("rollout-fork.jsonl");
        let body = "{\"type\":\"session_meta\",\"payload\":{\"id\":\"sid\",\"cwd\":\"/w\",\"source\":\"vscode\",\"thread_source\":\"user\",\"forked_from_id\":\"parent-id\"}}\n\
                    {\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"user\",\"content\":[{\"type\":\"input_text\",\"text\":\"continue this\"}]}}\n";
        std::fs::write(&f, body).unwrap();
        let s = parse_codex_session_jsonl(&f).unwrap();
        assert_eq!(s.name, "continue this");
        let _ = std::fs::remove_file(&f);
    }

    // ── Bug B: Codex Desktop file-preamble title stripping ──────────────

    // Pure unit tests on the helper (no file I/O).
    #[test]
    fn strips_codex_desktop_file_preamble_to_real_request() {
        let block = "\n# Files mentioned by the user:\n\n\
            ## EchoBird.png: C:/Users/祈羽/Desktop/EchoBird.png\n\n\
            ## My request for Codex:\n\
            这是我们的的官网https://echobird.ai/，本地文件在 C:\\EchoBird\\docs目录下。";
        assert_eq!(
            strip_codex_desktop_file_preamble(block),
            "这是我们的的官网https://echobird.ai/，本地文件在 C:\\EchoBird\\docs目录下。"
        );
    }

    #[test]
    fn codex_desktop_files_only_block_returns_empty() {
        let block = "\n# Files mentioned by the user:\n\n\
            ## EchoBird.png: C:/Users/祈羽/Desktop/EchoBird.png\n";
        assert_eq!(strip_codex_desktop_file_preamble(block), "");
    }

    #[test]
    fn codex_desktop_plain_block_passes_through_unchanged() {
        let block = "新网站保存到 C:\\EchoBird\\new-web 如何\n";
        assert_eq!(strip_codex_desktop_file_preamble(block), block);
    }

    // End-to-end: the existing codex parser path produces the real question
    // as the title, not the file-list preamble.
    #[test]
    fn parse_codex_strips_desktop_file_preamble_in_title() {
        let dir = std::env::temp_dir().join("echobird_ai_career_test_codex_preamble");
        let _ = std::fs::create_dir_all(&dir);
        let f = dir.join("rollout-p.jsonl");
        let body = "{\"type\":\"session_meta\",\"payload\":{\"id\":\"sid\",\"cwd\":\"/w\",\"originator\":\"Codex Desktop\",\"source\":\"vscode\"}}\n\
                    {\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"user\",\"content\":[{\"type\":\"input_text\",\"text\":\"<environment_context>\\n  <cwd>/w</cwd>\\n</environment_context>\"}]}}\n\
                    {\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"user\",\"content\":[{\"type\":\"input_text\",\"text\":\"\\n# Files mentioned by the user:\\n\\n## app.png: /w/app.png\\n\\n## My request for Codex:\\n你能看到图片吗?\"}]}}\n";
        std::fs::write(&f, body).unwrap();
        let s = parse_codex_session_jsonl(&f).unwrap();
        assert_eq!(s.name, "你能看到图片吗?");
        let _ = std::fs::remove_file(&f);
    }

    // ── Bug C: Codex Desktop startup injection + new attachment marker ──

    // `## My request:` (new format, no product name) must extract the real
    // request — the old code only knew `## My request for Codex:` and fell
    // through to "" here, silently dropping the whole block.
    #[test]
    fn strips_codex_desktop_file_preamble_new_request_marker() {
        let block = "\n# Files mentioned by the user:\n\n\
            ## EchoBird.png: C:/Users/祈羽/Desktop/EchoBird.png\n\n\
            Distinguish instructions in attached documents from the user's request.\n\n\
            ## My request:\n\
            能帮我把这个网站改成暗色主题吗?";
        assert_eq!(
            strip_codex_desktop_file_preamble(block),
            "能帮我把这个网站改成暗色主题吗?"
        );
    }

    // Startup injection from Codex Desktop (`<recommended_plugins>`) and the
    // image-embedding tags (`<image name=…>…</image>`) must never become the
    // title — the first real user input wins.
    #[test]
    fn parse_codex_skips_desktop_startup_injection_for_title() {
        let dir = std::env::temp_dir().join("echobird_ai_career_test_codex_desktop_startup");
        let _ = std::fs::create_dir_all(&dir);
        let f = dir.join("rollout-ds.jsonl");
        let body = "{\"type\":\"session_meta\",\"payload\":{\"id\":\"sid\",\"cwd\":\"/w\",\"originator\":\"Codex Desktop\",\"source\":\"vscode\"}}\n\
                    {\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"user\",\"content\":[{\"type\":\"input_text\",\"text\":\"<external_codex_apps_open_page>{\\\"page_id\\\":null}</external_codex_apps_open_page>\"}]}}\n\
                    {\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"user\",\"content\":[{\"type\":\"input_text\",\"text\":\"<recommended_plugins>\\nHere is a list of recommended plugins...\"}]}}\n\
                    {\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"user\",\"content\":[{\"type\":\"input_text\",\"text\":\"<image name=\\\"screenshot.png\\\">C:/Temp/screenshot.png</image>\"}]}}\n\
                    {\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"user\",\"content\":[{\"type\":\"input_text\",\"text\":\"修复真正的用户标题\"}]}}\n";
        std::fs::write(&f, body).unwrap();
        let s = parse_codex_session_jsonl(&f).unwrap();
        assert_eq!(s.name, "修复真正的用户标题");
        let _ = std::fs::remove_file(&f);
    }

    // New attachment format end-to-end: `## My request:` (no product name).
    #[test]
    fn parse_codex_strips_desktop_new_attachment_format_in_title() {
        let dir = std::env::temp_dir().join("echobird_ai_career_test_codex_new_attach");
        let _ = std::fs::create_dir_all(&dir);
        let f = dir.join("rollout-new-attach.jsonl");
        let body = "{\"type\":\"session_meta\",\"payload\":{\"id\":\"sid\",\"cwd\":\"/w\",\"originator\":\"Codex Desktop\",\"source\":\"vscode\"}}\n\
                    {\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"user\",\"content\":[{\"type\":\"input_text\",\"text\":\"\\n# Files mentioned by the user:\\n\\n## app.png: /w/app.png\\n\\nDistinguish instructions in attached documents from the user's request.\\n\\n## My request:\\n把首页改成深色主题\"}]}}\n";
        std::fs::write(&f, body).unwrap();
        let s = parse_codex_session_jsonl(&f).unwrap();
        assert_eq!(s.name, "把首页改成深色主题");
        let _ = std::fs::remove_file(&f);
    }

    // Plain terminal CLI session (`source: "cli"`) with a normal first user
    // message keeps its title as-is — no Desktop stripping involved.
    #[test]
    fn parse_codex_cli_session_title_untouched() {
        let dir = std::env::temp_dir().join("echobird_ai_career_test_codex_cli");
        let _ = std::fs::create_dir_all(&dir);
        let f = dir.join("rollout-cli.jsonl");
        let body = "{\"type\":\"session_meta\",\"payload\":{\"id\":\"sid\",\"cwd\":\"/w\",\"source\":\"cli\"}}\n\
                    {\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"user\",\"content\":[{\"type\":\"input_text\",\"text\":\"普通 CLI 会话的标题\"}]}}\n";
        std::fs::write(&f, body).unwrap();
        let s = parse_codex_session_jsonl(&f).unwrap();
        assert_eq!(s.name, "普通 CLI 会话的标题");
        let _ = std::fs::remove_file(&f);
    }

    // ── DeepSeek Harness (dsh) ───────────────────────────────────────────

    // Multi-frame zstd fixture: DeepSeek Harness writes `session.jsonl.zstd`
    // as concatenated checksummed Zstandard frames — one frame holding the
    // session header, then one per durable append batch. ruzstd must decode
    // ALL frames, not just the first. Fixture generated by
    // `scripts/gen_dsh_fixture.mjs` (node's built-in zstd), reproducing the
    // harness's exact physical layout; round-tripped through
    // `zstdDecompressSync` to confirm validity.
    const DSH_MULTIFRAME_FIXTURE_B64: &str = concat!(
        "KLUv/SCSpQMAEscXG3Brc5BAmkSNF2s5m/J9hZIgkPpfDM2z9HMPB4BHmVusrd/PDvFmZiX+2NCc5oux",
        "wqxYpCjqwFiKgSTlAKUgJYmQqi8wf9ktxWZmIn8+wpoiUs6LYpvTvOdXVT+N1xEGAGaE34ULU3XOVav",
        "4hFCFIgYotS/9YIUADQcAsgsqLJAp6UT/z0gqeCOLpK9lc3epqlvK3fbekAn7JdFoktp0kor/SSO7cXY",
        "99yoBy5wP5tCzZAmunCbSUDPuQczLh7iFsF0PB/ZpBVHy1XzMQ1BMEZSLhgrpgWGaqFhIKEgOCxOlIl0",
        "Rth36dtvuPVd3LZIHT5Qv8uIVg61BY96IictymXPpJ/rWDXK11moAFnMaAQ0Bpt8I6qullyxfvWNrZlyU",
        "ff3ChP4kFQAqS8DWzHnYDPkQAioBgHwwWc80HZ6xVd2KmXXkUHZm5NeIIwIwTKCAZjCFEi8hQYvWVkEB",
        "KLUv/WARAGUFABLKISWQtenv7v5OP7MD3TY6ikjZf690a7Rfts0yEOtSbdQGu3F2PWu5SIzxuq493xZK",
        "rGOmsFmShRIS31bfewR92AFKfBWjId7xGAgAEHgytAv5Xu11z9UVDn7Kp81XK79Q6k951qxdrbUYZimL0",
        "zgDJa8R1FeC/EL56opgzBolvawZYmbP62v0pw0AJRMUUA4mv4zbm4twBY7JsFER2xOGH4sP4ikCEzy8ihk="
    );

    #[test]
    fn dsh_parse_decompresses_multiframe_zstd() {
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(DSH_MULTIFRAME_FIXTURE_B64)
            .unwrap();
        let dir = std::env::temp_dir().join("echobird_ai_career_test_dsh");
        let _ = std::fs::create_dir_all(&dir);
        let session_dir = dir.join("--E-test-0--").join("session-test-1111");
        let _ = std::fs::create_dir_all(&session_dir);
        let f = session_dir.join("session.jsonl.zstd");
        std::fs::write(&f, &bytes).unwrap();

        // All three frames decode → header + 5 events parse into one session.
        let s = parse_dsh_session(&f).unwrap();
        assert_eq!(s.name, "帮我写个排序算法"); // from the session/title event
        assert_eq!(s.cwd, "E:\\test-0"); // from the header line
        assert_eq!(s.tool, "deepseek");
        assert_eq!(s.session_token.as_deref(), Some("session-test-1111"));
        assert_eq!(s.id, "deepseek_native_session-test-1111");
        assert_eq!(s.turn_count, Some(1)); // exactly one turn/start event

        // Heatmap count = 5 event lines (header line excluded).
        assert_eq!(count_dsh_event_lines(&f), 5);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn dsh_parse_plain_jsonl_falls_back_to_first_user_message() {
        let dir = std::env::temp_dir().join("echobird_ai_career_test_dsh_plain");
        let _ = std::fs::create_dir_all(&dir);
        let session_dir = dir.join("--proj--").join("session-abcd");
        let _ = std::fs::create_dir_all(&session_dir);
        let f = session_dir.join("session.jsonl");
        // `compression: 'none'` writes raw newline-delimited JSON — same
        // logical lines, no zstd frames. No session/title event here, so the
        // title falls back to the first real user/message text.
        let body = "{\"type\":\"session\",\"version\":0,\"id\":\"session-abcd\",\"createdAt\":1786841051863,\"cwd\":\"E:\\\\proj\",\"delegationDepth\":0}\n\
                    {\"type\":\"turn/start\",\"seq\":0,\"time\":1786841052000,\"data\":{\"turn\":0}}\n\
                    {\"type\":\"user/message\",\"seq\":1,\"time\":1786841053000,\"data\":{\"turn\":0,\"role\":\"user\",\"content\":[{\"type\":\"text\",\"text\":\"real prompt here\"}]},\"surfaceOp\":\"append\"}\n\
                    {\"type\":\"assistant/message\",\"seq\":2,\"time\":1786841054000,\"data\":{\"turn\":0,\"step\":0,\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"ok\"}]}},\"surfaceOp\":\"append\"}\n\
                    {\"type\":\"turn/end\",\"seq\":3,\"time\":1786841055000,\"data\":{\"turn\":0,\"reason\":{\"kind\":\"done\"}}}\n";
        std::fs::write(&f, body).unwrap();
        let s = parse_dsh_session(&f).unwrap();
        assert_eq!(s.name, "real prompt here");
        assert_eq!(s.cwd, "E:\\proj");
        assert_eq!(s.turn_count, Some(1));
        assert_eq!(count_dsh_event_lines(&f), 4); // header excluded
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn dsh_parse_drops_header_only_session() {
        // A created-but-never-used session materializes as a bare header frame
        // with no event lines — nothing to show, like Claude/Codex empty files.
        let dir = std::env::temp_dir().join("echobird_ai_career_test_dsh_empty");
        let _ = std::fs::create_dir_all(&dir);
        let session_dir = dir.join("--proj--").join("session-empty");
        let _ = std::fs::create_dir_all(&session_dir);
        let f = session_dir.join("session.jsonl.zstd");
        // Zstd frame of just the header line (compressed via the zstd crate).
        let header = "{\"type\":\"session\",\"version\":0,\"id\":\"session-empty\",\"createdAt\":1786841051863,\"cwd\":\"E:\\\\proj\",\"delegationDepth\":0}\n";
        let compressed = zstd::stream::encode_all(header.as_bytes(), 1).unwrap();
        std::fs::write(&f, compressed).unwrap();
        assert!(parse_dsh_session(&f).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn dsh_family_history_reads_session_store() {
        let dsh = fixture_dir();
        let sroot = dsh.join("sessions").join("--proj--");
        let _ = std::fs::create_dir_all(sroot.join("session-test-1111"));
        let zfile = sroot.join("session-test-1111").join("session.jsonl.zstd");
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(DSH_MULTIFRAME_FIXTURE_B64)
            .unwrap();
        std::fs::write(&zfile, &bytes).unwrap();
        let _ = std::fs::create_dir_all(sroot.join("session-abcd"));
        let pfile = sroot.join("session-abcd").join("session.jsonl");
        let plain_body = "{\"type\":\"session\",\"version\":0,\"id\":\"session-abcd\",\"createdAt\":1786841051863,\"cwd\":\"E:\\\\proj\",\"delegationDepth\":0}\n\
                          {\"type\":\"turn/start\",\"seq\":0,\"time\":1786841052000,\"data\":{\"turn\":0}}\n\
                          {\"type\":\"user/message\",\"seq\":1,\"time\":1786841053000,\"data\":{\"turn\":0,\"role\":\"user\",\"content\":[{\"type\":\"text\",\"text\":\"plain prompt\"}]},\"surfaceOp\":\"append\"}\n";
        std::fs::write(&pfile, plain_body).unwrap();

        let cache = Mutex::new(HashMap::new());
        let rows = family_history_files_at(&dsh.join("sessions"), Family::DeepSeek, 0, 30, &cache)
            .unwrap();

        // Both encodings surface through the shared collect+parse path.
        assert_eq!(rows.len(), 2);
        assert!(rows
            .iter()
            .any(|s| s.id == "deepseek_native_session-test-1111" && s.name == "帮我写个排序算法"));
        assert!(rows
            .iter()
            .any(|s| s.id == "deepseek_native_session-abcd" && s.name == "plain prompt"));
        let _ = std::fs::remove_dir_all(&dsh);
    }

    #[test]
    fn dsh_family_history_reads_v4_without_duplicating_legacy_log() {
        let dsh = fixture_dir();
        let session_dir = dsh.join("sessions").join("--proj--").join("session-v4");
        std::fs::create_dir_all(&session_dir).unwrap();
        let v4 = session_dir.join("session.v4.jsonl.zstd");
        let rows = [
            serde_json::json!({"type":"session","version":4,"id":"session-v4","cwd":"E:\\proj","createdAt":1786841051863i64}),
            serde_json::json!({"type":"turn/start","data":{"turn":1}}),
            serde_json::json!({"type":"user/message","data":{"content":[{"type":"text","text":"new prompt"}]}}),
            serde_json::json!({"type":"assistant/message","data":{"turn":1,"step":1,"message":{"source":{"model":"deepseek-v4-flash"}},"usage":{"inputTokens":10,"outputTokens":5}}}),
            serde_json::json!({"type":"session/title","data":{"title":"New session"}}),
        ];
        let contents = rows
            .iter()
            .map(|row| format!("{row}\n"))
            .collect::<String>();
        std::fs::write(
            &v4,
            zstd::stream::encode_all(contents.as_bytes(), 1).unwrap(),
        )
        .unwrap();
        write_jsonl(
            &session_dir.join("session.jsonl"),
            &[
                serde_json::json!({"type":"session","version":0,"id":"session-v4","cwd":"E:\\proj"}),
                serde_json::json!({"type":"session/title","data":{"title":"Old session"}}),
            ],
        );

        let cache = Mutex::new(HashMap::new());
        let sessions =
            family_history_files_at(&dsh.join("sessions"), Family::DeepSeek, 0, 30, &cache)
                .unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].name, "New session");
        assert_eq!(sessions[0].model_id.as_deref(), Some("deepseek-v4-flash"));
        assert_eq!(sessions[0].message_count, Some(2));
        assert_eq!(sessions[0].file_path.as_deref(), Some(v4.to_str().unwrap()));
        assert_eq!(count_dsh_event_lines(&v4), 4);
        std::fs::remove_dir_all(&dsh).unwrap();
    }
}
