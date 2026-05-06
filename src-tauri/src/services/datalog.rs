// Per-turn datalog for the agent loop.
//
// Each run_agent invocation opens one markdown file under
//   ~/.echobird/datalog/<server-key-slug>/YYYY-MM-DD_HH-MM-SS.md
// and appends as the loop progresses. Every write flushes to disk so logs
// survive crashes (a deployment Agent that crashes mid-fix is exactly when
// we want the trail).
//
// Borrowed shape from atomcode (turn/datalog.rs); paths and slugging are
// simpler — server_key is already a per-server identifier and is sanitized
// for filesystem use here.

use std::fmt::Write as FmtWrite;
use std::path::{Path, PathBuf};
use std::time::Instant;

/// Accumulates a single turn's log and flushes after each write.
pub struct DatalogWriter {
    enabled: bool,
    buf: String,
    active: bool,
    start: Option<Instant>,
    llm_turn_start: Option<Instant>,
    step: usize,
    tool_count: usize,
    file_path: Option<PathBuf>,
}

impl DatalogWriter {
    /// Build a writer for a given server key. Pass `enabled = false` for a
    /// no-op writer (every method returns immediately, no files created).
    pub fn new(enabled: bool) -> Self {
        Self {
            enabled,
            buf: String::new(),
            active: false,
            start: None,
            llm_turn_start: None,
            step: 0,
            tool_count: 0,
            file_path: None,
        }
    }

    /// `~/.echobird/datalog/<slug>` — root for one server's turn logs.
    fn log_dir(server_key: &str) -> PathBuf {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".echobird")
            .join("datalog")
            .join(slugify(server_key))
    }

    /// Begin a new turn: create the .md file, write env header + user message.
    pub fn begin_turn(&mut self, server_key: &str, user_message: &str, model_name: &str) {
        if !self.enabled {
            return;
        }
        self.buf.clear();
        self.step = 0;
        self.tool_count = 0;
        self.active = true;
        self.start = Some(Instant::now());
        self.llm_turn_start = None;

        let timestamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
        let filename = format!("{}.md", timestamp.replace(' ', "_").replace(':', "-"));
        let dir = Self::log_dir(server_key);
        if let Err(e) = std::fs::create_dir_all(&dir) {
            log::warn!("[Datalog] failed to create dir {:?}: {}", dir, e);
            self.active = false;
            return;
        }
        self.file_path = Some(dir.join(filename));

        let _ = writeln!(&mut self.buf, "# Turn {}", timestamp);
        let _ = writeln!(&mut self.buf, "**env:** model={}, server={}", model_name, server_key);
        let _ = writeln!(&mut self.buf);
        let _ = writeln!(&mut self.buf, "## User");
        let _ = writeln!(&mut self.buf, "```");
        let _ = writeln!(&mut self.buf, "{}", user_message);
        let _ = writeln!(&mut self.buf, "```");
        let _ = writeln!(&mut self.buf);
        let _ = writeln!(&mut self.buf, "## Agent");
        let _ = writeln!(&mut self.buf);
        self.flush();
    }

    /// Mark the start of a new LLM round-trip (one ReAct iteration).
    pub fn log_llm_call(&mut self) {
        if !self.active {
            return;
        }
        if let Some(prev) = self.llm_turn_start {
            let dur = prev.elapsed();
            if dur.as_millis() >= 1000 {
                let _ = writeln!(&mut self.buf, "  _({:.1}s)_\n", dur.as_secs_f64());
            }
        }
        self.step += 1;
        let _ = writeln!(&mut self.buf, "### Step {}", self.step);
        self.llm_turn_start = Some(Instant::now());
        self.flush();
    }

    /// Log a tool call (name + summarized args).
    pub fn log_tool_call(&mut self, name: &str, args: &str) {
        if !self.active {
            return;
        }
        self.tool_count += 1;
        let detail = format_tool_args(name, args);
        let _ = writeln!(&mut self.buf, "- **{}** {}", name, detail);
        if serde_json::from_str::<serde_json::Value>(args).is_err() {
            let preview: String = args.chars().take(160).collect();
            let _ = writeln!(&mut self.buf, "  [raw args (unparsed): {}]", preview);
        }
        self.flush();
    }

    /// Log a tool result. `output` is summarized — first line + line count.
    pub fn log_tool_result(&mut self, output: &str, success: bool) {
        if !self.active {
            return;
        }
        let icon = if success { "+" } else { "x" };
        let first_line = output.lines().next().unwrap_or("");
        let summary: String = if first_line.chars().count() > 100 {
            let head: String = first_line.chars().take(97).collect();
            format!("{}...", head)
        } else {
            first_line.to_string()
        };
        let total = output.lines().count();
        if total > 1 {
            let _ = writeln!(&mut self.buf, "  {} {} ({} lines)", icon, summary, total);
        } else {
            let _ = writeln!(&mut self.buf, "  {} {}", icon, summary);
        }
        let _ = writeln!(&mut self.buf);
        self.flush();
    }

    /// Log final assistant text (or any free-form text mid-turn).
    pub fn log_text(&mut self, text: &str) {
        if !self.active {
            return;
        }
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return;
        }
        let display: String = if trimmed.chars().count() > 500 {
            let head: String = trimmed.chars().take(497).collect();
            format!("{}...", head)
        } else {
            trimmed.to_string()
        };
        let _ = writeln!(&mut self.buf, "  > {}", display.replace('\n', "\n  > "));
        let _ = writeln!(&mut self.buf);
        self.flush();
    }

    /// Log an error message.
    pub fn log_error(&mut self, error: &str) {
        if !self.active {
            return;
        }
        let _ = writeln!(&mut self.buf, "**Error:** {}", error);
        let _ = writeln!(&mut self.buf);
        self.flush();
    }

    /// End the turn: append duration / tool count / step count.
    pub fn end_turn(&mut self) {
        if !self.active {
            return;
        }
        self.active = false;

        if let Some(prev) = self.llm_turn_start.take() {
            let dur = prev.elapsed();
            if dur.as_millis() >= 1000 {
                let _ = writeln!(&mut self.buf, "  _({:.1}s)_", dur.as_secs_f64());
            }
        }

        let total = self.start.map(|s| s.elapsed()).unwrap_or_default();
        let _ = writeln!(&mut self.buf);
        let _ = writeln!(&mut self.buf, "---");
        let _ = writeln!(
            &mut self.buf,
            "**Stats:** {} steps, {} tool calls, {:.1}s",
            self.step,
            self.tool_count,
            total.as_secs_f64(),
        );
        self.flush();
    }

    /// Path of the markdown file currently being written, if any. Useful for
    /// tests; production code shouldn't depend on this.
    pub fn file_path(&self) -> Option<&Path> {
        self.file_path.as_deref()
    }

    fn flush(&self) {
        if let Some(ref path) = self.file_path {
            if let Err(e) = std::fs::write(path, &self.buf) {
                log::warn!("[Datalog] flush failed for {:?}: {}", path, e);
            }
        }
    }
}

/// Sanitize a server key into a filesystem-safe slug. `local` stays `local`;
/// SSH keys with `:` `/` `\` etc. get those characters folded to `_`.
fn slugify(s: &str) -> String {
    let cleaned: String = s
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if cleaned.is_empty() {
        "unknown".to_string()
    } else {
        cleaned
    }
}

/// Render a one-line summary of a tool call's arguments. Best-effort: returns
/// an empty string for un-parseable args (the caller will have already logged
/// the raw form).
fn format_tool_args(tool_name: &str, args_json: &str) -> String {
    let v: serde_json::Value = match serde_json::from_str(args_json) {
        Ok(v) => v,
        Err(_) => return String::new(),
    };
    match tool_name {
        "shell_exec" => {
            let cmd = v.get("command").and_then(|x| x.as_str()).unwrap_or("");
            if cmd.chars().count() > 80 {
                let head: String = cmd.chars().take(77).collect();
                format!("`{}...`", head)
            } else {
                format!("`{}`", cmd)
            }
        }
        "file_read" | "file_write" | "file_edit" => {
            let path = v
                .get("path")
                .or_else(|| v.get("file_path"))
                .and_then(|x| x.as_str())
                .unwrap_or("");
            short_path(path)
        }
        "grep" => {
            let pat = v.get("pattern").and_then(|x| x.as_str()).unwrap_or("");
            let path = v.get("path").and_then(|x| x.as_str()).unwrap_or(".");
            format!("\"{}\" in {}", pat, short_path(path))
        }
        "glob" => {
            let pat = v.get("pattern").and_then(|x| x.as_str()).unwrap_or("");
            format!("\"{}\"", pat)
        }
        _ => match v.as_object() {
            Some(obj) => obj
                .iter()
                .map(|(k, val)| {
                    let s = match val {
                        serde_json::Value::String(s) if s.chars().count() > 30 => {
                            let head: String = s.chars().take(27).collect();
                            format!("{}...", head)
                        }
                        serde_json::Value::String(s) => s.clone(),
                        other => other.to_string(),
                    };
                    format!("{}={}", k, s)
                })
                .collect::<Vec<_>>()
                .join(" "),
            None => String::new(),
        },
    }
}

fn short_path(path: &str) -> String {
    let p = path.replace('\\', "/");
    let parts: Vec<&str> = p.rsplitn(3, '/').collect();
    match parts.len() {
        0 | 1 => p,
        2 => format!("{}/{}", parts[1], parts[0]),
        _ => format!(".../{}/{}", parts[1], parts[0]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_writer_is_noop() {
        let mut w = DatalogWriter::new(false);
        w.begin_turn("local", "hello", "test-model");
        w.log_llm_call();
        w.log_tool_call("shell_exec", r#"{"command":"ls"}"#);
        w.log_tool_result("output", true);
        w.end_turn();
        assert!(w.file_path().is_none());
    }

    #[test]
    fn slug_keeps_alphanum_and_dash() {
        assert_eq!(slugify("local"), "local");
        assert_eq!(slugify("server-1"), "server-1");
    }

    #[test]
    fn slug_replaces_unsafe_chars() {
        assert_eq!(slugify("user@host:22"), "user_host_22");
        assert_eq!(slugify("a/b\\c"), "a_b_c");
    }

    #[test]
    fn slug_empty_falls_back() {
        assert_eq!(slugify(""), "unknown");
    }

    #[test]
    fn format_args_shell_exec_truncates() {
        let long = "x".repeat(200);
        let out = format_tool_args("shell_exec", &format!(r#"{{"command":"{}"}}"#, long));
        assert!(out.starts_with("`"));
        assert!(out.contains("..."));
    }

    #[test]
    fn format_args_file_read_uses_short_path() {
        let out = format_tool_args("file_read", r#"{"path":"/a/b/c/file.rs"}"#);
        assert!(out.contains("file.rs"));
    }

    #[test]
    fn format_args_unknown_tool_renders_kv() {
        let out = format_tool_args("custom_tool", r#"{"flag":true,"n":3}"#);
        assert!(out.contains("flag=true"));
        assert!(out.contains("n=3"));
    }

    #[test]
    fn format_args_invalid_json_returns_empty() {
        assert_eq!(format_tool_args("anything", "not json"), "");
    }

    #[test]
    fn end_to_end_writes_file_with_stats() {
        let key = format!("test-{}", uuid::Uuid::new_v4());
        let mut w = DatalogWriter::new(true);
        w.begin_turn(&key, "install Claude Code", "test-model");
        w.log_llm_call();
        w.log_tool_call("shell_exec", r#"{"command":"npm i -g claude"}"#);
        w.log_tool_result("ok", true);
        w.log_text("done.");
        w.end_turn();

        let path = w.file_path().expect("file path should be set").to_path_buf();
        let content = std::fs::read_to_string(&path).expect("file should exist");
        assert!(content.contains("install Claude Code"));
        assert!(content.contains("Step 1"));
        assert!(content.contains("shell_exec"));
        assert!(content.contains("1 steps, 1 tool calls"));

        let _ = std::fs::remove_file(&path);
    }
}
