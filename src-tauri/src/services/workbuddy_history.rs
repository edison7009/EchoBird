//! Share local WorkBuddy history when explicitly applying an account.
//! Only the active rows' owner changes; IDs, bodies and cloud mappings stay intact.

use rusqlite::{Connection, OpenFlags, TransactionBehavior};
use serde_json::Value;
use std::{fs, path::Path, time::Duration};

const BACKUP_PREFIX: &str = "workbuddy.db.eb-merge-bak-";

pub(super) fn apply(
    auth_path: &Path,
    data_root: &Path,
    original: &Value,
    next: &Value,
) -> Result<(), String> {
    let uid = super::text(&next["account"], "uid")
        .or_else(|| super::text(&next["account"], "id"))
        .ok_or("accountError.invalidAccount")?;
    let existed = auth_path.try_exists().map_err(history_error)?;
    let mut auth_written = false;
    let result = merge(data_root, uid, || {
        super::write(auth_path, next)?;
        auth_written = true;
        if super::read(auth_path).as_ref() != Ok(next) {
            return Err("accountError.write".into());
        }
        Ok(())
    });
    match result {
        Ok(changed) => {
            log::info!("[WorkBuddy] Shared {changed} local history rows");
            Ok(())
        }
        Err(error) => {
            // The SQLite transaction has rolled back before restoring credentials.
            // In particular, a commit failure must not leave the new account active.
            if auth_written {
                if existed {
                    super::write(auth_path, original)?;
                } else {
                    fs::remove_file(auth_path).map_err(history_error)?;
                }
            }
            log::warn!("[WorkBuddy] History/account switch failed: {error}");
            Err(error)
        }
    }
}

fn history_error(error: impl std::fmt::Display) -> String {
    format!("accountError.write|WorkBuddy history: {error}")
}

fn merge(
    data_root: &Path,
    uid: &str,
    apply_auth: impl FnOnce() -> Result<(), String>,
) -> Result<usize, String> {
    let db_path = data_root.join("workbuddy.db");
    if !db_path.try_exists().map_err(history_error)? {
        return apply_auth().map(|()| 0);
    }
    let mut conn = Connection::open_with_flags(&db_path, OpenFlags::SQLITE_OPEN_READ_WRITE)
        .map_err(history_error)?;
    conn.busy_timeout(Duration::from_millis(500))
        .map_err(history_error)?;
    let has_sessions: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='sessions')",
            [],
            |row| row.get(0),
        )
        .map_err(history_error)?;
    if !has_sessions {
        // Older clients may not have migrated their history into workbuddy.db yet.
        return apply_auth().map(|()| 0);
    }
    let pending: usize = conn
        .query_row(
            "SELECT COUNT(*) FROM sessions WHERE user_id != ?1 AND deleted_at IS NULL",
            [uid],
            |row| row.get(0),
        )
        .map_err(history_error)?;
    if pending == 0 {
        return apply_auth().map(|()| 0);
    }

    // A consistent snapshot includes WAL contents. Never copy the live DB file alone.
    let backup = data_root.join(format!(
        "{BACKUP_PREFIX}{}-{}",
        chrono::Utc::now().format("%Y%m%d-%H%M%S-%f"),
        uuid::Uuid::new_v4()
    ));
    conn.execute("VACUUM INTO ?1", [backup.to_string_lossy().as_ref()])
        .map_err(history_error)?;
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(history_error)?;
    let changed = tx
        .execute(
            "UPDATE sessions SET user_id = ?1 WHERE user_id != ?1 AND deleted_at IS NULL",
            [uid],
        )
        .map_err(history_error)?;
    apply_auth()?;
    tx.commit().map_err(history_error)?;
    prune_backups(data_root);
    Ok(changed)
}

fn prune_backups(data_root: &Path) {
    let Ok(entries) = fs::read_dir(data_root) else {
        return;
    };
    let mut backups: Vec<_> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_file()
                && path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with(BACKUP_PREFIX))
        })
        .collect();
    backups.sort();
    let obsolete = backups.len().saturating_sub(5);
    for path in backups.into_iter().take(obsolete) {
        let _ = fs::remove_file(path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    struct Fixture(std::path::PathBuf);
    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!("wb-history-{}", uuid::Uuid::new_v4()));
            fs::create_dir_all(&root).unwrap();
            Self(root)
        }
        fn db(&self) -> Connection {
            Connection::open(self.0.join("workbuddy.db")).unwrap()
        }
        fn seed(&self) {
            self.db()
                .execute_batch(
                    "CREATE TABLE sessions (
                        id TEXT PRIMARY KEY, user_id TEXT NOT NULL, deleted_at INTEGER,
                        title TEXT, updated_at INTEGER, extra TEXT
                    );
                    INSERT INTO sessions VALUES
                        ('a', 'alice', NULL, 'First', 10, 'unknown-field'),
                        ('b', 'bob', NULL, 'Second', 20, 'other-field'),
                        ('c', 'carol', NULL, 'Third', 30, 'third-field'),
                        ('deleted', 'alice', 40, 'Deleted', 40, 'deleted-field');
                    CREATE TABLE automations (user_id TEXT);
                    INSERT INTO automations VALUES ('alice');",
                )
                .unwrap();
        }
        fn visible(&self, uid: &str) -> Vec<String> {
            self.db()
                .prepare(
                    "SELECT id FROM sessions WHERE user_id=?1 AND deleted_at IS NULL ORDER BY id",
                )
                .unwrap()
                .query_map([uid], |row| row.get(0))
                .unwrap()
                .collect::<Result<_, _>>()
                .unwrap()
        }
        fn backups(&self) -> Vec<std::path::PathBuf> {
            fs::read_dir(&self.0)
                .unwrap()
                .map(|entry| entry.unwrap().path())
                .filter(|path| {
                    path.file_name()
                        .unwrap()
                        .to_string_lossy()
                        .starts_with(BACKUP_PREFIX)
                })
                .collect()
        }
        fn switch(&self, uid: &str) -> Result<(), String> {
            let auth_path = self.0.join("auth.info");
            let original = super::super::read(&auth_path).unwrap_or(json!({}));
            apply(
                &auth_path,
                &self.0,
                &original,
                &json!({"account":{"uid":uid}}),
            )
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }

    #[test]
    fn switching_shares_existing_ids_without_changing_content_or_cloud_mapping() {
        let fixture = Fixture::new();
        fixture.seed();
        let body = fixture.0.join("a.jsonl");
        let cloud = fixture.0.join("edge-sync-mapping-v4.db");
        let models = fixture.0.join("models.json");
        for path in [&body, &cloud, &models] {
            fs::write(path, b"unchanged fixture data").unwrap();
        }
        // Positive control: reproduce the client's account filter before the switch.
        assert_eq!(fixture.visible("bob"), ["b"]);
        fixture.switch("bob").unwrap();
        assert_eq!(fixture.visible("bob"), ["a", "b", "c"]);
        let backup = Connection::open(&fixture.backups()[0]).unwrap();
        let columns = "SELECT id, deleted_at, title, updated_at, extra FROM sessions ORDER BY id";
        let rows = |conn: &Connection| {
            conn.prepare(columns)
                .unwrap()
                .query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, Option<i64>>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, i64>(3)?,
                        row.get::<_, String>(4)?,
                    ))
                })
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap()
        };
        assert_eq!(rows(&fixture.db()), rows(&backup));
        assert_eq!(
            backup
                .query_row("SELECT user_id FROM sessions WHERE id='a'", [], |r| r
                    .get::<_, String>(0))
                .unwrap(),
            "alice"
        );
        assert_eq!(
            fixture
                .db()
                .query_row("SELECT user_id FROM sessions WHERE id='deleted'", [], |r| r
                    .get::<_, String>(0))
                .unwrap(),
            "alice"
        );
        assert_eq!(
            fixture
                .db()
                .query_row("SELECT user_id FROM automations", [], |r| r
                    .get::<_, String>(0))
                .unwrap(),
            "alice"
        );
        for path in [&body, &cloud, &models] {
            assert_eq!(fs::read(path).unwrap(), b"unchanged fixture data");
        }
        fixture.switch("bob").unwrap();
        assert_eq!(fixture.backups().len(), 1);
        fixture.switch("alice").unwrap();
        assert_eq!(fixture.visible("alice"), ["a", "b", "c"]);
    }

    #[test]
    fn editions_remain_separate_and_backups_include_wal_rows() {
        let cn = Fixture::new();
        let ai = Fixture::new();
        cn.seed();
        ai.seed();
        let conn = cn.db();
        conn.execute_batch(
            "PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0;
            INSERT INTO sessions VALUES ('wal', 'alice', NULL, 'In WAL', 50, 'wal-data');",
        )
        .unwrap();
        cn.switch("bob").unwrap();
        assert_eq!(cn.visible("bob"), ["a", "b", "c", "wal"]);
        assert_eq!(ai.visible("bob"), ["b"]);
        assert!(ai.backups().is_empty());
        let backup = Connection::open(&cn.backups()[0]).unwrap();
        assert_eq!(
            backup
                .query_row("SELECT user_id FROM sessions WHERE id='wal'", [], |r| {
                    r.get::<_, String>(0)
                })
                .unwrap(),
            "alice"
        );
        assert_eq!(
            backup
                .query_row("PRAGMA integrity_check", [], |r| r.get::<_, String>(0))
                .unwrap(),
            "ok"
        );
        ai.switch("carol").unwrap();
        assert_eq!(ai.visible("carol"), ["a", "b", "c"]);
        assert_eq!(cn.visible("bob"), ["a", "b", "c", "wal"]);
    }

    #[test]
    fn missing_or_uninitialized_database_does_not_create_history() {
        let fixture = Fixture::new();
        fixture.switch("bob").unwrap();
        assert!(!fixture.0.join("workbuddy.db").exists());
        fixture
            .db()
            .execute_batch("CREATE TABLE settings (value TEXT)")
            .unwrap();
        fixture.switch("alice").unwrap();
        assert!(fixture.backups().is_empty());
        assert_eq!(
            fixture
                .db()
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE name='sessions'",
                    [],
                    |r| r.get::<_, i64>(0)
                )
                .unwrap(),
            0
        );
    }

    #[test]
    fn auth_write_failure_rolls_back_history() {
        let fixture = Fixture::new();
        fixture.seed();
        fs::create_dir(fixture.0.join("auth.info")).unwrap();
        assert!(fixture.switch("bob").is_err());
        assert_eq!(fixture.visible("bob"), ["b"]);
        assert_eq!(fixture.visible("alice"), ["a"]);
    }

    #[test]
    fn commit_failure_restores_existing_or_missing_auth_and_history() {
        for existed in [true, false] {
            let fixture = Fixture::new();
            fixture.seed();
            let original = json!({"account":{"uid":"alice"}, "extra":"preserved"});
            let auth_path = fixture.0.join("auth.info");
            if existed {
                super::super::write(&auth_path, &original).unwrap();
            }
            // A rollback-journal reader permits backup/update, but blocks the commit.
            let reader = fixture.db();
            reader
                .execute_batch("BEGIN; SELECT * FROM sessions;")
                .unwrap();
            let error = fixture.switch("bob").unwrap_err();
            assert!(error.contains("locked"), "{error}");
            assert_eq!(
                fixture.backups().len(),
                1,
                "backup must have succeeded first"
            );
            if existed {
                assert_eq!(super::super::read(&auth_path).unwrap(), original);
            } else {
                assert!(!auth_path.exists());
            }
            reader.execute_batch("ROLLBACK").unwrap();
            assert_eq!(fixture.visible("bob"), ["b"]);
        }
    }

    #[test]
    fn unreadable_history_blocks_credentials_and_existing_writer_blocks_switch() {
        let fixture = Fixture::new();
        fs::write(fixture.0.join("workbuddy.db"), b"not sqlite").unwrap();
        assert!(fixture.switch("bob").is_err());
        assert!(!fixture.0.join("auth.info").exists());
        fs::remove_file(fixture.0.join("workbuddy.db")).unwrap();
        fixture.seed();
        let writer = fixture.db();
        writer.execute_batch("BEGIN IMMEDIATE").unwrap();
        assert!(fixture.switch("bob").is_err());
        assert!(!fixture.0.join("auth.info").exists());
        writer.execute_batch("ROLLBACK").unwrap();
        assert_eq!(fixture.visible("bob"), ["b"]);
    }

    #[test]
    fn successful_switches_keep_five_recent_backups_only() {
        let fixture = Fixture::new();
        fixture.seed();
        let unrelated = fixture.0.join("workbuddy.db.user-backup");
        fs::write(&unrelated, b"keep").unwrap();
        for uid in ["bob", "alice", "bob", "alice", "bob", "alice", "bob"] {
            fixture.switch(uid).unwrap();
        }
        assert_eq!(fixture.backups().len(), 5);
        assert_eq!(fs::read(unrelated).unwrap(), b"keep");
    }
}
