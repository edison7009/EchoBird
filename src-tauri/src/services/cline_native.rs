use super::{
    cline_accounts::{self, Account, Credentials},
    native_account_files::{self as files, Transaction},
    tool_config_manager::cline::{self, desktop::Storage},
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

fn config_path() -> Result<PathBuf, String> {
    resolved_path(&cline::config_path())
}
fn resolved_path(path: &Path) -> Result<PathBuf, String> {
    // Match the native runtime's lexical path.resolve, without adding Windows' verbatim prefix.
    files::absolute_path(&if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|_| "accountError.home")?
            .join(path)
    })
}
pub(crate) fn existing_installed_config_path() -> Result<Option<PathBuf>, String> {
    if super::tool_manager::get_tool_exe_path("cline").is_none()
        && super::tool_manager::get_tool_exe_path("clinedesktop").is_none()
    {
        return Ok(None);
    }
    let path = config_path()?;
    Ok(path.is_file().then_some(path))
}
fn trusted(settings: &Value) -> Result<(), String> {
    if !settings.is_object() {
        return Err("accountError.format".into());
    }
    if let Some(base) = settings.get("baseUrl") {
        let url = base
            .as_str()
            .and_then(|base| url::Url::parse(base).ok())
            .ok_or("accountError.invalidAccount")?;
        if url.scheme() != "https"
            || url.host_str() != Some("api.cline.bot")
            || !matches!(url.path().trim_end_matches('/'), "" | "/api/v1")
            || url.username() != ""
            || url.password().is_some()
            || url.port().is_some_and(|port| port != 443)
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err("accountError.invalidAccount".into());
        }
    }
    if settings.get("protocol").is_some_and(|p| p != "openai-chat")
        || settings
            .get("client")
            .is_some_and(|c| c != "openai-compatible")
        || settings
            .get("routingProviderId")
            .is_some_and(|p| p != "cline" && p != "cline-pass")
    {
        return Err("accountError.invalidAccount".into());
    }
    Ok(())
}
fn native_credentials(auth: &Value, previous: &Credentials) -> Result<Credentials, String> {
    if auth["accountId"] != previous.user_id
        || auth["metadata"]["userInfo"]
            .get("clineUserId")
            .is_some_and(|id| id != &previous.user_id)
    {
        return Err("accountError.invalidAccount".into());
    }
    let mut c = previous.clone();
    c.access_token = cline_accounts::raw_token(
        auth["accessToken"]
            .as_str()
            .filter(|s| !s.trim().is_empty())
            .ok_or("accountError.invalidAccount")?,
    )
    .into();
    c.refresh_token = auth["refreshToken"]
        .as_str()
        .filter(|s| !s.trim().is_empty())
        .ok_or("accountError.invalidAccount")?
        .into();
    c.expires_at = auth["expiresAt"]
        .as_i64()
        .filter(|time| *time > 0)
        .or_else(|| {
            super::cursor_auth::claims(&c.access_token)
                .ok()?
                .get("exp")?
                .as_i64()?
                .checked_mul(1000)
        })
        .unwrap_or(1);
    if let Some(info) = auth["metadata"]
        .get("userInfo")
        .filter(|info| info.is_object())
    {
        if info["subject"]
            .as_str()
            .zip(previous.user_info["subject"].as_str())
            .is_some_and(|(native, saved)| native != saved)
        {
            return Err("accountError.invalidAccount".into());
        }
        c.user_info = info.clone();
        if let Some(email) = info["email"].as_str() {
            c.email = email.into();
        }
        if let Some(name) = info["name"].as_str() {
            c.name = Some(name.into());
        }
    }
    Ok(c)
}
fn auth(c: &Credentials) -> Value {
    json!({"accessToken":cline_accounts::bearer(c),"refreshToken":c.refresh_token,
        "accountId":c.user_id,"expiresAt":c.expires_at,"metadata":{"userInfo":c.user_info}})
}

pub(crate) struct RefreshSession {
    path: PathBuf,
    _db: rusqlite::Connection,
    bindings: Vec<(String, Value)>,
}
impl RefreshSession {
    pub(crate) async fn open_at(path: &Path) -> Result<Self, String> {
        let path = resolved_path(path)?;
        files::private_dir(path.parent().ok_or("accountError.home")?)?;
        let lock = PathBuf::from(format!(
            "{}.oauth-{:x}.lock",
            path.display(),
            Sha256::digest(b"cline")
        ));
        let db = rusqlite::Connection::open(lock).map_err(|_| "accountError.busy")?;
        db.execute_batch("PRAGMA busy_timeout = 0;")
            .map_err(|_| "accountError.busy")?;
        let deadline = tokio::time::Instant::now() + Duration::from_secs(60);
        loop {
            match db.execute_batch("BEGIN EXCLUSIVE;") {
                Ok(()) => break,
                Err(error)
                    if error.sqlite_error().is_some_and(|e| {
                        matches!(
                            e.code,
                            rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked
                        )
                    }) =>
                {
                    if tokio::time::Instant::now() >= deadline {
                        return Err("accountError.busy".into());
                    }
                    tokio::time::sleep(Duration::from_millis(25)).await;
                }
                Err(_) => return Err("accountError.busy".into()),
            }
        }
        Ok(Self {
            path,
            _db: db,
            bindings: vec![],
        })
    }
    pub(crate) fn credentials(
        &mut self,
        previous: &Credentials,
        baseline: Option<&str>,
    ) -> Result<Option<Credentials>, String> {
        let state = cline::load(&self.path)?;
        self.bindings.clear();
        let mut incoming = None;
        let providers = if state["lastUsedProvider"] == "cline-pass" {
            ["cline-pass", "cline"]
        } else {
            ["cline", "cline-pass"]
        };
        for provider in providers {
            let settings = &state["providers"][provider]["settings"];
            let auth = &settings["auth"];
            if auth["accountId"] != previous.user_id {
                continue;
            }
            trusted(settings)?;
            let c = native_credentials(auth, previous)?;
            let native_pair = cline_accounts::pair(&c);
            if baseline.is_none() && native_pair != cline_accounts::pair(previous) {
                continue;
            }
            if let Some(selected) = &incoming {
                if cline_accounts::pair(selected) != native_pair {
                    continue;
                }
            } else {
                incoming = Some(c);
            }
            self.bindings.push((provider.into(), auth.clone()));
        }
        Ok(incoming.map(|incoming| {
            if baseline == Some(cline_accounts::pair(&incoming).as_str())
                && cline_accounts::pair(previous) != cline_accounts::pair(&incoming)
            {
                previous.clone()
            } else {
                incoming
            }
        }))
    }
    pub(crate) fn synchronize(&mut self, c: &Credentials) -> Result<bool, String> {
        if self.bindings.is_empty() {
            return Ok(false);
        }
        let expected = auth(c);
        if self.bindings.iter().all(|(_, observed)| {
            ["accessToken", "refreshToken", "accountId", "expiresAt"]
                .iter()
                .all(|key| observed[key] == expected[key])
                && observed["metadata"]["userInfo"] == c.user_info
        }) {
            return Ok(true);
        }
        let mut state = cline::load(&self.path)?;
        for (provider, observed) in &self.bindings {
            let settings = &mut state["providers"][provider]["settings"];
            trusted(settings)?;
            if settings["auth"] != *observed {
                return Err("accountError.invalidAccount".into());
            }
            let mut next = observed.clone();
            for key in ["accessToken", "refreshToken", "accountId", "expiresAt"] {
                next[key] = auth(c)[key].clone();
            }
            if !next["metadata"].is_object() {
                next["metadata"] = json!({});
            }
            next["metadata"]["userInfo"] = c.user_info.clone();
            settings["auth"] = next;
            state["providers"][provider]["updatedAt"] =
                json!(chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true));
            state["providers"][provider]["tokenSource"] = json!("oauth");
        }
        let mut transaction = Transaction::new();
        transaction.replace(&self.path, &files::json(&state)?)?;
        transaction.commit();
        self.credentials(c, Some(&cline_accounts::pair(c)))?;
        Ok(true)
    }
}
impl Drop for RefreshSession {
    fn drop(&mut self) {
        let _ = self._db.execute_batch("ROLLBACK;");
    }
}

pub(crate) struct AccountApply {
    transaction: Option<Transaction>,
    storage: Option<Storage>,
    original_selection: Option<Value>,
    committed: bool,
}
impl AccountApply {
    pub(crate) fn commit(mut self) {
        if let Some(transaction) = self.transaction.take() {
            transaction.commit();
        }
        self.committed = true;
    }
}
impl Drop for AccountApply {
    fn drop(&mut self) {
        if !self.committed {
            if let (Some(storage), Some(selection)) = (&mut self.storage, &self.original_selection)
            {
                if storage.write(selection).is_err() {
                    log::error!("Failed to restore native account selection");
                }
            }
        }
    }
}
fn account_state(mut state: Value, c: &Credentials) -> Result<Value, String> {
    for provider in ["cline", "cline-pass"] {
        if provider == "cline-pass" && !state["providers"][provider]["settings"]["auth"].is_object()
        {
            continue;
        }
        let entry = &mut state["providers"][provider];
        if entry.is_null() {
            *entry = json!({"settings":{"provider":provider}});
        }
        let settings = &mut entry["settings"];
        trusted(settings)?;
        settings["provider"] = json!(provider);
        settings["auth"] = auth(c);
        entry["updatedAt"] =
            json!(chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true));
        entry["tokenSource"] = json!("oauth");
    }
    if state["lastUsedProvider"] != "cline-pass" {
        state["lastUsedProvider"] = json!("cline");
    }
    Ok(state)
}
fn begin_apply(
    session: &RefreshSession,
    c: &Credentials,
    mut storage: Option<Storage>,
) -> Result<AccountApply, String> {
    let state = account_state(cline::load(&session.path)?, c)?;
    let original_selection = match &mut storage {
        Some(storage) => Some(storage.read()?),
        None => None,
    };
    let mut apply = AccountApply {
        transaction: Some(Transaction::new()),
        storage,
        original_selection,
        committed: false,
    };
    apply
        .transaction
        .as_mut()
        .unwrap()
        .replace(&session.path, &files::json(&state)?)?;
    if let (Some(storage), Some(original)) = (&mut apply.storage, &apply.original_selection) {
        let mut selected = original.clone();
        if !selected.is_object() || !selected["lastModelByProvider"].is_object() {
            return Err("accountError.format".into());
        }
        if selected["lastProvider"] != "cline-pass" {
            selected["lastProvider"] = json!("cline");
        }
        storage.write(&selected)?;
    }
    Ok(apply)
}
pub(crate) async fn begin_apply_at(
    store_path: &Path,
    session: &mut RefreshSession,
    c: &Credentials,
    storage: Option<Storage>,
) -> Result<AccountApply, String> {
    cline_accounts::capture_native_at(store_path, session).await?;
    begin_apply(session, c, storage)
}
pub(crate) async fn apply(id: &str, tool: &str) -> Result<Account, String> {
    if !matches!(tool, "cline" | "clinedesktop") {
        return Err("accountError.invalidAccount".into());
    }
    if super::tool_manager::get_tool_exe_path(tool).is_none() {
        return Err("accountError.unavailable".into());
    }
    let sync_desktop =
        tool == "clinedesktop" || super::tool_manager::get_tool_exe_path("clinedesktop").is_some();
    cline_accounts::saved_credentials(id).await?;
    let state = cline::load(&config_path()?)?;
    for provider in ["cline", "cline-pass"] {
        if state["providers"][provider]["settings"].is_object() {
            trusted(&state["providers"][provider]["settings"])?;
        }
    }
    if sync_desktop {
        cline::desktop::read()?;
        super::process_manager::stop_desktop_for_account("clinedesktop").await?;
    }
    let storage = if sync_desktop {
        Some(Storage::open()?)
    } else {
        None
    };
    let mut session = cline_accounts::credentials_for_native(id).await?;
    if session.native.is_none() {
        session.native = Some(RefreshSession::open_at(&config_path()?).await?);
    }
    let native = session.native.as_mut().ok_or("accountError.unavailable")?;
    let transaction = begin_apply_at(
        &cline_accounts::store_path()?,
        native,
        &session.credentials,
        storage,
    )
    .await?;
    let account = cline_accounts::mark_active(id, &session.credentials).await?;
    transaction.commit();
    Ok(account)
}

pub(crate) fn discover() -> Result<Vec<Credentials>, String> {
    let Some(path) = existing_installed_config_path()? else {
        return Ok(vec![]);
    };
    let state = cline::load(&path)?;
    let mut found = vec![];
    for provider in ["cline", "cline-pass"] {
        let settings = &state["providers"][provider]["settings"];
        let auth = &settings["auth"];
        if !auth.is_object() {
            continue;
        }
        trusted(settings)?;
        let user_id = auth["accountId"]
            .as_str()
            .filter(|s| !s.is_empty())
            .ok_or("accountError.invalidAccount")?;
        let info = &auth["metadata"]["userInfo"];
        let previous = Credentials {
            access_token: String::new(),
            refresh_token: String::new(),
            expires_at: 1,
            user_id: user_id.into(),
            email: info["email"].as_str().unwrap_or(user_id).into(),
            name: info["name"].as_str().map(str::to_owned),
            user_info: info.clone(),
        };
        found.push(native_credentials(auth, &previous)?);
    }
    Ok(found)
}

#[cfg(test)]
mod tests;
