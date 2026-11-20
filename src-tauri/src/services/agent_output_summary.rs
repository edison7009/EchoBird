use std::collections::{BTreeMap, HashSet};

use super::agent_output_store::OutputRecord;

const MAX_SUMMARY_BYTES: usize = 8_000;
const MAX_LINE_BYTES: usize = 600;
const DIAGNOSTIC_WORDS: &[&str] = &[
    "error",
    "failed",
    "failure",
    "warning",
    "panic",
    "exception",
    "traceback",
    "fatal",
    "denied",
    "not found",
    "unable",
    "caused by",
    "eacces",
    "enoent",
    "err!",
];

pub fn summarize(record: &OutputRecord, output_id: Option<&str>) -> String {
    let raw = record.combined();
    let status = if record.success { "success" } else { "failure" };
    let exit_code = record
        .exit_code
        .map_or_else(|| "unknown".to_string(), |code| code.to_string());
    let target = header_value(&record.target, 160);
    let recovery = match output_id {
        Some(id) => format!(
            "Output ID: {}\nRead full output with tool_output_read: {{\"output_id\":\"{}\",\"offset\":0,\"max_bytes\":6000}}\n",
            header_value(id, 64), header_value(id, 64)
        ),
        None => "Full output archive unavailable; omitted text cannot be recovered.\n".into(),
    };
    let header =
        format!("Status: {status}\nExit code: {exit_code}\nTarget: {target}\n{recovery}\n");
    let budget = MAX_SUMMARY_BYTES - header.len() - 256;
    if raw.len() <= budget {
        return format!(
            "{header}{}",
            if raw.is_empty() { "(no output)" } else { &raw }
        );
    }

    let normalized = if known_command(&record.command) {
        clean_known_output(&raw)
    } else {
        raw.clone()
    };
    let candidate = if normalized.len() < raw.len() {
        &normalized
    } else {
        &raw
    };
    let body = if candidate.len() <= budget {
        candidate.clone()
    } else {
        excerpt(candidate, budget)
    };
    let footer = format!(
        "\n[Output condensed from {} bytes / {} lines. Omitted text may include additional diagnostics.]",
        raw.len(),
        raw.lines().count()
    );
    format!("{header}{body}{footer}")
}

fn header_value(value: &str, max_bytes: usize) -> String {
    let cleaned: String = value.chars().filter(|ch| !ch.is_control()).collect();
    cleaned[..floor_boundary(&cleaned, max_bytes.min(cleaned.len()))].to_string()
}

fn known_command(command: &str) -> bool {
    if command.contains([';', '|', '&', '\n', '\r', '>', '<', '`']) || command.contains("$(") {
        return false;
    }
    let command = command.trim().to_ascii_lowercase();
    let command = command.strip_prefix("sudo ").unwrap_or(&command);
    let words: Vec<_> = command.split_whitespace().take(5).collect();
    matches!(
        words.as_slice(),
        ["npm" | "pnpm", "install" | "ci" | "build" | "test", ..]
            | ["npm" | "pnpm", "run", "build" | "test", ..]
            | ["yarn", "install" | "add" | "build" | "test", ..]
            | [
                "cargo",
                "build" | "check" | "test" | "install" | "clippy",
                ..
            ]
            | ["pip" | "pip3", "install", ..]
            | ["python" | "python3", "-m", "pip", "install", ..]
            | ["uv", "pip", "install", ..]
            | ["uv", "sync", ..]
            | ["apt" | "apt-get", "install" | "update" | "upgrade", ..]
            | ["brew", "install" | "update" | "upgrade", ..]
    )
}

fn diagnostic(line: &str) -> bool {
    let line = line.to_ascii_lowercase();
    DIAGNOSTIC_WORDS.iter().any(|word| line.contains(word))
}

fn severe_diagnostic(line: &str) -> bool {
    let line = line.to_ascii_lowercase();
    DIAGNOSTIC_WORDS
        .iter()
        .any(|word| *word != "warning" && line.contains(word))
}

fn clean_known_output(raw: &str) -> String {
    let stripped = strip_ansi(raw);
    let mut seen_activity = HashSet::new();
    let mut lines = Vec::new();
    for line in stripped.split('\n') {
        let updates: Vec<_> = line.split('\r').collect();
        let diagnostics: Vec<_> = updates
            .iter()
            .copied()
            .filter(|part| diagnostic(part))
            .collect();
        let retained = if diagnostics.is_empty() {
            updates
                .iter()
                .rev()
                .find(|part| !part.is_empty())
                .copied()
                .into_iter()
                .collect()
        } else {
            diagnostics
        };
        for line in retained {
            let lower = line.trim_start().to_ascii_lowercase();
            if !diagnostic(line) {
                let progress = lower.starts_with("progress:")
                    || lower.starts_with("npm http ")
                    || lower.starts_with("npm sill ")
                    || lower.starts_with("npm timing ")
                    || (line.contains('%') && (line.contains('[') || line.contains('█')));
                let activity = [
                    "downloading ",
                    "compiling ",
                    "checking ",
                    "installing ",
                    "fetching ",
                ]
                .iter()
                .any(|prefix| lower.starts_with(prefix));
                if progress || (activity && !seen_activity.insert(line.to_string())) {
                    continue;
                }
            }
            lines.push(line);
        }
    }
    lines.join("\n")
}

pub(super) fn strip_ansi(raw: &str) -> String {
    let mut chars = raw.chars().peekable();
    let mut clean = String::with_capacity(raw.len());
    while let Some(ch) = chars.next() {
        if ch != '\u{1b}' {
            clean.push(ch);
            continue;
        }
        match chars.next() {
            Some('[') => {
                for ch in chars.by_ref() {
                    if ('@'..='~').contains(&ch) {
                        break;
                    }
                }
            }
            Some(']') => {
                while let Some(ch) = chars.next() {
                    if ch == '\u{7}' || (ch == '\u{1b}' && chars.next_if_eq(&'\\').is_some()) {
                        break;
                    }
                }
            }
            _ => {}
        }
    }
    clean
}

fn excerpt(raw: &str, budget: usize) -> String {
    let lines: Vec<_> = raw.lines().collect();
    let mut selected = BTreeMap::new();
    let mut remaining = budget;
    let mut add = |index: usize| {
        if selected.contains_key(&index) || remaining <= 80 {
            return;
        }
        let line = clip_line(lines[index], MAX_LINE_BYTES.min(remaining - 40));
        remaining -= line.len() + 40;
        selected.insert(index, line);
    };
    for index in 0..lines.len().min(2) {
        add(index);
    }
    for index in lines.len().saturating_sub(4)..lines.len() {
        add(index);
    }
    if let Some(index) = lines.iter().position(|line| *line == "--- stderr ---") {
        add(index);
        if index + 1 < lines.len() {
            add(index + 1);
        }
    }
    let (severe, warnings): (Vec<_>, Vec<_>) = lines
        .iter()
        .enumerate()
        .filter_map(|(index, line)| diagnostic(line).then_some(index))
        .partition(|&index| severe_diagnostic(lines[index]));
    for indices in [&severe, &warnings] {
        for &index in indices {
            add(index);
        }
        for &index in indices {
            for nearby in index.saturating_sub(2)..(index + 3).min(lines.len()) {
                add(nearby);
            }
        }
    }
    for index in 2..lines.len().min(6) {
        add(index);
    }

    let mut result = String::new();
    let mut next = 0;
    for (index, line) in selected {
        if index > next {
            result.push_str(&format!("[... {} lines omitted ...]\n", index - next));
        }
        result.push_str(&line);
        result.push('\n');
        next = index + 1;
    }
    if next < lines.len() {
        result.push_str(&format!("[... {} lines omitted ...]", lines.len() - next));
    }
    result
}

fn clip_line(line: &str, budget: usize) -> String {
    if line.len() <= budget {
        return line.to_string();
    }
    let marker_budget = 48;
    let side = budget.saturating_sub(marker_budget) / 2;
    let head = floor_boundary(line, side);
    let mut tail = line.len() - side;
    while !line.is_char_boundary(tail) {
        tail += 1;
    }
    let lower = line.to_ascii_lowercase();
    if let Some(offset) = DIAGNOSTIC_WORDS
        .iter()
        .filter_map(|word| lower.find(word))
        .min()
        .filter(|offset| budget >= 160 && *offset > head && *offset < tail)
    {
        let side = (budget - 96) / 4;
        let head = floor_boundary(line, side);
        let mut tail = line.len() - side;
        while !line.is_char_boundary(tail) {
            tail += 1;
        }
        let start = floor_boundary(line, offset.saturating_sub(side));
        let end = floor_boundary(line, (offset + side).min(tail));
        return format!(
            "{} [... {} bytes omitted ...] {} [... {} bytes omitted ...] {}",
            &line[..head],
            start - head,
            &line[start..end],
            tail - end,
            &line[tail..]
        );
    }
    format!(
        "{} [... {} bytes omitted ...] {}",
        &line[..head],
        tail - head,
        &line[tail..]
    )
}

fn floor_boundary(text: &str, mut index: usize) -> usize {
    while !text.is_char_boundary(index) {
        index -= 1;
    }
    index
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(command: &str, stdout: String, stderr: String, success: bool) -> OutputRecord {
        OutputRecord {
            command: command.into(),
            target: "local".into(),
            exit_code: Some(if success { 0 } else { 1 }),
            success,
            stdout,
            stderr,
        }
    }

    #[test]
    fn small_output_is_complete_and_status_uses_exit_result() {
        let input = record(
            "cargo test",
            "0 errors found\nAll tests passed\n".into(),
            "warning: harmless\n".into(),
            true,
        );
        let result = summarize(&input, Some("saved-id"));
        assert!(result.contains("Status: success\nExit code: 0"));
        assert!(result.ends_with(&input.combined()));
        assert!(!result.contains("condensed"));
    }

    #[test]
    fn early_failure_and_stderr_survive_progress_output() {
        let stdout = format!(
            "starting\nerror: dependency missing\n  required by package X\n{}done\n",
            "Progress: [99%]\n".repeat(2_000)
        );
        let input = record(
            "npm install",
            stdout,
            "fatal: could not install\n".into(),
            false,
        );
        let result = summarize(&input, Some("saved-id"));
        assert!(result.contains("Status: failure\nExit code: 1"));
        assert!(result.contains("error: dependency missing"));
        assert!(result.contains("required by package X"));
        assert!(result.contains("fatal: could not install"));
        assert!(!result.contains("Progress:"));
        assert!(result.contains("\"offset\":0,\"max_bytes\":6000"));
    }

    #[test]
    fn ansi_unicode_and_carriage_progress_keep_diagnostics() {
        let stdout = format!(
            "\u{1b}[31merror: 安装失败\u{1b}[0m\rProgress: [1%]\rProgress: [2%]\n{}",
            "Compiling 包\n".repeat(2_000)
        );
        let input = record("cargo build", stdout, String::new(), false);
        let result = summarize(&input, Some("saved-id"));
        assert!(result.contains("error: 安装失败"));
        assert!(!result.contains('\u{1b}'));
        assert!(!result.contains('\r'));
        assert_eq!(result.matches("Compiling 包").count(), 1);
        assert!(result.len() <= MAX_SUMMARY_BYTES);
    }

    #[test]
    fn unknown_and_compound_commands_keep_raw_progress_and_failure_windows() {
        for command in [
            "custom-tool",
            "npm install && custom-tool",
            "npm test | other-tool",
        ] {
            let stdout = format!(
                "first\n{}error: important failure\ncontext\n{}last\n",
                "Progress: [20%]\n".repeat(1_000),
                "Progress: [99%]\n".repeat(1_000)
            );
            let input = record(command, stdout, String::new(), false);
            let result = summarize(&input, Some("saved-id"));
            assert!(result.contains("first"));
            assert!(result.contains("last"));
            assert!(result.contains("Progress:"));
            assert!(result.contains("error: important failure"));
            assert!(result.contains("context"));
            assert!(result.len() <= MAX_SUMMARY_BYTES);
        }
    }

    #[test]
    fn very_long_lines_remain_bounded_and_recoverable() {
        let input = record(
            "custom-tool",
            format!("HEAD{}TAIL", "字".repeat(20_000)),
            "error: stderr problem".into(),
            false,
        );
        let result = summarize(&input, Some("saved-id"));
        assert!(result.contains("HEAD"));
        assert!(result.contains("TAIL"));
        assert!(result.contains("error: stderr problem"));
        assert!(result.contains("bytes omitted"));
        assert!(result.len() <= MAX_SUMMARY_BYTES);
        assert!(result.len() < input.combined().len());
    }

    #[test]
    fn diagnostic_in_the_middle_of_a_long_line_is_visible() {
        let input = record(
            "custom-tool",
            format!(
                "HEAD{}error: missing package{}TAIL",
                "字".repeat(5_000),
                "字".repeat(5_000)
            ),
            String::new(),
            false,
        );
        let result = summarize(&input, Some("saved-id"));
        assert!(result.contains("HEAD"));
        assert!(result.contains("error: missing package"));
        assert!(result.contains("TAIL"));
        assert!(result.len() <= MAX_SUMMARY_BYTES);
    }

    #[test]
    fn failure_in_the_middle_of_stderr_is_not_displaced_by_stdout() {
        let input = record(
            "custom-tool",
            "stdout progress\n".repeat(2_000),
            format!(
                "{}fatal: stderr failure\nimportant detail\n{}",
                "stderr progress\n".repeat(1_000),
                "stderr progress\n".repeat(1_000)
            ),
            false,
        );
        let result = summarize(&input, Some("saved-id"));
        assert!(result.contains("--- stderr ---"));
        assert!(result.contains("fatal: stderr failure"));
        assert!(result.contains("important detail"));
        assert!(result.len() <= MAX_SUMMARY_BYTES);
    }

    #[test]
    fn warning_flood_does_not_displace_a_unique_fatal_error() {
        let input = record(
            "cargo build",
            format!(
                "{}fatal: unique dependency failure\n  package required here\n{}",
                "warning: deprecated feature\n".repeat(2_000),
                "Compiling noisy dependency\n".repeat(2_000)
            ),
            String::new(),
            false,
        );
        let result = summarize(&input, Some("saved-id"));
        assert!(result.contains("fatal: unique dependency failure"));
        assert!(result.contains("package required here"));
        assert!(result.contains("Status: failure\nExit code: 1"));
        assert!(result.len() <= MAX_SUMMARY_BYTES);
    }

    #[test]
    fn archive_failure_is_explicit_without_changing_success() {
        let mut input = record("cargo test", "done\n".repeat(5_000), String::new(), true);
        input.exit_code = None;
        let result = summarize(&input, None);
        assert!(result.contains("Status: success\nExit code: unknown"));
        assert!(result.contains("archive unavailable"));
        assert!(!result.contains("tool_output_read"));
        assert!(result.len() <= MAX_SUMMARY_BYTES);
    }

    #[test]
    fn diagnostics_are_never_removed_as_repeated_noise() {
        let input = record(
            "pip install package",
            format!(
                "{}{}",
                "Progress: [99%]\n".repeat(1_000),
                "warning: retry failed\n".repeat(3)
            ),
            String::new(),
            false,
        );
        let result = summarize(&input, Some("saved-id"));
        assert_eq!(result.matches("warning: retry failed").count(), 3);
    }

    #[test]
    fn noisy_success_does_not_become_failure_due_to_diagnostic_words() {
        let input = record(
            "cargo test",
            format!(
                "{}test detects_error ... ok\nresult: all tests passed\n",
                "Compiling dependency\n".repeat(2_000)
            ),
            String::new(),
            true,
        );
        let result = summarize(&input, Some("saved-id"));
        assert!(result.contains("Status: success\nExit code: 0"));
        assert!(result.contains("test detects_error ... ok"));
        assert!(result.len() < input.combined().len());
    }

    #[test]
    fn arbitrary_metadata_cannot_overflow_the_summary() {
        let mut input = record("custom-tool", "output\n".repeat(2_000), String::new(), true);
        input.target = "远端\n".repeat(10_000);
        let result = summarize(&input, Some(&"id\n".repeat(10_000)));
        assert!(result.len() <= MAX_SUMMARY_BYTES);
    }
}
