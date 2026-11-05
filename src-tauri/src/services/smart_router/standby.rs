use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, Weak};

use futures_util::future::{BoxFuture, FutureExt, Shared};
use tokio_util::sync::CancellationToken;

use super::{resource_key, AppState, Candidate, RouteProtocol};

type Signature = Vec<(String, String)>;

#[derive(Default)]
struct GroupState {
    signature: Signature,
    current: Option<(String, String)>,
    epoch: u64,
    round_started: bool,
    worker: Option<CancellationToken>,
    ready: Option<Ready>,
}

struct Ready {
    candidate_id: String,
    _probe: Arc<SharedProbe>,
}

impl GroupState {
    fn cancel_round(&mut self) {
        if let Some(worker) = self.worker.take() {
            worker.cancel();
        }
        self.ready = None;
        self.round_started = false;
        self.epoch = self.epoch.saturating_add(1);
    }
}

pub(super) struct Standby {
    enabled: bool,
    group: Mutex<GroupState>,
}

struct Round {
    epoch: u64,
    cancel: CancellationToken,
    candidates: Vec<Candidate>,
}

impl Default for Standby {
    fn default() -> Self {
        Self::new()
    }
}

impl Standby {
    pub(super) fn new() -> Self {
        Self {
            enabled: true,
            group: Mutex::new(GroupState::default()),
        }
    }

    #[cfg(test)]
    pub(super) fn disabled() -> Self {
        Self {
            enabled: false,
            group: Mutex::new(GroupState::default()),
        }
    }

    pub(super) fn epoch(&self) -> u64 {
        self.group
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .epoch
    }

    pub(super) fn order_for_request(
        &self,
        candidates: &mut [Candidate],
        expected_epoch: Option<u64>,
    ) -> Option<u64> {
        if !self.enabled {
            return Some(0);
        }
        let mut group = self.group.lock().unwrap_or_else(|error| error.into_inner());
        let signature = signature(candidates);
        let stale = expected_epoch.is_some_and(|epoch| epoch != group.epoch);
        if stale && group.signature != signature {
            return None;
        }
        if !stale && group.signature != signature {
            group.cancel_round();
            group.current = signature.first().cloned();
            group.signature = signature;
        }
        if let Some(index) = group.current.as_ref().and_then(|current| {
            candidates
                .iter()
                .position(|candidate| identity(candidate) == *current)
        }) {
            candidates.rotate_left(index);
        }
        if candidates.len() > 1 {
            if let Some(index) = group.ready.as_ref().and_then(|ready| {
                candidates[1..]
                    .iter()
                    .position(|candidate| candidate.internal_id == ready.candidate_id)
            }) {
                candidates[1..].rotate_left(index);
            }
        }
        (!stale).then_some(group.epoch)
    }

    #[cfg(test)]
    pub(super) fn order(&self, candidates: &mut [Candidate]) -> u64 {
        self.order_for_request(candidates, None).unwrap()
    }

    fn begin_round(
        &self,
        candidate: &Candidate,
        candidates: &[Candidate],
        epoch: u64,
    ) -> Option<Round> {
        if !self.enabled {
            return None;
        }
        let mut group = self.group.lock().unwrap_or_else(|error| error.into_inner());
        let used = identity(candidate);
        if group.epoch != epoch
            || group.signature != signature(candidates)
            || !group.signature.contains(&used)
        {
            return None;
        }
        if group.current.as_ref() != Some(&used) {
            group.cancel_round();
            group.current = Some(used);
        }
        if group.round_started {
            return None;
        }
        group.round_started = true;
        let cancel = CancellationToken::new();
        group.worker = Some(cancel.clone());
        let index = candidates
            .iter()
            .position(|other| identity(other) == identity(candidate))?;
        let mut seen = HashSet::from([resource_key(candidate)]);
        let candidates = candidates
            .iter()
            .cycle()
            .skip(index + 1)
            .take(candidates.len().saturating_sub(1))
            .filter(|candidate| seen.insert(resource_key(candidate)))
            .cloned()
            .collect();
        Some(Round {
            epoch: group.epoch,
            cancel,
            candidates,
        })
    }

    pub(super) fn used(
        self: &Arc<Self>,
        state: &AppState,
        candidate: &Candidate,
        candidates: &[Candidate],
        protocol: RouteProtocol,
        epoch: u64,
    ) {
        if state.shutdown.is_cancelled() {
            return;
        }
        let Some(round) = self.begin_round(candidate, candidates, epoch) else {
            return;
        };
        let standby = self.clone();
        let state = state.clone();
        tokio::spawn(async move {
            for candidate in round.candidates {
                if round.cancel.is_cancelled() || state.shutdown.is_cancelled() {
                    break;
                }
                if super::candidate_is_cooling_down(&state, &candidate) {
                    continue;
                }
                let probe = state
                    .probe_pool
                    .acquire(&state, candidate.clone(), protocol);
                let usable = tokio::select! {
                    biased;
                    _ = round.cancel.cancelled() => break,
                    _ = state.shutdown.cancelled() => break,
                    usable = probe.result.clone() => usable,
                };
                if usable {
                    let mut group = standby
                        .group
                        .lock()
                        .unwrap_or_else(|error| error.into_inner());
                    if group.epoch == round.epoch
                        && !round.cancel.is_cancelled()
                        && !state.shutdown.is_cancelled()
                    {
                        group.ready = Some(Ready {
                            candidate_id: candidate.internal_id,
                            _probe: probe,
                        });
                    }
                    break;
                }
            }
            let mut group = standby
                .group
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            if group.epoch == round.epoch {
                group.worker = None;
            }
        });
    }

    fn invalidate(&self) {
        let mut group = self.group.lock().unwrap_or_else(|error| error.into_inner());
        group.cancel_round();
        group.signature.clear();
        group.current = None;
    }

    #[cfg(test)]
    pub(super) fn snapshot(&self) -> (Option<String>, Option<String>, bool, u64) {
        let group = self.group.lock().unwrap_or_else(|error| error.into_inner());
        (
            group.current.as_ref().map(|(id, _)| id.clone()),
            group.ready.as_ref().map(|ready| ready.candidate_id.clone()),
            group.worker.is_some(),
            group.epoch,
        )
    }
}

fn identity(candidate: &Candidate) -> (String, String) {
    (candidate.internal_id.clone(), candidate.fingerprint.clone())
}

fn signature(candidates: &[Candidate]) -> Signature {
    candidates.iter().map(identity).collect()
}

struct SharedProbe {
    result: Shared<BoxFuture<'static, bool>>,
}

#[derive(Default)]
pub(super) struct ProbePool {
    probes: Mutex<HashMap<String, Weak<SharedProbe>>>,
}

impl ProbePool {
    #[cfg(test)]
    pub(super) fn owner_count(&self, candidate: &Candidate, protocol: RouteProtocol) -> usize {
        self.probes
            .lock()
            .unwrap()
            .get(&probe_key(candidate, protocol))
            .map_or(0, Weak::strong_count)
    }

    fn acquire(
        &self,
        state: &AppState,
        candidate: Candidate,
        protocol: RouteProtocol,
    ) -> Arc<SharedProbe> {
        let mut probes = self
            .probes
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        probes.retain(|_, probe| probe.strong_count() > 0);
        let key = probe_key(&candidate, protocol);
        if let Some(probe) = probes.get(&key).and_then(Weak::upgrade) {
            return probe;
        }
        let state = state.clone();
        let probe = Arc::new(SharedProbe {
            result: async move { super::standby_probe::check(&state, candidate, protocol).await }
                .boxed()
                .shared(),
        });
        probes.insert(key, Arc::downgrade(&probe));
        probe
    }

    pub(super) fn forget(&self, candidate: &Candidate) {
        let mut probes = self
            .probes
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        for protocol in [RouteProtocol::Chat, RouteProtocol::Responses] {
            probes.remove(&probe_key(candidate, protocol));
        }
    }
}

fn probe_key(candidate: &Candidate, protocol: RouteProtocol) -> String {
    format!("{protocol:?}:{}", resource_key(candidate))
}

static PROBES: OnceLock<Arc<ProbePool>> = OnceLock::new();

pub(super) fn shared_probes() -> Arc<ProbePool> {
    PROBES
        .get_or_init(|| Arc::new(ProbePool::default()))
        .clone()
}

type Registry = HashMap<PathBuf, Vec<Weak<Standby>>>;
static GROUPS: OnceLock<Mutex<Registry>> = OnceLock::new();

pub(super) fn register(path: &Path, standby: &Arc<Standby>) {
    let mut groups = GROUPS
        .get_or_init(Mutex::default)
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    groups.retain(|_, entries| {
        entries.retain(|entry| entry.strong_count() > 0);
        !entries.is_empty()
    });
    let entries = groups.entry(path.to_owned()).or_default();
    let weak = Arc::downgrade(standby);
    if !entries.iter().any(|entry| Weak::ptr_eq(entry, &weak)) {
        entries.push(weak);
    }
}

pub(super) fn invalidate(path: &Path) {
    invalidate_matching(Some(path));
}

pub(super) fn invalidate_all() {
    invalidate_matching(None);
    if let Some(probes) = PROBES.get() {
        probes
            .probes
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clear();
    }
}

fn invalidate_matching(path: Option<&Path>) {
    let Some(registry) = GROUPS.get() else {
        return;
    };
    let standbys: Vec<_> = {
        let mut groups = registry.lock().unwrap_or_else(|error| error.into_inner());
        let mut standbys = Vec::new();
        groups.retain(|key, entries| {
            entries.retain(|entry| {
                let Some(standby) = entry.upgrade() else {
                    return false;
                };
                if path.is_none_or(|path| path == key) {
                    standbys.push(standby);
                }
                true
            });
            !entries.is_empty()
        });
        standbys
    };
    for standby in standbys {
        standby.invalidate();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidates() -> Vec<Candidate> {
        (1..=4)
            .map(|index| Candidate {
                internal_id: index.to_string(),
                model_id: format!("model-{index}"),
                base_url: "http://127.0.0.1:1/v1".into(),
                api_key: "fixture".into(),
                account_source: None,
                fingerprint: format!("fingerprint-{index}"),
                attempt: None,
            })
            .collect()
    }

    #[test]
    fn working_position_stays_put_and_old_success_cannot_move_it_back() {
        let standby = Standby::new();
        let original = candidates();
        let epoch = standby.order(&mut original.clone());
        let initial = standby.begin_round(&original[0], &original, epoch).unwrap();
        assert_eq!(initial.candidates[0].internal_id, "2");
        assert!(standby
            .begin_round(&original[0], &original, epoch)
            .is_none());
        let next = standby.begin_round(&original[1], &original, epoch).unwrap();
        assert!(initial.cancel.is_cancelled());
        assert_eq!(
            next.candidates
                .iter()
                .map(|candidate| candidate.internal_id.as_str())
                .collect::<Vec<_>>(),
            ["3", "4", "1"]
        );
        assert!(standby
            .begin_round(&original[0], &original, epoch)
            .is_none());
        let mut ordered = original.clone();
        let next_epoch = standby.order(&mut ordered);
        assert_eq!(next_epoch, next.epoch);
        assert_eq!(
            ordered
                .iter()
                .map(|candidate| candidate.internal_id.as_str())
                .collect::<Vec<_>>(),
            ["2", "3", "4", "1"]
        );
    }

    #[test]
    fn configuration_changes_cancel_the_round_and_restore_configured_order() {
        let standby = Standby::new();
        let mut original = candidates();
        let epoch = standby.order(&mut original);
        let round = standby.begin_round(&original[2], &original, epoch).unwrap();
        original.swap(0, 1);
        let changed_epoch = standby.order(&mut original);
        assert!(round.cancel.is_cancelled());
        assert!(changed_epoch > round.epoch);
        assert_eq!(standby.snapshot().0.as_deref(), Some("2"));
        assert!(standby
            .begin_round(&original[2], &original, epoch)
            .is_none());
        original[0].fingerprint = "edited".into();
        assert!(standby.order(&mut original) > changed_epoch);
    }

    #[test]
    fn configuration_changes_reject_a_request_that_loaded_the_previous_snapshot() {
        let standby = Standby::new();
        let original = candidates();
        standby.order(&mut original.clone());
        let captured = standby.epoch();
        standby.invalidate();
        let invalidated = standby.epoch();
        assert!(standby
            .order_for_request(&mut original.clone(), Some(captured))
            .is_none());
        assert_eq!(standby.snapshot(), (None, None, false, invalidated));
        assert!(standby
            .begin_round(&original[0], &original, captured)
            .is_none());
        assert!(standby
            .order_for_request(&mut original.clone(), Some(invalidated))
            .is_some());
    }

    #[test]
    fn concurrent_old_requests_follow_the_new_position_without_overwriting_it() {
        let standby = Standby::new();
        let original = candidates();
        let captured = standby.order(&mut original.clone());
        let round = standby
            .begin_round(&original[1], &original, captured)
            .unwrap();
        let mut stale_request = original.clone();
        assert!(standby
            .order_for_request(&mut stale_request, Some(captured))
            .is_none());
        assert_eq!(stale_request[0].internal_id, "2");
        assert_eq!(standby.snapshot().3, round.epoch);
        assert!(standby
            .begin_round(&original[0], &original, captured)
            .is_none());
        assert!(!round.cancel.is_cancelled());
    }

    #[test]
    fn a_ready_standby_is_first_fallback_without_displacing_the_working_model() {
        let standby = Standby::new();
        let original = candidates();
        let epoch = standby.order(&mut original.clone());
        standby.begin_round(&original[0], &original, epoch).unwrap();
        standby.group.lock().unwrap().ready = Some(Ready {
            candidate_id: "3".into(),
            _probe: Arc::new(SharedProbe {
                result: futures_util::future::ready(true).boxed().shared(),
            }),
        });
        let mut ordered = original.clone();
        assert_eq!(standby.order(&mut ordered), epoch);
        assert_eq!(
            ordered
                .iter()
                .map(|candidate| candidate.internal_id.as_str())
                .collect::<Vec<_>>(),
            ["1", "3", "4", "2"]
        );
        assert_eq!(standby.snapshot().0.as_deref(), Some("1"));
        let next = standby.begin_round(&original[2], &original, epoch).unwrap();
        assert_eq!(
            next.candidates
                .iter()
                .map(|candidate| candidate.internal_id.as_str())
                .collect::<Vec<_>>(),
            ["4", "1", "2"]
        );
        assert_eq!(standby.snapshot().0.as_deref(), Some("3"));
        assert!(standby.snapshot().1.is_none());
    }

    #[test]
    fn duplicate_resource_aliases_do_not_consume_more_probe_requests() {
        let standby = Standby::new();
        let mut original = candidates();
        let mut alias = original[0].clone();
        alias.internal_id = "alias".into();
        original.insert(1, alias);
        let epoch = standby.order(&mut original.clone());
        let round = standby.begin_round(&original[0], &original, epoch).unwrap();
        assert_eq!(
            round
                .candidates
                .iter()
                .map(|candidate| candidate.internal_id.as_str())
                .collect::<Vec<_>>(),
            ["2", "3", "4"]
        );
        assert_eq!(
            probe_key(&original[0], RouteProtocol::Chat),
            probe_key(&original[1], RouteProtocol::Chat)
        );
        assert_ne!(
            probe_key(&original[0], RouteProtocol::Chat),
            probe_key(&original[0], RouteProtocol::Responses)
        );
    }

    #[test]
    fn registry_invalidation_stops_only_the_matching_groups() {
        let first = Arc::new(Standby::new());
        let second = Arc::new(Standby::new());
        let original = candidates();
        let first_epoch = first.order(&mut original.clone());
        let second_epoch = second.order(&mut original.clone());
        let first_round = first
            .begin_round(&original[0], &original, first_epoch)
            .unwrap();
        let second_round = second
            .begin_round(&original[0], &original, second_epoch)
            .unwrap();
        let path = PathBuf::from(format!("standby-registry-{}", uuid::Uuid::new_v4()));
        let other = path.with_extension("other");
        register(&path, &first);
        register(&path, &first);
        register(&other, &second);
        invalidate(&path);
        assert!(first_round.cancel.is_cancelled());
        assert!(!second_round.cancel.is_cancelled());
        assert!(first.snapshot().0.is_none());
        assert_eq!(second.snapshot().0.as_deref(), Some("1"));
    }

    #[test]
    fn disabled_controllers_do_not_reorder_or_start_background_work() {
        let standby = Standby::disabled();
        let mut original = candidates();
        assert_eq!(standby.order(&mut original), 0);
        assert!(standby.begin_round(&original[1], &original, 0).is_none());
        assert_eq!(original[0].internal_id, "1");
        assert_eq!(standby.snapshot(), (None, None, false, 0));
    }
}
