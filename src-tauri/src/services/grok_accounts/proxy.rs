use super::*;
use std::collections::BTreeMap;

type Locks = Mutex<BTreeMap<PathBuf, Arc<tokio::sync::Mutex<()>>>>;
type PendingRotations = Mutex<BTreeMap<PathBuf, (String, Saved)>>;

fn pending() -> &'static PendingRotations {
    static PENDING: OnceLock<PendingRotations> = OnceLock::new();
    PENDING.get_or_init(|| Mutex::new(BTreeMap::new()))
}

pub(super) fn forget_pending(path: &Path) {
    if let Ok(mut pending) = pending().lock() {
        pending.remove(path);
    }
}

pub(super) fn account_lock(path: &Path) -> Result<Arc<tokio::sync::Mutex<()>>, String> {
    static LOCKS: OnceLock<Locks> = OnceLock::new();
    Ok(LOCKS
        .get_or_init(|| Mutex::new(BTreeMap::new()))
        .lock()
        .map_err(|_| "accountError.busy")?
        .entry(path.into())
        .or_default()
        .clone())
}

pub(super) fn current(path: &Path) -> Result<Saved, String> {
    let mut saved: Saved = super::super::cursor_auth::read(path)?;
    let mut pending = pending().lock().map_err(|_| "accountError.busy")?;
    if let Some((original, renewed)) = pending.get(path) {
        if storage::pair(&saved.auth) == *original {
            saved.auth = renewed.auth.clone();
            saved.native_pair = renewed.native_pair.clone();
            saved.summary.active = renewed.summary.active;
        } else {
            pending.remove(path);
        }
    }
    Ok(saved)
}

pub(super) fn valid(saved: &Saved, id: &str) -> Result<(), String> {
    let identity = summary(
        &saved.key,
        &serde_json::json!({saved.key.clone():saved.auth}),
        false,
    )?;
    let legacy = format!("{:x}", Sha256::digest(&saved.key));
    if saved.summary.id != id
        || (id != identity.id && id != legacy)
        || saved.auth["oidc_issuer"] != "https://auth.x.ai"
        || saved.auth["user_id"]
            .as_str()
            .is_none_or(|id| id.trim().is_empty())
        || saved.auth["key"]
            .as_str()
            .is_none_or(|key| key.trim().is_empty())
    {
        return Err("accountError.invalidAccount".into());
    }
    Ok(())
}

fn persist(path: &Path, previous: &Saved, incoming: &Saved) -> Result<Saved, String> {
    let _store = ACCOUNT_LOCK.lock().map_err(|_| "accountError.busy")?;
    let current = current(path)?;
    if current.auth != previous.auth {
        return Ok(current);
    }
    let on_disk: Saved = super::super::cursor_auth::read(path)?;
    let mut incoming = incoming.clone();
    incoming.summary.plan = on_disk.summary.plan;
    pending().lock().map_err(|_| "accountError.busy")?.insert(
        path.into(),
        (storage::pair(&on_disk.auth), incoming.clone()),
    );
    if write(path, &incoming).is_ok() {
        forget_pending(path);
    }
    Ok(incoming)
}

fn write(path: &Path, value: &impl serde::Serialize) -> Result<(), String> {
    #[cfg(test)]
    if tests::failed_writes().lock().unwrap().contains(path) {
        return Err("accountError.write".into());
    }
    super::super::cursor_auth::write(path, value)
}

pub(super) struct Session {
    path: PathBuf,
    pub(super) saved: Saved,
    native: Option<PathBuf>,
    native_lock: Option<storage::NativeLock>,
    _action: tokio::sync::OwnedMutexGuard<()>,
}

impl Session {
    pub(super) async fn open(path: &Path, native: Option<&Path>) -> Result<Self, String> {
        let action = account_lock(path)?.lock_owned().await;
        let id = path
            .file_stem()
            .and_then(|id| id.to_str())
            .ok_or("accountError.invalidAccount")?;
        {
            let _store = ACCOUNT_LOCK.lock().map_err(|_| "accountError.busy")?;
            valid(&current(path)?, id)?;
        }
        let native_lock = match native {
            Some(path) => storage::NativeLock::acquire(path, false).await?,
            None => None,
        };
        let saved = {
            let _store = ACCOUNT_LOCK.lock().map_err(|_| "accountError.busy")?;
            current(path)?
        };
        valid(&saved, id)?;
        let mut session = Self {
            path: path.into(),
            saved,
            native: native_lock.as_ref().and(native.map(Path::to_path_buf)),
            native_lock,
            _action: action,
        };
        session.reconcile()?;
        if session.saved.native_pair.is_none() {
            session.native = None;
            session.native_lock = None;
        }
        let needs_save = pending()
            .lock()
            .map_err(|_| "accountError.busy")?
            .contains_key(path);
        if needs_save {
            session.saved = persist(path, &session.saved, &session.saved)?;
        }
        Ok(session)
    }

    fn reconcile(&mut self) -> Result<(), String> {
        let Some(path) = &self.native else {
            return Ok(());
        };
        self.native_lock
            .as_ref()
            .ok_or("accountError.busy")?
            .ensure_current()?;
        let native: Value = super::super::cursor_auth::read(path)?;
        let row = &native[&self.saved.key];
        let previous = self.saved.clone();
        let native_pair = storage::pair(row);
        let own_pair = storage::pair(&self.saved.auth);
        if !storage::valid_native(&self.saved, row) {
            if self.saved.native_pair.is_some() || self.saved.summary.active {
                self.saved.native_pair = None;
                self.saved.summary.active = false;
                self.saved = persist(&self.path, &previous, &self.saved)?;
            }
            return Ok(());
        }
        // An isolated login is not replaced by another login for the same user.
        // Legacy applied snapshots recorded active without a native pair binding.
        if self.saved.native_pair.is_none() && !self.saved.summary.active && native_pair != own_pair
        {
            return Ok(());
        }
        if self.saved.native_pair.as_ref() == Some(&native_pair) && native_pair != own_pair {
            self.synchronize()?;
            return Ok(());
        }
        self.saved.auth = row.clone();
        self.saved.native_pair = Some(native_pair);
        self.saved.summary.active = true;
        if self.saved.auth != previous.auth
            || self.saved.native_pair != previous.native_pair
            || self.saved.summary.active != previous.summary.active
        {
            self.saved = persist(&self.path, &previous, &self.saved)?;
        }
        Ok(())
    }

    fn synchronize(&mut self) -> Result<(), String> {
        let (Some(path), Some(baseline)) = (&self.native, &self.saved.native_pair) else {
            return Ok(());
        };
        self.native_lock
            .as_ref()
            .ok_or("accountError.busy")?
            .ensure_current()?;
        let mut native: Value = super::super::cursor_auth::read(path)?;
        let row = &native[&self.saved.key];
        if !storage::same_identity(&self.saved, row) || storage::pair(row) != *baseline {
            return Err("accountError.cancelled".into());
        }
        native[&self.saved.key] = self.saved.auth.clone();
        write(&storage::native_target(path)?, &native)?;
        let previous = self.saved.clone();
        self.saved.native_pair = Some(storage::pair(&self.saved.auth));
        self.saved = persist(&self.path, &previous, &self.saved)?;
        Ok(())
    }

    pub(super) async fn renew(
        &mut self,
        rejected: Option<&str>,
        token_url: &str,
    ) -> Result<(), String> {
        if let Some(lock) = &self.native_lock {
            lock.ensure_current()?;
        }
        let current = {
            let _store = ACCOUNT_LOCK.lock().map_err(|_| "accountError.busy")?;
            current(&self.path)?
        };
        if rejected.is_some_and(|token| current.auth["key"] != token) {
            self.saved = current;
            return Ok(());
        }
        let mut renewed = current.clone();
        renewed.auth = renew_auth(&client()?, token_url, &current.auth).await?;
        self.saved = persist(&self.path, &current, &renewed)?;
        self.synchronize()?;
        Ok(())
    }
}

pub(super) fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| "accountError.network".into())
}

fn expires(auth: &Value) -> Option<i64> {
    auth["expires_at"]
        .as_str()
        .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
        .map(|date| date.timestamp())
        .or_else(|| {
            super::super::cursor_auth::claims(auth["key"].as_str()?)
                .ok()?
                .get("exp")?
                .as_i64()
        })
}

fn value(saved: &Saved) -> Value {
    serde_json::json!({"accessToken":saved.auth["key"],"userId":saved.auth["user_id"],
        "accountId":saved.summary.id,"principalType":saved.auth["principal_type"],
        "principalId":saved.auth["principal_id"],"authScope":saved.key,
        "credentialGeneration":storage::pair(&saved.auth)})
}

pub(crate) async fn credentials(id: &str, force: bool) -> Result<Value, String> {
    let id = id.to_owned();
    complete(async move { credentials_at(&id, force, None, None).await }).await
}
pub(crate) async fn after_rejection(id: &str, rejected: &Value) -> Result<Value, String> {
    let token = rejected["accessToken"]
        .as_str()
        .filter(|token| !token.is_empty())
        .ok_or("accountError.invalidAccount")?
        .to_owned();
    let generation = rejected["credentialGeneration"].as_str().map(str::to_owned);
    let id = id.to_owned();
    complete(async move { credentials_at(&id, true, Some(&token), generation.as_deref()).await })
        .await
}

pub(super) async fn complete<T: Send + 'static>(
    task: impl std::future::Future<Output = Result<T, String>> + Send + 'static,
) -> Result<T, String> {
    // A consumed refresh token must be saved even if the requesting client disconnects.
    tokio::spawn(task)
        .await
        .map_err(|_| "accountError.network".to_owned())?
}
async fn credentials_at(
    id: &str,
    force: bool,
    rejected: Option<&str>,
    generation: Option<&str>,
) -> Result<Value, String> {
    let path = saved_path(id)?;
    let observed = current(&path)?;
    let native =
        super::super::tool_manager::get_tool_exe_path("grok").and_then(|_| auth_path().ok());
    obtain(
        &path,
        native.as_deref(),
        &observed,
        force,
        rejected,
        generation,
        TOKEN_URL,
    )
    .await
}
async fn obtain(
    path: &Path,
    native: Option<&Path>,
    observed: &Saved,
    force: bool,
    rejected: Option<&str>,
    generation: Option<&str>,
    token_url: &str,
) -> Result<Value, String> {
    let mut session = Session::open(path, native).await?;
    let id = path
        .file_stem()
        .and_then(|id| id.to_str())
        .ok_or("accountError.invalidAccount")?;
    valid(&session.saved, id)?;
    let rejected = rejected.or_else(|| observed.auth["key"].as_str());
    let current_pair = storage::pair(&session.saved.auth);
    let unchanged = generation.map_or_else(
        || storage::pair(&observed.auth) == current_pair,
        |generation| generation == current_pair,
    );
    if unchanged
        && (expires(&session.saved.auth)
            .is_some_and(|time| time <= chrono::Utc::now().timestamp() + 30)
            || (force && rejected.is_some_and(|token| session.saved.auth["key"] == token)))
    {
        session.renew(rejected, token_url).await?;
    }
    valid(&session.saved, id)?;
    Ok(value(&session.saved))
}

#[cfg(test)]
#[path = "proxy_tests.rs"]
mod tests;
