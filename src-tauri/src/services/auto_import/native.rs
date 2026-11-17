//! Explicit native account discovery. Adapters only persist EchoBird-owned state.
use super::super::*;

pub(crate) fn same_account(id: &str, email: &str, existing_id: &str, existing_email: &str) -> bool {
    id == existing_id
        || (email.contains('@') && email.trim().eq_ignore_ascii_case(existing_email.trim()))
}

#[derive(Default)]
pub(crate) struct Counts {
    pub added: usize,
    pub duplicates: usize,
    pub skipped: usize,
}
impl Counts {
    pub(crate) fn record(&mut self, result: Result<Option<bool>, String>) {
        match result {
            Ok(Some(true)) => self.added += 1,
            Ok(Some(false)) => self.duplicates += 1,
            Ok(None) => {}
            Err(_) => self.skipped += 1,
        }
    }
}

pub(super) const SOURCES: &[(&str, &[&str])] = &[
    ("codex", &["codex", "chatgptdesktop"]),
    ("claudecode", &["claudecode"]),
    ("claudedesktop", &["claudedesktop"]),
    ("antigravity", &["antigravity", "antigravitydesktop"]),
    ("workbuddy", &["workbuddy"]),
    ("workbuddyai", &["workbuddyai"]),
    ("trae", &["trae"]),
    ("traecn", &["traecn"]),
    ("dumate", &["dumate"]),
    ("qwenwork", &["qwenwork"]),
    ("qwenworkcn", &["qwenworkcn"]),
    ("codearts", &["codearts"]),
    ("zcode", &["zcode"]),
    ("opendesign", &["opendesign"]),
    ("kirocrew", &["kirocrew"]),
    ("kirocli", &["kirocli"]),
    ("grok", &["grok"]),
    ("grokbot", &["grokbot"]),
    ("dsh", &["dsh"]),
    ("cline", &["cline", "clinedesktop"]),
    ("copilot", &["copilot"]),
    ("copilotdesktop", &["copilotdesktop"]),
    ("doubaowork", &["doubaowork"]),
    ("dola", &["dola"]),
    ("qoder", &["qoder"]),
    ("qodercn", &["qodercn"]),
    ("devin", &["devin"]),
    ("devincli", &["devincli"]),
    ("cursor", &["cursor"]),
    ("cursorcli", &["cursorcli"]),
    ("commandcode", &["commandcode"]),
    ("kilo", &["kilo", "kilodesktop"]),
    ("manus", &["manus"]),
    ("cue", &["cue"]),
    ("kimicn", &["kimicode", "kimidesktop"]),
    ("kimiglobal", &["kimicode", "kimidesktop"]),
    ("minimaxcn", &["minimaxcode", "minimaxdesktop"]),
    ("minimaxglobal", &["minimaxcode", "minimaxdesktop"]),
];

pub(super) async fn scan(id: &str) -> Result<Counts, String> {
    let result = match id {
        "qoder" => qoder_accounts::import_native(qoder_accounts::Edition::Qoder).await,
        "qodercn" => qoder_accounts::import_native(qoder_accounts::Edition::QoderCn).await,
        "grok" => return grok_accounts::import_native(),
        "grokbot" => return grok_bot_accounts::import_native().await,
        "dsh" => deepseek_accounts::import_native().await,
        "cline" => return cline_accounts::import_native().await,
        "copilot" => return copilot_accounts::scan_native(false).await,
        "copilotdesktop" => return copilot_accounts::scan_native(true).await,
        "doubaowork" => {
            return doubao_accounts::import_native(doubao_accounts::Edition::DoubaoWork).await
        }
        "dola" => return doubao_accounts::import_native(doubao_accounts::Edition::Dola).await,
        "minimaxcn" => return minimax_accounts::import_native(minimax_accounts::Region::Cn).await,
        "minimaxglobal" => {
            return minimax_accounts::import_native(minimax_accounts::Region::Global).await
        }
        "cursor" => cursor_accounts::import_native(false).await,
        "cursorcli" => cursor_accounts::import_native(true).await,
        "commandcode" => commandcode_accounts::import_native().await,
        "kilo" => kilo_accounts::import_native().await,
        "manus" => manus_accounts::import_native().await,
        "cue" => cue_accounts::import_native().await,
        "kimicn" => kimi_accounts::import_native(kimi_accounts::Region::Cn).await,
        "kimiglobal" => kimi_accounts::import_native(kimi_accounts::Region::Global).await,
        "codex" => codex_accounts::import_native(),
        "claudecode" => claude_code_accounts::import_native().await,
        "claudedesktop" => claude_desktop_accounts::import_native().await,
        "antigravity" => antigravity_accounts::import_native().await,
        "workbuddy" => workbuddy_accounts::import_native(workbuddy_accounts::Edition::Cn).await,
        "workbuddyai" => workbuddy_accounts::import_native(workbuddy_accounts::Edition::Ai).await,
        "trae" => trae_accounts::import_native(trae_accounts::Edition::Trae).await,
        "traecn" => trae_accounts::import_native(trae_accounts::Edition::Traecn).await,
        "dumate" => dumate_accounts::import_native().await,
        "qwenwork" => qwenwork_accounts::import_native(qwenwork_accounts::Edition::QwenWork).await,
        "qwenworkcn" => {
            qwenwork_accounts::import_native(qwenwork_accounts::Edition::QwenWorkCn).await
        }
        "codearts" => codearts_accounts::import_native().await,
        "zcode" => zcode_accounts::import_native().await,
        "opendesign" => opendesign_accounts::import_native().await,
        "kirocrew" => kiro_accounts::import_native(kiro_accounts::Edition::Crew).await,
        "kirocli" => kiro_accounts::import_native(kiro_accounts::Edition::Cli).await,
        "devin" => devin_accounts::import_native(false).await,
        "devincli" => devin_accounts::import_native(true).await,
        _ => Err("accountError.invalidAccount".into()),
    };
    let mut counts = Counts::default();
    counts.record(result);
    Ok(counts)
}

pub(super) async fn scan_installed<F, Fut>(installed: bool, operation: F) -> Counts
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<Counts, String>>,
{
    if !installed {
        return Counts::default();
    }
    match operation().await {
        Ok(counts) => counts,
        Err(_) => Counts {
            skipped: 1,
            ..Default::default()
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_scan_slot_uses_real_tool_catalog_ids() {
        let tools = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../tools");
        let mut slots = std::collections::BTreeSet::new();
        for &(id, clients) in SOURCES {
            assert!(slots.insert(id), "duplicate scan slot {id}");
            assert!(!clients.is_empty());
            for client in clients {
                assert!(
                    tools.join(client).join("paths.json").is_file(),
                    "unknown native client {client}"
                );
            }
        }
    }

    #[tokio::test]
    async fn unavailable_sources_do_not_read_and_failures_do_not_stop_later_sources() {
        let calls = std::cell::Cell::new(0);
        let absent = scan_installed(false, || async {
            panic!("uninstalled source must not be touched")
        })
        .await;
        assert_eq!(absent.added, 0);
        let failed = scan_installed(true, || async {
            calls.set(calls.get() + 1);
            Err("private source error".into())
        })
        .await;
        assert_eq!(failed.skipped, 1);
        let valid = scan_installed(true, || async {
            calls.set(calls.get() + 1);
            let mut counts = Counts::default();
            counts.record(Ok(Some(true)));
            counts.record(Ok(Some(false)));
            counts.record(Err("corrupt record".into()));
            counts.record(Ok(None));
            counts.record(Ok(Some(true)));
            Ok(counts)
        })
        .await;
        assert_eq!((valid.added, valid.duplicates, valid.skipped), (2, 1, 1));
        assert_eq!(calls.get(), 2); // Positive control: a later installed source was actually read.
    }

    #[test]
    fn identity_matches_id_or_real_email_never_display_names() {
        assert!(same_account("id", "a@b", "id", "other@b"));
        assert!(same_account("id", " A@B ", "other", "a@b"));
        assert!(!same_account("id", "User", "other", "User"));
        assert!(!same_account("id", "", "other", ""));
    }
}
