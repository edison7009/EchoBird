//! Bounded local aggregates. Response content and credentials are never persisted.
use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use chrono::{Datelike, Local, Timelike};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::usage::TokenUsage;
use crate::models::model::AccountSource;

pub const RETENTION_DAYS: i32 = 14;
const MAX_IDENTITIES: usize = 256;
const MAX_GROUPS: usize = 32;
const MAX_FILE_BYTES: u64 = 8 * 1024 * 1024;
const FLUSH_DELAY: Duration = Duration::from_secs(30);
const STORAGE_ERROR: &str = "Unable to read or save local router statistics";
static COLLECTOR: OnceLock<Arc<Collector>> = OnceLock::new();
static ENABLED: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Counts {
    #[serde(default, skip_serializing_if = "TokenUsage::is_empty")]
    pub usage: TokenUsage,
    pub attempts: u64,
    pub completed: u64,
    pub failed: u64,
    pub cancelled: u64,
    pub protections: [u64; 5],
    pub errors: [u64; 6],
}

impl Counts {
    pub fn add(&mut self, other: &Self) {
        self.usage.add(&other.usage);
        self.attempts = self.attempts.saturating_add(other.attempts);
        self.completed = self.completed.saturating_add(other.completed);
        self.failed = self.failed.saturating_add(other.failed);
        self.cancelled = self.cancelled.saturating_add(other.cancelled);
        for (target, value) in self.protections.iter_mut().zip(other.protections) {
            *target = target.saturating_add(value);
        }
        for (target, value) in self.errors.iter_mut().zip(other.errors) {
            *target = target.saturating_add(value);
        }
    }
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Totals {
    #[serde(default, skip_serializing_if = "TokenUsage::is_empty")]
    pub usage: TokenUsage,
    pub calls: u64,
    pub completed: u64,
    pub failed: u64,
    pub cancelled: u64,
    pub rescued: u64,
    pub duration_ms: u64,
}

impl Totals {
    fn add(&mut self, other: &Self) {
        self.usage.add(&other.usage);
        self.calls = self.calls.saturating_add(other.calls);
        self.completed = self.completed.saturating_add(other.completed);
        self.failed = self.failed.saturating_add(other.failed);
        self.cancelled = self.cancelled.saturating_add(other.cancelled);
        self.rescued = self.rescued.saturating_add(other.rescued);
        self.duration_ms = self.duration_ms.saturating_add(other.duration_ms);
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Identity {
    pub group: String,
    pub account: String,
    pub model: String,
    pub last_used_at: u64,
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Day {
    rows: BTreeMap<String, Counts>,
    hours: BTreeMap<u8, BTreeMap<String, Totals>>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Store {
    version: u8,
    since: u64,
    capped: Option<i32>,
    identities: BTreeMap<String, Identity>,
    groups: BTreeSet<String>,
    days: BTreeMap<i32, Day>,
    #[serde(default)]
    credits: TokenCredits,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TokenCredits {
    pub ledger_id: String,
    pub total: u64,
}
impl Default for TokenCredits {
    fn default() -> Self {
        Self {
            ledger_id: uuid::Uuid::new_v4().to_string(),
            total: 0,
        }
    }
}

impl Default for Store {
    fn default() -> Self {
        Self {
            version: 1,
            since: 0,
            capped: None,
            identities: BTreeMap::new(),
            groups: BTreeSet::new(),
            days: BTreeMap::new(),
            credits: TokenCredits::default(),
        }
    }
}

impl Store {
    fn trim(&mut self, today: i32) {
        self.days
            .retain(|day, _| *day > today - RETENTION_DAYS && *day <= today);
    }

    fn valid(&self) -> bool {
        let hash =
            |value: &str| value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit());
        self.version == 1
            && uuid::Uuid::parse_str(&self.credits.ledger_id).is_ok()
            && self.identities.len() <= MAX_IDENTITIES
            && self.groups.len() <= MAX_GROUPS
            && self.days.len() <= RETENTION_DAYS as usize
            && self.groups.iter().all(|id| hash(id))
            && self.identities.iter().all(|(id, value)| {
                hash(id)
                    && hash(&value.account)
                    && hash(&value.model)
                    && self.groups.contains(&value.group)
            })
            && self.days.values().all(|day| {
                day.rows.len() <= MAX_IDENTITIES
                    && day.rows.keys().all(|id| self.identities.contains_key(id))
                    && day.hours.len() <= 24
                    && day.hours.iter().all(|(hour, groups)| {
                        *hour < 24
                            && groups.len() <= MAX_GROUPS
                            && groups.keys().all(|id| self.groups.contains(id))
                    })
            })
    }
}

struct State {
    durable_credits: TokenCredits,
    data: Store,
    revision: u64,
    saved_revision: u64,
    saved_at: u64,
    storage_error: bool,
}

pub struct Collector {
    path: PathBuf,
    state: Mutex<State>,
    write: Mutex<()>,
    active: AtomicBool,
    writer_started: AtomicBool,
    epoch: AtomicU64,
    wake: tokio::sync::Notify,
    #[cfg(test)]
    writes: AtomicU64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Row {
    pub id: String,
    #[serde(flatten)]
    pub identity: Identity,
    pub counts: Counts,
}

#[derive(Serialize)]
pub struct Trend {
    pub day: i32,
    pub hour: u8,
    pub totals: Totals,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub recording: bool,
    pub since: u64,
    pub saved_at: u64,
    pub storage_error: bool,
    pub capped: bool,
    pub today: i32,
    pub totals: Totals,
    pub rows: Vec<Row>,
    pub trend: Vec<Trend>,
}

pub fn hash(value: &str) -> String {
    hex::encode(Sha256::digest(value.as_bytes()))
}

pub fn row_identity(
    group: &str,
    candidate: &str,
    model: &str,
    source: Option<&AccountSource>,
) -> (String, Identity) {
    let account = hash(
        &source
            .map(|source| serde_json::to_string(source).unwrap_or_default())
            .unwrap_or_else(|| format!("candidate:{candidate}")),
    );
    let model = hash(model);
    let group = hash(group);
    let id = hash(&format!("{group}/{account}/{model}"));
    (
        id,
        Identity {
            group,
            account,
            model,
            last_used_at: 0,
        },
    )
}

fn clock() -> (i32, u8, u64) {
    let now = Local::now();
    (
        now.date_naive().num_days_from_ce(),
        now.hour() as u8,
        now.timestamp_millis().max(0) as u64,
    )
}

impl Collector {
    pub fn open(path: PathBuf) -> Result<Arc<Self>, String> {
        let mut data = match std::fs::File::open(&path) {
            Ok(file) => {
                if file.metadata().map_err(|_| STORAGE_ERROR)?.len() > MAX_FILE_BYTES {
                    return Err(STORAGE_ERROR.into());
                }
                let mut bytes = Vec::new();
                file.take(MAX_FILE_BYTES + 1)
                    .read_to_end(&mut bytes)
                    .map_err(|_| STORAGE_ERROR)?;
                if bytes.len() as u64 > MAX_FILE_BYTES {
                    return Err(STORAGE_ERROR.into());
                }
                let data: Store = serde_json::from_slice(&bytes).map_err(|_| STORAGE_ERROR)?;
                if !data.valid() {
                    return Err(STORAGE_ERROR.into());
                }
                data
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Store::default(),
            Err(_) => return Err(STORAGE_ERROR.into()),
        };
        data.trim(clock().0);
        Ok(Arc::new(Self {
            path,
            state: Mutex::new(State {
                durable_credits: data.credits.clone(),
                data,
                revision: 0,
                saved_revision: 0,
                saved_at: 0,
                storage_error: false,
            }),
            write: Mutex::new(()),
            active: AtomicBool::new(false),
            writer_started: AtomicBool::new(false),
            epoch: AtomicU64::new(0),
            wake: tokio::sync::Notify::new(),
            #[cfg(test)]
            writes: AtomicU64::new(0),
        }))
    }

    pub fn configure(&self, active: bool) {
        if self.active.swap(active, Ordering::SeqCst) != active {
            self.epoch.fetch_add(1, Ordering::SeqCst);
            if active {
                let state = self.state.lock().unwrap_or_else(|error| error.into_inner());
                if state.revision != state.saved_revision {
                    self.wake.notify_one();
                }
            }
        }
    }

    fn update(&self, epoch: u64, update: impl FnOnce(&mut Store, i32, u8, u64)) {
        if !self.active.load(Ordering::Relaxed) || self.epoch.load(Ordering::Relaxed) != epoch {
            return;
        }
        let (today, hour, now) = clock();
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        if !self.active.load(Ordering::Relaxed) || self.epoch.load(Ordering::Relaxed) != epoch {
            return;
        }
        state.data.trim(today);
        update(&mut state.data, today, hour, now);
        state.revision = state.revision.saturating_add(1);
        drop(state);
        self.wake.notify_one();
    }

    pub fn begin(self: &Arc<Self>, group: &str) -> Option<Arc<Request>> {
        if !self.active.load(Ordering::Relaxed) {
            return None;
        }
        let epoch = self.epoch.load(Ordering::SeqCst);
        let group = hash(group);
        let mut admitted = false;
        let mut started = (0, 0);
        self.update(epoch, |store, day, hour, now| {
            if !store.groups.contains(&group) && store.groups.len() >= MAX_GROUPS {
                let expired = store
                    .groups
                    .iter()
                    .find(|id| {
                        !store.days.values().any(|day| {
                            day.hours.values().any(|groups| groups.contains_key(*id))
                                || day.rows.keys().any(|row| {
                                    store
                                        .identities
                                        .get(row)
                                        .is_some_and(|identity| identity.group == **id)
                                })
                        })
                    })
                    .cloned();
                if let Some(expired) = expired {
                    store.groups.remove(&expired);
                    store
                        .identities
                        .retain(|_, identity| identity.group != expired);
                } else {
                    store.capped = Some(day);
                    return;
                }
            }
            store.groups.insert(group.clone());
            if store.since == 0 {
                store.since = now;
            }
            let totals = store
                .days
                .entry(day)
                .or_default()
                .hours
                .entry(hour)
                .or_default()
                .entry(group.clone())
                .or_default();
            totals.calls = totals.calls.saturating_add(1);
            admitted = true;
            started = (day, hour);
        });
        // Even capped requests need a handle tied to their original recording epoch.
        Some(Arc::new(Request {
            collector: self.clone(),
            epoch,
            admitted,
            group,
            day: started.0,
            hour: started.1,
            started: Instant::now(),
            state: Mutex::new(RequestState::default()),
        }))
    }

    pub fn snapshot(&self, days: u8, group: Option<&str>) -> Result<Snapshot, String> {
        if !matches!(days, 1 | 7 | 14) {
            return Err("Invalid statistics period".into());
        }
        let today = clock().0;
        let state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        let mut rows: BTreeMap<String, Counts> = BTreeMap::new();
        let mut totals = Totals::default();
        let mut trend = Vec::new();
        for (day, values) in state.data.days.range((today - i32::from(days) + 1)..=today) {
            for (id, counts) in &values.rows {
                if state
                    .data
                    .identities
                    .get(id)
                    .is_some_and(|identity| group.is_none_or(|group| group == identity.group))
                {
                    rows.entry(id.clone()).or_default().add(counts);
                }
            }
            for (hour, groups) in &values.hours {
                let mut hour_totals = Totals::default();
                for (id, counts) in groups {
                    if group.is_none_or(|group| group == id) {
                        hour_totals.add(counts);
                    }
                }
                totals.add(&hour_totals);
                trend.push(Trend {
                    day: *day,
                    hour: *hour,
                    totals: hour_totals,
                });
            }
        }
        // Keep the last recorded use even when its daily bucket has expired.
        let rows = state
            .data
            .identities
            .iter()
            .filter(|(_, identity)| group.is_none_or(|group| group == identity.group))
            .map(|(id, identity)| Row {
                id: id.clone(),
                identity: identity.clone(),
                counts: rows.remove(id).unwrap_or_default(),
            })
            .collect();
        Ok(Snapshot {
            recording: self.active.load(Ordering::Relaxed),
            since: state.data.since,
            saved_at: state.saved_at,
            storage_error: state.storage_error,
            capped: state
                .data
                .capped
                .is_some_and(|day| day > today - RETENTION_DAYS),
            today,
            totals,
            rows,
            trend,
        })
    }

    pub fn flush(&self) -> Result<(), String> {
        let _write = self.write.lock().unwrap_or_else(|error| error.into_inner());
        let (data, revision) = {
            let state = self.state.lock().unwrap_or_else(|error| error.into_inner());
            if state.revision == state.saved_revision {
                return Ok(());
            }
            (state.data.clone(), state.revision)
        };
        let result = serde_json::to_vec(&data)
            .map_err(|_| STORAGE_ERROR.to_string())
            .and_then(|bytes| {
                if bytes.len() as u64 > MAX_FILE_BYTES {
                    return Err(STORAGE_ERROR.into());
                }
                crate::services::native_account_files::write_private_file(&self.path, &bytes)
                    .map_err(|_| STORAGE_ERROR.to_string())
            });
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        state.storage_error = result.is_err();
        if result.is_ok() {
            state.durable_credits = data.credits;
            state.saved_revision = revision;
            state.saved_at = clock().2;
            #[cfg(test)]
            self.writes.fetch_add(1, Ordering::Relaxed);
        }
        result
    }

    pub fn token_credits(&self) -> Result<TokenCredits, String> {
        if !self.active.load(Ordering::Relaxed) {
            return Err("Developer preview is paused".into());
        }
        let state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        if state.storage_error {
            return Err(STORAGE_ERROR.into());
        }
        // Reads never force disk I/O or publish credits before the writer saves them.
        Ok(state.durable_credits.clone())
    }

    pub fn observe(
        self: &Arc<Self>,
        response: axum::response::Response,
        request: Option<&Arc<Request>>,
        candidate: &str,
    ) -> axum::response::Response {
        let Some(request) = request else {
            return response;
        };
        let epoch = request.epoch;
        if !self.active.load(Ordering::Relaxed) || self.epoch.load(Ordering::SeqCst) != epoch {
            return response;
        }
        let target = {
            let state = request
                .state
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            state
                .current
                .as_ref()
                .filter(|attempt| attempt.candidate == candidate)
                .map(|attempt| {
                    (
                        attempt.row.clone(),
                        attempt.day,
                        request.day,
                        request.hour,
                        request.group.clone(),
                    )
                })
        };
        let collector = self.clone();
        super::usage::observe(response, move |usage| {
            collector.update(epoch, |store, _, _, _| {
                store.credits.total = store.credits.total.saturating_add(usage.total_tokens);
                if let Some((row, row_day, day, hour, group)) = &target {
                    if let Some(counts) = store
                        .days
                        .get_mut(row_day)
                        .and_then(|day| day.rows.get_mut(row))
                    {
                        counts.usage.add(&usage);
                    }
                    if let Some(totals) = store
                        .days
                        .get_mut(day)
                        .and_then(|day| day.hours.get_mut(hour))
                        .and_then(|groups| groups.get_mut(group))
                    {
                        totals.usage.add(&usage);
                    }
                }
            });
        })
    }
}

fn global() -> Result<Arc<Collector>, String> {
    if let Some(collector) = COLLECTOR.get() {
        return Ok(collector.clone());
    }
    let collector = Collector::open(
        crate::utils::platform::echobird_dir().join("config/smart-router-usage.json"),
    )?;
    let _ = COLLECTOR.set(collector);
    Ok(COLLECTOR.get().unwrap().clone())
}

async fn writer_loop(writer: Arc<Collector>, delay: Duration) {
    loop {
        writer.wake.notified().await;
        tokio::time::sleep(delay).await;
        if !writer.active.load(Ordering::Relaxed) {
            continue;
        }
        let flush = writer.clone();
        let result = tokio::task::spawn_blocking(move || flush.flush()).await;
        if !matches!(result, Ok(Ok(()))) && writer.active.load(Ordering::Relaxed) {
            writer.wake.notify_one();
        }
    }
}

pub fn configure(active: bool) {
    ENABLED.store(active, Ordering::SeqCst);
    if active {
        if let Ok(collector) = global() {
            collector.configure(true);
            if !collector.writer_started.swap(true, Ordering::SeqCst) {
                tauri::async_runtime::spawn(writer_loop(collector.clone(), FLUSH_DELAY));
            }
        }
    } else if let Some(collector) = COLLECTOR.get() {
        collector.configure(false);
        let _ = collector.flush();
    }
}

pub fn begin(group: &str) -> Option<Arc<Request>> {
    if !ENABLED.load(Ordering::Relaxed) {
        return None;
    }
    COLLECTOR.get()?.begin(group)
}

pub fn snapshot(days: u8, group: Option<&str>) -> Result<Snapshot, String> {
    global()?.snapshot(days, group)
}
pub fn token_credits() -> Result<TokenCredits, String> {
    global()?.token_credits()
}

pub fn observe(
    response: axum::response::Response,
    request: Option<&Arc<Request>>,
    candidate: &str,
) -> axum::response::Response {
    if !ENABLED.load(Ordering::Relaxed) {
        return response;
    }
    match COLLECTOR.get() {
        Some(collector) => collector.observe(response, request, candidate),
        None => response,
    }
}

#[derive(Serialize)]
pub struct Group {
    pub id: String,
    pub name: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Source {
    pub id: String,
    pub group: String,
    pub account: String,
    pub model: String,
    pub name: String,
    pub model_name: String,
    pub provider: String,
    pub enabled: bool,
}

#[derive(Serialize)]
pub struct Catalog {
    pub groups: Vec<Group>,
    pub sources: Vec<Source>,
    pub capped: bool,
}

// Metadata is resolved on navigation/refresh and never persisted. This only
// reads EchoBird's configured models, not native accounts or upstream quotas.
pub fn catalog() -> Result<Catalog, String> {
    let models = crate::services::model_manager::get_user_models();
    let local = crate::services::local_llm::get_server_info_sync();
    let mut sources = BTreeMap::new();
    let mut groups = Vec::new();
    let mut capped = false;
    for id in super::profiles::ids() {
        if groups.len() >= MAX_GROUPS {
            capped = true;
            break;
        }
        let bytes = match std::fs::read(super::profiles::path_for(&id)) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(_) => return Err(STORAGE_ERROR.into()),
        };
        let config: super::StoredConfig =
            serde_json::from_slice(&bytes).map_err(|_| STORAGE_ERROR)?;
        let group = hash(&id);
        groups.push(Group {
            id: group.clone(),
            name: config.name.chars().take(80).collect(),
        });
        for candidate in &config.candidate_ids {
            let model = models.iter().find(|model| model.internal_id == *candidate);
            let (name, model_name, provider, account_source) = match model {
                Some(model) => {
                    let provider = model
                        .account_source
                        .as_ref()
                        .map(|source| {
                            serde_json::to_value(source.provider)
                                .ok()
                                .and_then(|value| value.as_str().map(str::to_owned))
                                .unwrap_or_default()
                        })
                        .unwrap_or_else(|| {
                            url::Url::parse(&model.base_url)
                                .ok()
                                .and_then(|url| url.host_str().map(str::to_owned))
                                .unwrap_or_else(|| "api".into())
                        });
                    (
                        model.name.clone(),
                        model
                            .model_id
                            .as_deref()
                            .unwrap_or_default()
                            .trim()
                            .to_owned(),
                        provider,
                        model.account_source.as_ref(),
                    )
                }
                None if candidate == "local-server" => (
                    local.model_name.clone(),
                    local.model_name.clone(),
                    "local".into(),
                    None,
                ),
                None => continue,
            };
            let (id, identity) = row_identity(&id, candidate, &model_name, account_source);
            if !sources.contains_key(&id) && sources.len() >= MAX_IDENTITIES {
                capped = true;
                continue;
            }
            sources.insert(
                id.clone(),
                Source {
                    id,
                    group: group.clone(),
                    account: identity.account,
                    model: identity.model,
                    name: name.chars().take(120).collect(),
                    model_name: model_name.chars().take(120).collect(),
                    provider,
                    enabled: config.enabled && !config.disabled_candidate_ids.contains(candidate),
                },
            );
        }
    }
    Ok(Catalog {
        groups,
        sources: sources.into_values().collect(),
        capped,
    })
}

pub fn shutdown() {
    ENABLED.store(false, Ordering::SeqCst);
    if let Some(collector) = COLLECTOR.get() {
        collector.configure(false);
        let collector = collector.clone();
        let (send, receive) = std::sync::mpsc::sync_channel(1);
        let _ = std::thread::Builder::new()
            .name("statistics-exit".into())
            .spawn(move || {
                let _ = send.send(collector.flush());
            });
        let _ = receive.recv_timeout(Duration::from_secs(2));
    }
}

#[derive(Default)]
struct RequestState {
    current: Option<Attempt>,
    protections: [u64; 5],
    attempts: u32,
    success: bool,
    duration_ms: u64,
    finished: bool,
}

struct Attempt {
    candidate: String,
    row: String,
    day: i32,
    done: bool,
}

pub struct Request {
    collector: Arc<Collector>,
    epoch: u64,
    admitted: bool,
    group: String,
    day: i32,
    hour: u8,
    started: Instant,
    state: Mutex<RequestState>,
}

impl Request {
    pub fn protections(&self, counts: [u64; 5]) {
        self.state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .protections = counts;
    }

    pub fn attempt(
        &self,
        group: &str,
        candidate: &str,
        model: &str,
        source: Option<&AccountSource>,
    ) {
        if !self.admitted {
            return;
        }
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        if state.finished {
            return;
        }
        if let Some(previous) = state.current.as_mut().filter(|attempt| !attempt.done) {
            self.end_attempt(previous, Some(5), false);
        }
        let (id, mut identity) = row_identity(group, candidate, model, source);
        let mut started_day = None;
        self.collector.update(self.epoch, |store, day, _, now| {
            if !store.identities.contains_key(&id) && store.identities.len() >= MAX_IDENTITIES {
                let expired = store
                    .identities
                    .iter()
                    .find(|(id, _)| !store.days.values().any(|day| day.rows.contains_key(*id)))
                    .map(|(id, _)| id.clone());
                if let Some(expired) = expired {
                    store.identities.remove(&expired);
                } else {
                    store.capped = Some(day);
                    return;
                }
            }
            identity.last_used_at = now;
            store.identities.insert(id.clone(), identity);
            let row = store
                .days
                .entry(day)
                .or_default()
                .rows
                .entry(id.clone())
                .or_default();
            row.attempts = row.attempts.saturating_add(1);
            for (target, value) in row.protections.iter_mut().zip(state.protections) {
                *target = target.saturating_add(value);
            }
            started_day = Some(day);
        });
        state.attempts = state.attempts.saturating_add(1);
        state.success = false;
        state.current = Some(Attempt {
            candidate: candidate.into(),
            row: id,
            day: started_day.unwrap_or_else(|| clock().0),
            done: false,
        });
    }

    fn end_attempt(&self, attempt: &mut Attempt, failure: Option<usize>, cancelled: bool) {
        if attempt.done {
            return;
        }
        attempt.done = true;
        self.collector.update(self.epoch, |store, _, _, _| {
            let Some(row) = store
                .days
                .get_mut(&attempt.day)
                .and_then(|day| day.rows.get_mut(&attempt.row))
            else {
                return;
            };
            if cancelled {
                row.cancelled = row.cancelled.saturating_add(1);
            } else if let Some(failure) = failure {
                row.failed = row.failed.saturating_add(1);
                row.errors[failure] = row.errors[failure].saturating_add(1);
            } else {
                row.completed = row.completed.saturating_add(1);
            }
        });
    }

    pub fn outcome(&self, candidate: &str, failure: Option<usize>) {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        if state.finished {
            return;
        }
        if let Some(attempt) = state
            .current
            .as_mut()
            .filter(|attempt| attempt.candidate == candidate)
        {
            if !attempt.done {
                self.end_attempt(attempt, failure, false);
                state.success = failure.is_none();
                if state.success {
                    state.duration_ms =
                        self.started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;
                }
            }
        }
    }

    pub fn settled(&self) -> bool {
        self.state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .current
            .as_ref()
            .is_some_and(|attempt| attempt.done)
    }

    pub fn response_ready(&self) {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        if state.success {
            state.duration_ms = self.started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;
        }
    }

    pub fn finish(&self, response_ok: bool, cancelled: bool) {
        if !self.admitted {
            return;
        }
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        if state.finished {
            return;
        }
        state.finished = true;
        if let Some(attempt) = state.current.as_mut().filter(|attempt| !attempt.done) {
            self.end_attempt(attempt, Some(5), cancelled);
        } else if !response_ok && !cancelled && state.success {
            // Response restoration can fail after the upstream has completed.
            if let Some(attempt) = &state.current {
                self.collector.update(self.epoch, |store, _, _, _| {
                    if let Some(row) = store
                        .days
                        .get_mut(&attempt.day)
                        .and_then(|day| day.rows.get_mut(&attempt.row))
                    {
                        row.completed = row.completed.saturating_sub(1);
                        row.failed = row.failed.saturating_add(1);
                        row.errors[5] = row.errors[5].saturating_add(1);
                    }
                });
            }
        }
        let success = response_ok && state.success && !cancelled;
        self.collector.update(self.epoch, |store, _, _, _| {
            let Some(day) = store.days.get_mut(&self.day) else {
                return;
            };
            let totals = day
                .hours
                .entry(self.hour)
                .or_default()
                .entry(self.group.clone())
                .or_default();
            if cancelled {
                totals.cancelled = totals.cancelled.saturating_add(1);
            } else if success {
                totals.completed = totals.completed.saturating_add(1);
                totals.rescued = totals.rescued.saturating_add(u64::from(state.attempts > 1));
                totals.duration_ms = totals.duration_ms.saturating_add(state.duration_ms);
            } else {
                totals.failed = totals.failed.saturating_add(1);
            }
        });
    }
}

pub struct Completion {
    pub request: Arc<Request>,
    pub response_ok: bool,
    pub response_ready: bool,
    pub done: bool,
}
impl Completion {
    pub fn error(&mut self) {
        self.request.finish(false, false);
        self.done = true;
    }
}
impl Drop for Completion {
    fn drop(&mut self) {
        if !self.done {
            if self.response_ready && self.request.settled() {
                self.request.finish(self.response_ok, false);
            } else {
                self.request.finish(false, true);
            }
        }
    }
}

#[cfg(test)]
#[path = "statistics_tests.rs"]
mod tests;
