// Post-action verification for known install/repair intents.
//
// After a successful shell_exec install command (or file_write to a known
// config), match the action against an intent table. If a matching verifier
// exists, run it. If verification fails, the original ToolResult is rewritten
// with a "completed but verification failed" message so the next ReAct
// iteration sees the failure and naturally re-plans.
//
// The intent table is intentionally narrow: each entry is a deterministic
// signature, not a heuristic. False positives here mean wasted verifier runs;
// false negatives mean the install fails silently — bias toward narrow.
//
// This is the "install/repair Agent core shape": verify → if fail, feed
// diagnostic back to the LLM → loop until pass or budget exhausted.

use super::agent_tools::{exec_shell, ToolResult};
use crate::commands::ssh_commands::SSHPool;

/// A verifiable install or config-write action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallIntent {
    ClaudeCode,
    Codex,
    GeminiCli,
    /// File path that was just written and should parse as JSON.
    JsonConfig(String),
}

impl InstallIntent {
    pub fn label(&self) -> &str {
        match self {
            InstallIntent::ClaudeCode => "Claude Code",
            InstallIntent::Codex => "Codex",
            InstallIntent::GeminiCli => "Gemini CLI",
            InstallIntent::JsonConfig(_) => "JSON config file",
        }
    }
}

/// Match a shell command against the known-intent table. Returns `Some` only
/// for installs that mutate state and have a deterministic verifier.
pub fn detect_install_intent_from_shell(command: &str) -> Option<InstallIntent> {
    let cmd = command.to_lowercase();

    // Claude Code — npm package `@anthropic-ai/claude-code`.
    if cmd.contains("@anthropic-ai/claude-code") {
        return Some(InstallIntent::ClaudeCode);
    }

    // Codex — npm package `@openai/codex`. (Some docs reference `codex-cli`;
    // both share the same `codex` binary, so the verifier is the same.)
    if cmd.contains("@openai/codex") || cmd.contains("@openai/codex-cli") {
        return Some(InstallIntent::Codex);
    }

    // Gemini CLI — npm package `@google/gemini-cli`.
    if cmd.contains("@google/gemini-cli") {
        return Some(InstallIntent::GeminiCli);
    }

    None
}

/// Match a written file path against config-validation intents.
pub fn detect_install_intent_from_write(path: &str) -> Option<InstallIntent> {
    let lower = path.to_lowercase().replace('\\', "/");
    if lower.ends_with("/claude_desktop_config.json")
        || lower.ends_with("/mcp.json")
        || lower.ends_with("/.mcp.json")
    {
        return Some(InstallIntent::JsonConfig(path.to_string()));
    }
    None
}

/// Run the verifier for an intent. Returns `Ok(())` on pass, `Err(reason)`
/// when the install reported success but verification failed.
pub async fn verify(
    intent: &InstallIntent,
    server_id: &str,
    ssh_pool: &SSHPool,
) -> Result<(), String> {
    match intent {
        InstallIntent::ClaudeCode => {
            verify_command_present("claude --version", server_id, ssh_pool).await
        }
        InstallIntent::Codex => {
            verify_command_present("codex --version", server_id, ssh_pool).await
        }
        InstallIntent::GeminiCli => {
            verify_command_present("gemini --version", server_id, ssh_pool).await
        }
        InstallIntent::JsonConfig(path) => verify_json_file(path, server_id, ssh_pool).await,
    }
}

async fn verify_command_present(
    cmd: &str,
    server_id: &str,
    ssh_pool: &SSHPool,
) -> Result<(), String> {
    let r = exec_shell(cmd, server_id, ssh_pool).await;
    if !r.success {
        return Err(format!(
            "`{}` failed:\n{}",
            cmd,
            first_n_lines(&r.output, 5)
        ));
    }
    // Empty stdout from a `--version` check is suspicious — the binary may exist
    // but be broken (npm shim with wrong shebang, etc.). Treat as a failure.
    if r.output.trim().is_empty() {
        return Err(format!(
            "`{}` succeeded but printed no output — binary may be broken or not on PATH",
            cmd
        ));
    }
    Ok(())
}

async fn verify_json_file(path: &str, server_id: &str, ssh_pool: &SSHPool) -> Result<(), String> {
    // Use python3 over jq — jq isn't always installed, python3 is on every
    // Linux/macOS we deploy to and Windows has it via the launcher when MCP
    // configs come up. Quote the path for spaces.
    let cmd = format!(
        "python3 -c \"import json,sys; json.load(open(r'{}'))\" 2>&1 || \
         python -c \"import json,sys; json.load(open(r'{}'))\" 2>&1",
        path, path
    );
    let r = exec_shell(&cmd, server_id, ssh_pool).await;
    if !r.success {
        return Err(format!(
            "{} did not parse as JSON:\n{}",
            path,
            first_n_lines(&r.output, 5)
        ));
    }
    Ok(())
}

/// Wrap a ToolResult with a verification failure. The original output stays
/// (the model needs it for context) but the success flag flips and a banner
/// tells the model what to fix next.
pub fn wrap_failure(original: ToolResult, intent: &InstallIntent, reason: String) -> ToolResult {
    let banner = format!(
        "\n\n--- VERIFICATION FAILED ---\n\
         The {} action reported success, but post-action verification failed:\n\
         {}\n\
         Diagnose the cause and fix it. Common causes: PATH not refreshed (run \
         `hash -r` or open a new shell), wrong package name, install partially \
         failed mid-way, sudo required, or a syntax error in the written file.",
        intent.label(),
        reason,
    );
    ToolResult {
        success: false,
        output: format!("{}{}", original.output, banner),
    }
}

fn first_n_lines(s: &str, n: usize) -> String {
    s.lines().take(n).collect::<Vec<_>>().join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_claude_code_npm_install() {
        let i = detect_install_intent_from_shell("npm install -g @anthropic-ai/claude-code");
        assert_eq!(i, Some(InstallIntent::ClaudeCode));
    }

    #[test]
    fn detects_claude_code_via_npm_i_shorthand() {
        let i = detect_install_intent_from_shell("sudo npm i -g @anthropic-ai/claude-code@latest");
        assert_eq!(i, Some(InstallIntent::ClaudeCode));
    }

    #[test]
    fn detects_codex_both_package_names() {
        assert_eq!(
            detect_install_intent_from_shell("npm install -g @openai/codex"),
            Some(InstallIntent::Codex)
        );
        assert_eq!(
            detect_install_intent_from_shell("npm i -g @openai/codex-cli"),
            Some(InstallIntent::Codex)
        );
    }

    #[test]
    fn detects_gemini_cli() {
        assert_eq!(
            detect_install_intent_from_shell("npm install -g @google/gemini-cli"),
            Some(InstallIntent::GeminiCli)
        );
    }

    #[test]
    fn case_insensitive_match() {
        assert_eq!(
            detect_install_intent_from_shell("NPM INSTALL -g @ANTHROPIC-AI/CLAUDE-CODE"),
            Some(InstallIntent::ClaudeCode)
        );
    }

    #[test]
    fn unrelated_command_yields_none() {
        assert_eq!(detect_install_intent_from_shell("ls -la /tmp"), None);
        assert_eq!(
            detect_install_intent_from_shell("pip install requests"),
            None
        );
    }

    #[test]
    fn detects_mcp_config_path_unix() {
        let i =
            detect_install_intent_from_write("/home/u/.config/Claude/claude_desktop_config.json");
        assert!(matches!(i, Some(InstallIntent::JsonConfig(_))));
    }

    #[test]
    fn detects_mcp_config_path_windows() {
        let i = detect_install_intent_from_write(
            r"C:\Users\eben\AppData\Roaming\Claude\claude_desktop_config.json",
        );
        assert!(matches!(i, Some(InstallIntent::JsonConfig(_))));
    }

    #[test]
    fn detects_dot_mcp_json() {
        let i = detect_install_intent_from_write("/repo/.mcp.json");
        assert!(matches!(i, Some(InstallIntent::JsonConfig(_))));
    }

    #[test]
    fn ignores_unrelated_writes() {
        assert!(detect_install_intent_from_write("/tmp/random.txt").is_none());
        assert!(detect_install_intent_from_write("/etc/hosts").is_none());
    }

    #[test]
    fn wrap_failure_preserves_original_output() {
        let original = ToolResult {
            success: true,
            output: "added 1 package".to_string(),
        };
        let wrapped = wrap_failure(
            original,
            &InstallIntent::ClaudeCode,
            "claude not found".into(),
        );
        assert!(!wrapped.success);
        assert!(
            wrapped.output.contains("added 1 package"),
            "must keep original output"
        );
        assert!(wrapped.output.contains("VERIFICATION FAILED"));
        assert!(wrapped.output.contains("claude not found"));
        assert!(wrapped.output.contains("Claude Code"));
    }
}
