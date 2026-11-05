use super::*;
use serde_json::json;
use std::sync::{Mutex, OnceLock};

type Pending = Mutex<BTreeMap<(PathBuf, String), (Credentials, Credentials)>>;

fn pending() -> &'static Pending {
    static PENDING: OnceLock<Pending> = OnceLock::new();
    PENDING.get_or_init(|| Mutex::new(BTreeMap::new()))
}

pub(super) fn pair(credentials: &Credentials) -> String {
    format!(
        "{:x}",
        Sha256::digest(format!(
            "{}\0{}",
            credentials.access().unwrap_or(""),
            text(&credentials.token, "refresh_token").unwrap_or("")
        ))
    )
}

pub(super) fn forget_pending(path: &Path, id: &str) {
    if let Ok(mut pending) = pending().lock() {
        pending.remove(&(path.into(), id.into()));
    }
}

pub(super) fn list_at(path: &Path) -> Result<Vec<Account>, String> {
    Ok(load_at(path)?
        .into_iter()
        .filter(|(id, saved)| *id == saved.account.id)
        .map(|(_, saved)| saved.account)
        .collect())
}

fn valid(id: &str, credentials: &Credentials) -> Result<(), String> {
    credentials.access()?;
    credentials.region()?;
    if credentials.identity().as_deref() != Some(id) {
        return Err("accountError.invalidAccount".into());
    }
    if let Some(profile) = credentials.profile() {
        let parts = profile.split(':').collect::<Vec<_>>();
        if parts.len() != 6
            || parts[0] != "arn"
            || parts[2] != "codewhisperer"
            || !parts[5].starts_with("profile/")
            || parts[5].len() <= 8
        {
            return Err("accountError.invalidAccount".into());
        }
    }
    Ok(())
}

pub(super) fn current(path: &Path, id: &str) -> Result<(Saved, Credentials), String> {
    let store = load_at(path)?;
    let saved = store
        .get(id)
        .filter(|saved| saved.account.id == id)
        .cloned()
        .ok_or("accountError.cancelled")?;
    let mut credentials = unprotect(&saved)?;
    let mut pending = pending().lock().map_err(|_| "accountError.busy")?;
    if let Some((previous, incoming)) = pending.get(&(path.into(), id.into())) {
        if same_credentials(&credentials, previous) {
            credentials = incoming.clone();
        } else {
            pending.remove(&(path.into(), id.into()));
        }
    }
    valid(id, &credentials)?;
    Ok((saved, credentials))
}

pub(super) fn may_adopt_native(credentials: &Credentials, saved: &Saved, edition: Edition) -> bool {
    let Ok(previous) = unprotect(saved) else {
        return false;
    };
    if pair(credentials) == pair(&previous) {
        return true;
    }
    credentials.source_path.as_deref().is_some_and(|path| {
        saved.native_pairs.contains_key(&native::binding_key(
            credentials.source.unwrap_or(edition),
            path,
        )) && same_identity(credentials, &previous)
    })
}

fn same_identity(native: &Credentials, previous: &Credentials) -> bool {
    native.provider().eq_ignore_ascii_case(previous.provider())
        && native.identity().is_some()
        && native.identity() == previous.identity()
}

fn related(native: &Credentials, previous: &Credentials) -> bool {
    native.provider().eq_ignore_ascii_case(previous.provider())
        && (same_identity(native, previous)
            || text(&native.token, "refresh_token")
                .is_some_and(|value| Some(value) == text(&previous.token, "refresh_token")))
}

fn merge(native: &Credentials, previous: &Credentials) -> Credentials {
    let mut incoming = native.clone();
    incoming.subject = incoming.subject.or_else(|| previous.subject.clone());
    incoming.name = incoming.name.or_else(|| previous.name.clone());
    incoming
}

fn same_credentials(first: &Credentials, second: &Credentials) -> bool {
    first.token == second.token
        && first.registration == second.registration
        && first.subject == second.subject
        && first.name == second.name
}

async fn persist(
    path: &Path,
    id: &str,
    previous: &Credentials,
    incoming: &Credentials,
) -> Result<(), String> {
    let _guard = ACCOUNT_LOCK.lock().await;
    let mut store = load_at(path)?;
    let saved = store.get_mut(id).ok_or("accountError.cancelled")?;
    let on_disk = unprotect(saved)?;
    if !same_credentials(&on_disk, previous) {
        return Err("accountError.cancelled".into());
    }
    // Retain a consumed refresh response before encryption or file replacement can fail.
    pending()
        .lock()
        .map_err(|_| "accountError.busy")?
        .insert((path.into(), id.into()), (on_disk, incoming.clone()));
    saved.credential = protect(incoming)?;
    save_at(path, &store)?;
    forget_pending(path, id);
    Ok(())
}

struct Owner {
    session: native::Session,
    baseline: String,
}

async fn owners(
    path: &Path,
    id: &str,
    credentials: &mut Credentials,
    targets: &[(Edition, PathBuf)],
) -> Result<Vec<Owner>, String> {
    let mut owners = Vec::new();
    for (edition, target) in targets {
        let (saved, previous) = {
            let _guard = ACCOUNT_LOCK.lock().await;
            current(path, id)?
        };
        let bound = saved
            .native_pairs
            .contains_key(&native::binding_key(*edition, target));
        let session = match native::Session::open(*edition, target) {
            Ok(Some(session)) => session,
            Ok(None) => continue,
            Err(error) if bound => return Err(error),
            Err(_) => continue,
        };
        let incoming = match session.read() {
            Ok(Some(incoming)) => incoming,
            Ok(None) => continue,
            Err(error) if bound => return Err(error),
            Err(_) => continue,
        };
        let own_pair = pair(&previous);
        let native_pair = pair(&incoming);
        let binding = saved.native_pairs.get(&session.key());
        if native_pair != own_pair && binding.is_none() {
            continue;
        }
        if native_pair != own_pair
            && binding != Some(&native_pair)
            && !related(&incoming, &previous)
        {
            if bound && incoming.identity().is_none() {
                return Err("accountError.loginRequired".into());
            }
            continue;
        }
        if binding != Some(&native_pair) || native_pair == own_pair {
            let candidate = merge(&incoming, &previous);
            valid(id, &candidate)?;
            if !same_credentials(&candidate, &previous) {
                let disk = {
                    let _guard = ACCOUNT_LOCK.lock().await;
                    unprotect(load_at(path)?.get(id).ok_or("accountError.cancelled")?)?
                };
                persist(path, id, &disk, &candidate).await?;
            }
            *credentials = candidate;
        } else {
            *credentials = previous;
        }
        let _guard = ACCOUNT_LOCK.lock().await;
        let mut store = load_at(path)?;
        let key = session.key();
        let mut changed = false;
        for (other_id, other) in &mut store {
            if other_id != id {
                changed |= other.native_pairs.remove(&key).is_some();
            }
        }
        let saved = store.get_mut(id).ok_or("accountError.cancelled")?;
        if saved.native_pairs.get(&session.key()) != Some(&native_pair) {
            saved
                .native_pairs
                .insert(session.key(), native_pair.clone());
            changed = true;
        }
        if changed {
            save_at(path, &store)?;
        }
        owners.push(Owner {
            session,
            baseline: native_pair,
        });
    }
    Ok(owners)
}

async fn synchronize(
    path: &Path,
    id: &str,
    credentials: &Credentials,
    owners: &mut [Owner],
) -> Result<(), String> {
    for owner in owners {
        let Some(native) = owner.session.read()? else {
            return Err("accountError.cancelled".into());
        };
        if pair(&native) != owner.baseline {
            return Err("accountError.cancelled".into());
        }
        if pair(credentials) != owner.baseline {
            owner.session.write(credentials, false)?;
        }
        let _guard = ACCOUNT_LOCK.lock().await;
        let mut store = load_at(path)?;
        let saved = store.get_mut(id).ok_or("accountError.cancelled")?;
        if pair(&unprotect(saved)?) != pair(credentials) {
            return Err("accountError.cancelled".into());
        }
        if saved.native_pairs.get(&owner.session.key()) != Some(&pair(credentials)) {
            saved
                .native_pairs
                .insert(owner.session.key(), pair(credentials));
            save_at(path, &store)?;
        }
    }
    Ok(())
}

pub(super) async fn capture_outgoing(
    path: &Path,
    session: &mut native::Session,
) -> Result<(), String> {
    let Some(incoming) = session.read()? else {
        return Ok(());
    };
    let rows = {
        let _guard = ACCOUNT_LOCK.lock().await;
        load_at(path)?
    };
    let mut known = false;
    let bound = rows
        .values()
        .any(|saved| saved.native_pairs.contains_key(&session.key()));
    let incoming_pair = pair(&incoming);
    for (id, saved) in rows {
        if incoming.identity().is_some_and(|identity| identity != id) {
            continue;
        }
        let previous = unprotect(&saved)?;
        let binding = saved.native_pairs.get(&session.key());
        if incoming_pair != pair(&previous)
            && binding != Some(&incoming_pair)
            && (binding.is_none() || !related(&incoming, &previous))
        {
            continue;
        }
        known = true;
        let (_, current) = {
            let _guard = ACCOUNT_LOCK.lock().await;
            current(path, &id)?
        };
        if binding == Some(&incoming_pair) && pair(&current) != incoming_pair {
            if !same_credentials(&previous, &current) {
                persist(path, &id, &previous, &current).await?;
            }
            continue;
        }
        let candidate = merge(&incoming, &previous);
        valid(&id, &candidate)?;
        if !same_credentials(&candidate, &previous) {
            persist(path, &id, &previous, &candidate).await?;
        }
    }
    if !known && incoming.identity().is_none() && bound {
        return Err("accountError.invalidAccount".into());
    }
    Ok(())
}

pub(super) async fn apply_at(
    path: &Path,
    id: &str,
    credentials: &Credentials,
    session: &mut native::Session,
) -> Result<(), String> {
    capture_outgoing(path, session).await?;
    let binding = session.key();
    let apply = session.begin_apply(credentials)?;
    let _guard = ACCOUNT_LOCK.lock().await;
    let mut store = load_at(path)?;
    let saved = store.get_mut(id).ok_or("accountError.cancelled")?;
    if !same_credentials(&unprotect(saved)?, credentials) {
        return Err("accountError.cancelled".into());
    }
    saved
        .native_pairs
        .insert(binding.clone(), pair(credentials));
    for (other_id, other) in &mut store {
        if other_id != id {
            other.native_pairs.remove(&binding);
        }
    }
    #[cfg(test)]
    if tests::failed_writes().lock().unwrap().contains(path) {
        return Err("accountError.write".into());
    }
    let mut transaction = super::super::native_account_files::Transaction::new();
    transaction.replace(
        path,
        &serde_json::to_vec_pretty(&store).map_err(|_| "accountError.format")?,
    )?;
    apply.commit()?;
    transaction.commit();
    Ok(())
}

pub(super) async fn obtain_locked(
    path: &Path,
    id: &str,
    force: bool,
    rejected: Option<&Value>,
    targets: &[(Edition, PathBuf)],
    running: bool,
    endpoint: Option<&str>,
) -> Result<Credentials, String> {
    let (_, mut credentials) = {
        let _guard = ACCOUNT_LOCK.lock().await;
        current(path, id)?
    };
    let mut owners = owners(path, id, &mut credentials, targets).await?;
    let rejected_current = rejected.is_none_or(|rejected| {
        rejected["credentialGeneration"].as_str().map_or_else(
            || rejected["accessToken"].as_str() == credentials.access().ok(),
            |generation| generation == pair(&credentials),
        )
    });
    let due = credentials
        .expiry()
        .is_none_or(|expiry| expiry <= chrono::Utc::now().timestamp() + 120);
    if (force && rejected_current) || due {
        if running && !owners.is_empty() {
            if force
                || credentials
                    .expiry()
                    .is_none_or(|expiry| expiry <= chrono::Utc::now().timestamp() + 15)
            {
                return Err("accountError.closeClient".into());
            }
        } else {
            let disk = {
                let _guard = ACCOUNT_LOCK.lock().await;
                unprotect(load_at(path)?.get(id).ok_or("accountError.cancelled")?)?
            };
            if !force && endpoint.is_none() {
                oauth::ensure_fresh(&mut credentials).await?;
            } else {
                oauth::refresh_at(&mut credentials, endpoint).await?;
            }
            valid(id, &credentials)?;
            persist(path, id, &disk, &credentials).await?;
        }
    }
    // Heal a previous failed own-store save without consuming the grant again.
    let disk = {
        let _guard = ACCOUNT_LOCK.lock().await;
        unprotect(load_at(path)?.get(id).ok_or("accountError.cancelled")?)?
    };
    if !same_credentials(&disk, &credentials) {
        persist(path, id, &disk, &credentials).await?;
    }
    synchronize(path, id, &credentials, &mut owners).await?;
    Ok(credentials)
}

pub(super) async fn running(
    path: &Path,
    id: &str,
    targets: &[(Edition, PathBuf)],
) -> Result<bool, String> {
    let (saved, previous) = {
        let _guard = ACCOUNT_LOCK.lock().await;
        current(path, id)?
    };
    for edition in [Edition::Crew, Edition::Cli] {
        if !installed(edition) {
            continue;
        }
        let Some(credentials) = native::read(edition).ok().flatten() else {
            continue;
        };
        if credentials.source_path.as_ref().is_some_and(|path| {
            let native_pair = pair(&credentials);
            let binding = saved.native_pairs.get(&native::binding_key(
                credentials.source.unwrap_or(edition),
                path,
            ));
            targets.iter().any(|(_, target)| target == path)
                && (native_pair == pair(&previous)
                    || binding == Some(&native_pair)
                    || (binding.is_some() && related(&credentials, &previous)))
        }) && super::super::process_manager::desktop_tool_is_running(edition.tool()).await?
        {
            return Ok(true);
        }
    }
    Ok(false)
}

pub(super) fn value(id: &str, credentials: &Credentials) -> Result<Value, String> {
    valid(id, credentials)?;
    let mut value = json!({"accessToken":credentials.access()?, "accountId":id, "region":credentials.region()?, "credentialGeneration":pair(credentials), "provider":credentials.provider()});
    if let Some(profile) = credentials.profile() {
        value["profileArn"] = json!(profile);
    }
    if credentials.provider().eq_ignore_ascii_case("ExternalIdp") {
        value["tokenType"] = json!("EXTERNAL_IDP");
    }
    Ok(value)
}

pub(super) async fn credentials(
    id: &str,
    force: bool,
    rejected: Option<&Value>,
) -> Result<Value, String> {
    let id = id.to_owned();
    let rejected = rejected.cloned();
    // Keep renewal and persistence alive after the router client disconnects.
    complete(async move {
        let _request = REQUEST_LOCK.lock().await;
        let path = store_path()?;
        let targets = native::targets()?;
        let running = running(&path, &id, &targets).await?;
        let credentials = obtain_locked(
            &path,
            &id,
            force,
            rejected.as_ref(),
            &targets,
            running,
            None,
        )
        .await?;
        value(&id, &credentials)
    })
    .await
}

pub(super) async fn complete<T: Send + 'static>(
    task: impl std::future::Future<Output = Result<T, String>> + Send + 'static,
) -> Result<T, String> {
    tokio::spawn(task)
        .await
        .map_err(|_| "accountError.network".to_string())?
}

#[cfg(test)]
#[path = "proxy_tests.rs"]
pub(super) mod tests;
