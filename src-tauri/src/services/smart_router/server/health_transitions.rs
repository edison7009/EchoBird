use crate::services::model_manager;
// Router responsibility; behavior is covered by the server regression suite.
use super::failure_policy::{classify_failure, cooldown_for_failure, retry_after};
use super::health_selection::{
    account_health_key, applicable_account_health, candidate_health, is_cooling_down, resource_key,
    source_health_key,
};
use super::memory_store::{
    persist_memory_at, persist_route_memory, route_memory_path, shared_route_memory,
};
use super::{
    now_ms, proxy_error_from_chain, AppState, AttemptHealth, Candidate, CandidateHealth,
    FailureClass, RouteMemory,
};
use crate::models::model::AccountProvider;
use axum::http::header;
use axum::http::HeaderMap;
use axum::http::StatusCode;
use serde_json::json;
use serde_json::Value;
use sha2::Digest;
use sha2::Sha256;
use std::collections::HashSet;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

pub(super) fn mark_stream_failure(
    state: &AppState,
    candidate: &Candidate,
    error: &(dyn std::error::Error + 'static),
) -> Option<FailureClass> {
    if let Some(proxy) = proxy_error_from_chain(error) {
        if let Some(source) = &candidate.account_source {
            if (matches!(
                source.provider,
                AccountProvider::Antigravity | AccountProvider::Copilot
            ) && matches!(
                proxy.status,
                StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN
            )) || (source.provider == AccountProvider::Copilot
                && matches!(
                    proxy.error_code.as_deref(),
                    Some("model_not_found" | "model_not_supported")
                ))
            {
                crate::services::account_proxy::forget_preparation_metadata(
                    source.provider,
                    &source.account_id,
                );
            }
        }
        let mut headers = HeaderMap::new();
        if let Some(value) = &proxy.retry_after {
            headers.insert(header::RETRY_AFTER, value.clone());
        }
        let body = json!({"error":{"message":proxy.message,"code":proxy.error_code,"type":proxy.error_type,"retry_after":proxy.retry_after.as_ref().and_then(|value|value.to_str().ok())}}).to_string();
        return mark_upstream_failure(
            state,
            candidate,
            proxy.status,
            body.as_bytes(),
            retry_after(&headers),
            proxy.model_scope,
        );
    }
    mark_failure(state, candidate, FailureClass::Transient, None);
    Some(FailureClass::Transient)
}

pub(super) fn mark_upstream_failure(
    state: &AppState,
    candidate: &Candidate,
    status: StatusCode,
    body: &[u8],
    retry_after: Option<Duration>,
    model_scope: bool,
) -> Option<FailureClass> {
    let failure = classify_failure(status, body);
    let Some(failure) = failure else {
        if let Some(statistics) = &state.request_statistics {
            statistics.outcome(&candidate.internal_id, Some(5));
        }
        return None;
    };
    let reset_at_ms = if model_scope
        && candidate.account_source.as_ref().is_some_and(|source| {
            crate::services::plugin_accounts::application(source.provider).is_some()
        }) {
        serde_json::from_slice::<Value>(body)
            .ok()
            .and_then(|value| {
                value["error"]["retry_after"]
                    .as_str()
                    .and_then(|value| value.parse::<u64>().ok())
            })
            .filter(|seconds| *seconds <= 7 * 24 * 60 * 60)
            .map(|seconds| now_ms().saturating_add(seconds.saturating_mul(1000)))
    } else {
        None
    };
    mark_failure_until(
        state,
        candidate,
        failure,
        retry_after,
        model_scope,
        reset_at_ms,
    );
    Some(failure)
}

#[cfg(test)]
pub(super) fn begin_attempt(state: &AppState, candidate: &mut Candidate) {
    candidate.attempt = Some(new_attempt(&state.route_memory.lock().unwrap(), candidate));
}

pub(super) fn try_begin_attempt(state: &AppState, candidate: &mut Candidate) -> bool {
    let Ok(mut memory) = state.route_memory.lock() else {
        return false;
    };
    if is_cooling_down(&memory, candidate, now_ms()) {
        return false;
    }
    // Weak leases last through the response body, and disappear on cancellation.
    // Healthy resources take no lease and retain their normal parallelism.
    let expired: Vec<_> = memory
        .recovering
        .iter()
        .filter(|(_, attempt)| {
            attempt
                .upgrade()
                .is_none_or(|attempt| !attempt_epoch_is_current(&memory, &attempt))
        })
        .map(|(key, _)| key.clone())
        .collect();
    for key in expired {
        memory.recovering.remove(&key);
    }
    let mut recovery_keys = Vec::new();
    if candidate_health(&memory, candidate).is_some() {
        recovery_keys.push(format!("model:{}", resource_key(candidate)));
    }
    if let Some(key) = account_health_key(candidate).filter(|_| {
        applicable_account_health(&memory, candidate).is_some_and(|health| {
            health
                .cooldown_until_ms
                .is_some_and(|until| until <= now_ms())
        })
    }) {
        recovery_keys.push(format!("account:{key}"));
    }
    if recovery_keys
        .iter()
        .any(|key| memory.recovering.contains_key(key))
    {
        return false;
    }
    let attempt = new_attempt(&memory, candidate);
    memory
        .candidate_epochs
        .entry(candidate.internal_id.clone())
        .or_default();
    for key in recovery_keys {
        memory.recovering.insert(key, Arc::downgrade(&attempt));
    }
    candidate.attempt = Some(attempt);
    true
}

pub(super) fn new_attempt(memory: &RouteMemory, candidate: &Candidate) -> Arc<AttemptHealth> {
    Arc::new(AttemptHealth {
        revision: memory.revision,
        candidate_id: candidate.internal_id.clone(),
        account_key: account_health_key(candidate),
        account_epoch: account_health_key(candidate)
            .and_then(|key| memory.account_epochs.get(&key).copied())
            .unwrap_or(0),
        candidate_epoch: memory
            .candidate_epochs
            .get(&candidate.internal_id)
            .copied()
            .unwrap_or(0),
        credential_version: Mutex::new(None),
    })
}

pub(super) fn attempt_has_current_credentials(memory: &RouteMemory, candidate: &Candidate) -> bool {
    let Some(attempt) = &candidate.attempt else {
        return true;
    };
    if !attempt_epoch_is_current(memory, attempt) {
        return false;
    }
    let Some(key) = account_health_key(candidate) else {
        return true;
    };
    let version = attempt.credential_version.lock().ok();
    match (
        version.as_deref().and_then(|value| value.as_ref()),
        memory.credential_versions.get(&key),
    ) {
        (Some(attempted), Some((current, _))) => attempted == current,
        _ => true,
    }
}

pub(super) fn attempt_epoch_is_current(memory: &RouteMemory, attempt: &AttemptHealth) -> bool {
    memory
        .candidate_epochs
        .get(&attempt.candidate_id)
        .copied()
        .unwrap_or(0)
        == attempt.candidate_epoch
        && attempt.account_key.as_ref().is_none_or(|key| {
            memory.account_epochs.get(key).copied().unwrap_or(0) == attempt.account_epoch
        })
}

pub(super) fn attempt_is_current(memory: &RouteMemory, candidate: &Candidate) -> bool {
    if !attempt_has_current_credentials(memory, candidate) {
        return false;
    }
    let Some(attempt) = &candidate.attempt else {
        return true;
    };
    if memory
        .recovered_revisions
        .get(&format!("model:{}", resource_key(candidate)))
        .is_some_and(|revision| attempt.revision < *revision)
    {
        return false;
    }
    if let Some(key) = account_health_key(candidate) {
        if memory
            .recovered_revisions
            .get(&format!("account:{key}"))
            .is_some_and(|revision| attempt.revision < *revision)
        {
            return false;
        }
    }
    true
}

pub(super) fn clear_account_authentication(
    memory: &mut RouteMemory,
    key: &str,
    candidate_ids: &HashSet<String>,
) -> bool {
    let before = memory.candidates.len() + memory.accounts.len();
    memory.candidates.retain(|id, health| {
        !(health.failure == Some(FailureClass::Authentication)
            && (health.account_key.as_deref() == Some(key) || candidate_ids.contains(id)))
    });
    if memory
        .accounts
        .get(key)
        .is_some_and(|health| health.failure == Some(FailureClass::Authentication))
    {
        memory.accounts.remove(key);
    }
    before != memory.candidates.len() + memory.accounts.len()
}

pub(crate) fn forget_account_authentication(
    provider: crate::models::model::AccountProvider,
    account_id: &str,
) {
    crate::services::account_proxy::forget_preparation_metadata(provider, account_id);
    let source = crate::models::model::AccountSource {
        provider,
        account_id: account_id.into(),
    };
    let key = source_health_key(&source);
    let candidate_ids = model_manager::get_user_models()
        .into_iter()
        .filter(|model| model.account_source.as_ref() == Some(&source))
        .map(|model| model.internal_id)
        .collect();
    let memory = shared_route_memory();
    let Ok(mut memory) = memory.lock() else {
        return;
    };
    let epoch = memory.account_epochs.entry(key.clone()).or_default();
    *epoch = epoch.saturating_add(1);
    memory.credential_versions.remove(&key);
    memory.revision = memory.revision.saturating_add(1);
    if clear_account_authentication(&mut memory, &key, &candidate_ids) {
        persist_memory_at(&route_memory_path(), memory);
    }
}

pub(super) fn bind_attempt_credentials(
    state: &AppState,
    candidate: &Candidate,
    credentials: &Value,
) {
    let Some(attempt) = &candidate.attempt else {
        return;
    };
    let Some(key) = account_health_key(candidate) else {
        return;
    };
    let version = hex::encode(Sha256::digest(credentials.to_string().as_bytes()));
    let Ok(mut memory) = state.route_memory.lock() else {
        return;
    };
    if !attempt_epoch_is_current(&memory, attempt) {
        return;
    }
    let Ok(mut attempted_version) = attempt.credential_version.lock() else {
        return;
    };
    let current = memory.credential_versions.get(&key);
    let may_update = current.is_none_or(|(current, revision)| {
        current == &version
            || attempted_version.as_ref() == Some(current)
            || *revision <= attempt.revision
    });
    *attempted_version = Some(version.clone());
    if may_update && current.is_none_or(|(current, _)| current != &version) {
        memory.revision = memory.revision.saturating_add(1);
        let revision = memory.revision;
        memory
            .credential_versions
            .insert(key.clone(), (version, revision));
        if clear_account_authentication(&mut memory, &key, &HashSet::new()) {
            persist_route_memory(state, memory);
        }
    }
}

pub(super) fn mark_success(state: &AppState, candidate: &Candidate) {
    if let Some(statistics) = &state.request_statistics {
        statistics.outcome(&candidate.internal_id, None);
    }
    let Ok(mut memory) = state.route_memory.lock() else {
        return;
    };
    if !attempt_is_current(&memory, candidate) {
        return;
    }
    let revision = candidate
        .attempt
        .as_ref()
        .map_or(u64::MAX, |attempt| attempt.revision);
    let may_clear = |health: &CandidateHealth| {
        health.revision <= revision
            && !(matches!(
                health.failure,
                Some(FailureClass::Billing | FailureClass::RateLimit)
            ) && health
                .cooldown_until_ms
                .is_some_and(|until| until > now_ms()))
    };
    let previous_len = memory.candidates.len();
    let resource = resource_key(candidate);
    memory.candidates.retain(|_, health| {
        !health
            .resource_key
            .as_deref()
            .map_or(health.fingerprint == candidate.fingerprint, |key| {
                key == resource
            })
            || !may_clear(health)
    });
    let removed_failure = memory.candidates.len() != previous_len;
    let removed_account = account_health_key(candidate).is_some_and(|key| {
        memory.accounts.get(&key).is_some_and(may_clear) && memory.accounts.remove(&key).is_some()
    });
    let watermark = revision.min(memory.revision);
    if removed_failure {
        memory
            .recovered_revisions
            .insert(format!("model:{resource}"), watermark);
    }
    if removed_account {
        if let Some(key) = account_health_key(candidate) {
            memory
                .recovered_revisions
                .insert(format!("account:{key}"), watermark);
        }
    }
    if removed_failure || removed_account {
        persist_route_memory(state, memory);
    }
}

pub(super) fn mark_failure(
    state: &AppState,
    candidate: &Candidate,
    failure: FailureClass,
    retry_after: Option<Duration>,
) {
    mark_failure_scoped(state, candidate, failure, retry_after, false);
}

pub(super) fn mark_failure_scoped(
    state: &AppState,
    candidate: &Candidate,
    failure: FailureClass,
    retry_after: Option<Duration>,
    model_scope: bool,
) {
    mark_failure_until(state, candidate, failure, retry_after, model_scope, None);
}

pub(super) fn mark_failure_until(
    state: &AppState,
    candidate: &Candidate,
    failure: FailureClass,
    retry_after: Option<Duration>,
    model_scope: bool,
    reset_at_ms: Option<u64>,
) {
    if let Some(statistics) = &state.request_statistics {
        let category = match failure {
            FailureClass::RateLimit => 0,
            FailureClass::Authentication => 1,
            FailureClass::Billing => 2,
            FailureClass::ModelUnavailable => 3,
            FailureClass::Transient => 4,
            FailureClass::RequestIncompatible => 5,
        };
        statistics.outcome(&candidate.internal_id, Some(category));
    }
    if failure == FailureClass::RequestIncompatible {
        return;
    }
    let Ok(mut memory) = state.route_memory.lock() else {
        return;
    };
    if !attempt_is_current(&memory, candidate) {
        return;
    }
    let mut next = candidate_health(&memory, candidate)
        .cloned()
        .unwrap_or_else(|| CandidateHealth {
            fingerprint: candidate.fingerprint.clone(),
            ..CandidateHealth::default()
        });
    next.fingerprint = candidate.fingerprint.clone();
    next.resource_key = Some(resource_key(candidate));
    let same_transient_wave = failure == FailureClass::Transient
        && next.failure == Some(failure)
        && (next.cooldown_until_ms.is_some_and(|until| until > now_ms())
            || candidate
                .attempt
                .as_ref()
                .is_some_and(|attempt| attempt.revision < next.revision));
    if same_transient_wave {
        return;
    }
    memory.revision = memory.revision.saturating_add(1);
    let revision = memory.revision;
    next.consecutive_failures = next.consecutive_failures.saturating_add(1);
    let entry = memory
        .candidates
        .entry(candidate.internal_id.clone())
        .or_default();
    *entry = next;
    let cooldown = cooldown_for_failure(failure, retry_after, entry.consecutive_failures);
    let deadline = reset_at_ms.unwrap_or_else(|| {
        now_ms().saturating_add(cooldown.as_millis().min(u128::from(u64::MAX)) as u64)
    });
    if entry
        .cooldown_until_ms
        .is_none_or(|until| until <= deadline)
    {
        entry.failure = Some(failure);
        entry.cooldown_until_ms = Some(deadline);
    }
    entry.revision = revision;
    entry.account_key = account_health_key(candidate);
    state.probe_pool.forget(candidate);
    let consecutive_failures = entry.consecutive_failures;
    if !model_scope && failure == FailureClass::Authentication {
        if let Some(key) = account_health_key(candidate) {
            let existing = memory.accounts.get(&key);
            if existing
                .and_then(|entry| entry.cooldown_until_ms)
                .is_none_or(|until| until <= deadline)
            {
                memory.accounts.insert(
                    key.clone(),
                    CandidateHealth {
                        consecutive_failures,
                        cooldown_until_ms: Some(deadline),
                        fingerprint: candidate.fingerprint.clone(),
                        resource_key: None,
                        failure: Some(failure),
                        revision,
                        account_key: Some(key),
                    },
                );
            }
        }
    }
    persist_route_memory(state, memory);
}
