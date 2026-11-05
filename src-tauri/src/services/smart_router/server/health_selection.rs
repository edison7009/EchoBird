// Router responsibility; behavior is covered by the server regression suite.
use super::{
    candidate_fingerprint, now_ms, AppState, Candidate, CandidateHealth, FailureClass, RouteMemory,
};
use sha2::Digest;
use sha2::Sha256;

pub(super) fn prioritized_candidates(
    state: &AppState,
    candidates: Vec<Candidate>,
) -> Vec<Candidate> {
    let Ok(memory) = state.route_memory.lock() else {
        return candidates;
    };
    let now = now_ms();
    candidates
        .into_iter()
        .filter(|candidate| !is_cooling_down(&memory, candidate, now))
        .collect()
}

pub(super) fn candidate_health<'a>(
    memory: &'a RouteMemory,
    candidate: &Candidate,
) -> Option<&'a CandidateHealth> {
    let resource = resource_key(candidate);
    memory
        .candidates
        .values()
        .filter(|health| {
            health
                .resource_key
                .as_deref()
                .map_or(health.fingerprint == candidate.fingerprint, |key| {
                    key == resource
                })
        })
        .max_by_key(|health| health.cooldown_until_ms)
}

pub(super) fn resource_key(candidate: &Candidate) -> String {
    if let Some(source) = &candidate.account_source {
        candidate_fingerprint(
            &candidate.model_id,
            "account://echobird",
            &serde_json::to_string(source).unwrap_or_default(),
        )
    } else {
        candidate_fingerprint(
            &candidate.model_id,
            candidate.base_url.trim_end_matches('/'),
            &candidate.api_key,
        )
    }
}

pub(super) fn account_is_cooling_down(
    memory: &RouteMemory,
    candidate: &Candidate,
    now: u64,
) -> bool {
    applicable_account_health(memory, candidate)
        .is_some_and(|health| health.cooldown_until_ms.is_some_and(|until| until > now))
}

pub(super) fn applicable_account_health<'a>(
    memory: &'a RouteMemory,
    candidate: &Candidate,
) -> Option<&'a CandidateHealth> {
    let health = account_health_key(candidate).and_then(|key| memory.accounts.get(&key))?;
    // Old quota records may cover the whole account. Only authentication is
    // reliable account-wide evidence; model quota failures stay on their resource.
    (health.failure == Some(FailureClass::Authentication)).then_some(health)
}

pub(super) fn account_health_key(candidate: &Candidate) -> Option<String> {
    candidate.account_source.as_ref().map(source_health_key)
}

pub(super) fn source_health_key(source: &crate::models::model::AccountSource) -> String {
    hex::encode(Sha256::digest(
        serde_json::to_vec(source).unwrap_or_default(),
    ))
}

pub(super) fn is_cooling_down(memory: &RouteMemory, candidate: &Candidate, now: u64) -> bool {
    candidate_health(memory, candidate)
        .and_then(|health| health.cooldown_until_ms)
        .is_some_and(|until| until > now)
        || account_is_cooling_down(memory, candidate, now)
}

pub(super) fn candidate_is_cooling_down(state: &AppState, candidate: &Candidate) -> bool {
    let Ok(memory) = state.route_memory.lock() else {
        return false;
    };
    is_cooling_down(&memory, candidate, now_ms())
}
