#[cfg(windows)]
mod alt_release;
mod app_icon;
mod app_icon_cache;
mod audio;
pub mod config;
mod dictation_target;
pub(crate) mod island_input;
mod monitor;
mod native;
#[cfg(windows)]
mod paste_windows;
mod pipeline;
pub mod selection;
mod selection_gesture;
mod selection_native;
mod selection_pointer;
mod shortcut;
mod speech_api;
#[cfg(test)]
mod speech_api_tests;
mod streaming;
#[cfg(target_os = "linux")]
mod wayland;
#[cfg(any(target_os = "linux", test))]
mod wayland_state;

use audio::Recorder;
use axum::body::Bytes;
use config::{Config, Saved};
use island_input::{WINDOW_HEIGHT as ISLAND_HEIGHT, WINDOW_WIDTH as ISLAND_WIDTH};
use serde::Serialize;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};
use tokio_util::sync::CancellationToken;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub origin: String,
    pub config: Config,
    pub days: std::collections::BTreeMap<String, u64>,
    pub usage: config::Usage,
    pub phase: String,
    pub active_channel: Option<usize>,
    pub level: f32,
    pub elapsed_ms: u64,
    pub text: String,
    pub result_audio_id: Option<u64>,
    pub error: Option<String>,
    pub revision: u64,
    pub shortcut_active: bool,
    pub shortcut_description: Option<String>,
    pub app_icon: Option<String>,
    pub app_icon_loading: bool,
    pub selection_open: bool,
}

struct Runtime {
    origin: String,
    phase: String,
    active_channel: Option<usize>,
    level: f32,
    started: Option<Instant>,
    elapsed_ms: u64,
    text: String,
    result_audio: Option<(u64, Bytes)>,
    error: Option<String>,
    revision: u64,
    generation: u64,
    cancel: CancellationToken,
    config: Config,
    insert: bool,
    target: Option<dictation_target::Target>,
    shortcut_active: bool,
    shortcut_description: Option<String>,
    app_icon: Option<String>,
    app_icon_loading: bool,
    app_icon_generation: u64,
}

impl Runtime {
    fn finish_audio(&mut self, generation: u64, audio: Option<Bytes>) -> bool {
        if generation != self.generation || self.cancel.is_cancelled() {
            return false;
        }
        if self.origin != "test" {
            self.result_audio = audio.map(|bytes| (generation, bytes));
        }
        true
    }

    fn replay_audio(&self, id: u64) -> Result<Bytes, String> {
        if self.active() {
            return Err("voice.error.busy".into());
        }
        self.result_audio
            .as_ref()
            .filter(|(current, _)| *current == id)
            .map(|(_, bytes)| bytes.clone())
            .ok_or_else(|| "voice.error.noAudio".into())
    }

    fn active_channel(&self) -> Option<usize> {
        if matches!(self.phase.as_str(), "recording" | "transcribing")
            && !self.cancel.is_cancelled()
        {
            self.active_channel
        } else {
            None
        }
    }

    fn begin_app_icon(&mut self, loading: bool) -> u64 {
        self.app_icon_generation += 1;
        self.app_icon_loading = loading;
        if !loading {
            self.app_icon = None;
        }
        self.app_icon_generation
    }

    fn finish_app_icon(&mut self, generation: u64, icon: Option<String>) -> bool {
        if generation != self.app_icon_generation {
            return false;
        }
        self.app_icon = icon;
        self.app_icon_loading = false;
        true
    }

    fn cancel(&mut self, expected: Option<u64>) -> bool {
        if expected.is_some_and(|generation| {
            generation != self.generation || !self.active() || self.origin == "test"
        }) {
            return false;
        }
        self.generation += 1;
        self.cancel.cancel();
        self.target = None;
        self.phase = "idle".into();
        self.started = None;
        self.level = 0.0;
        true
    }

    fn active(&self) -> bool {
        matches!(
            self.phase.as_str(),
            "starting" | "recording" | "testing" | "transcribing" | "optimizing" | "inserting"
        )
    }
}

pub struct Voice {
    path: PathBuf,
    saved: Mutex<Saved>,
    runtime: Mutex<Runtime>,
    recorder: Recorder,
    stream: Mutex<Option<streaming::Session>>,
    operation: tokio::sync::Mutex<()>,
    registration: tokio::sync::Mutex<Option<shortcut::Registration>>,
    suspended: AtomicBool,
    held: AtomicBool,
    stopped: AtomicBool,
    selection: Mutex<selection::State>,
}

fn shared(app: &AppHandle) -> Arc<Voice> {
    app.state::<Arc<Voice>>().inner().clone()
}

pub fn init(app: &AppHandle) {
    let path = crate::utils::platform::echobird_dir().join("config/voice-input.json");
    let (saved, error) = match config::load(&path) {
        Ok(saved) => (saved, None),
        Err(error) => (Saved::default(), Some(error)),
    };
    let config = saved.config.clone();
    app.manage(Arc::new(Voice {
        path,
        saved: Mutex::new(saved),
        runtime: Mutex::new(Runtime {
            origin: "page".into(),
            phase: "idle".into(),
            active_channel: None,
            level: 0.0,
            started: None,
            elapsed_ms: 0,
            text: String::new(),
            result_audio: None,
            error,
            revision: 0,
            generation: 0,
            cancel: CancellationToken::new(),
            config: config.clone(),
            insert: false,
            target: None,
            shortcut_active: false,
            shortcut_description: None,
            app_icon: None,
            app_icon_loading: false,
            app_icon_generation: 0,
        }),
        recorder: Recorder::new(),
        stream: Mutex::new(None),
        operation: tokio::sync::Mutex::new(()),
        registration: tokio::sync::Mutex::new(None),
        suspended: AtomicBool::new(false),
        held: AtomicBool::new(false),
        stopped: AtomicBool::new(false),
        selection: Mutex::new(selection::State::default()),
    }));
    monitor::start(app);
    selection::start(app);
    if island_visible(&config) {
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            let voice = shared(&app);
            let _operation = voice.operation.lock().await;
            if config.hotkey_active() {
                match shortcut::Registration::create(&app, &config.shortcut).await {
                    Ok(registration) => {
                        {
                            let mut runtime = voice.runtime.lock().unwrap();
                            runtime.shortcut_active = true;
                            runtime.shortcut_description = registration.description.clone();
                        }
                        *voice.registration.lock().await = Some(registration);
                    }
                    Err(error) => voice.runtime.lock().unwrap().error = Some(error),
                }
            }
            if let Err(error) = show_island(&app) {
                voice.runtime.lock().unwrap().error.get_or_insert(error);
            }
            publish(&app);
        });
    }
}

pub fn snapshot(app: &AppHandle) -> Snapshot {
    let voice = shared(app);
    let saved = voice.saved.lock().unwrap().clone();
    let runtime = voice.runtime.lock().unwrap();
    let selection = voice.selection.lock().unwrap();
    Snapshot {
        origin: runtime.origin.clone(),
        config: saved.config,
        days: saved.days,
        usage: saved.usage,
        phase: runtime.phase.clone(),
        active_channel: runtime.active_channel(),
        level: runtime.level,
        elapsed_ms: runtime
            .started
            .map(|time| time.elapsed().as_millis() as u64)
            .unwrap_or(runtime.elapsed_ms),
        text: runtime.text.clone(),
        result_audio_id: runtime.result_audio.as_ref().map(|(id, _)| *id),
        error: runtime.error.clone(),
        revision: runtime.revision,
        shortcut_active: runtime.shortcut_active,
        shortcut_description: runtime.shortcut_description.clone(),
        app_icon: runtime.app_icon.clone(),
        app_icon_loading: runtime.app_icon_loading,
        selection_open: selection.open,
    }
}

fn publish(app: &AppHandle) {
    shared(app).runtime.lock().unwrap().revision += 1;
    let snapshot = snapshot(app);
    let _ = app.emit_to("main", "voice-input-state", &snapshot);
    let _ = app.emit_to("voice-island", "voice-input-state", &snapshot);
}

pub fn devices() -> Result<Vec<String>, String> {
    audio::devices()
}

pub async fn save(app: &AppHandle, mut config: Config) -> Result<Snapshot, String> {
    config.normalize_optimization_mode();
    config.disable_without_channels();
    config.validate()?;
    if !native::modifier(&config.shortcut) {
        shortcut::parse(&config.shortcut)?;
    }
    let voice = shared(app);
    let _operation = voice.operation.lock().await;
    if voice.runtime.lock().unwrap().active() {
        return Err("voice.error.busy".into());
    }
    let mut current = voice.registration.lock().await;
    let same = config.hotkey_active()
        && current
            .as_ref()
            .is_some_and(|registration| registration.binding == config.shortcut);
    let replacement = if config.hotkey_active() && !same {
        Some(shortcut::Registration::create(app, &config.shortcut).await?)
    } else {
        None
    };
    let mut saved = voice.saved.lock().unwrap().clone();
    let mode_changed = saved.config.hotkey_mode != config.hotkey_mode;
    saved.config = config.clone();
    if let Err(error) = config::save(&voice.path, &saved) {
        if let Some(registration) = &replacement {
            registration.close(app);
        }
        return Err(error);
    }
    if !same || mode_changed {
        voice.held.store(false, Ordering::Release);
    }
    if !same {
        if let Some(previous) = current.take() {
            previous.close(app);
        }
        *current = replacement;
    }
    {
        let mut runtime = voice.runtime.lock().unwrap();
        runtime.shortcut_active = current.is_some();
        runtime.shortcut_description = current
            .as_ref()
            .and_then(|registration| registration.description.clone());
        runtime.error = None;
    }
    *voice.saved.lock().unwrap() = saved;
    selection::close(app);
    if island_visible(&config) {
        if let Err(error) = show_island(app) {
            voice.runtime.lock().unwrap().error = Some(error);
        }
    } else if let Some(window) = app.get_webview_window("voice-island") {
        let _ = window.hide();
    }
    publish(app);
    Ok(snapshot(app))
}

pub fn suspend(app: &AppHandle, suspended: bool) {
    let voice = shared(app);
    voice.suspended.store(suspended, Ordering::Release);
    voice.held.store(false, Ordering::Release);
}

pub async fn begin(app: &AppHandle, origin: &str) -> Result<Snapshot, String> {
    let test = origin == "test";
    let voice = shared(app);
    let _operation = voice.operation.lock().await;
    let config = voice.saved.lock().unwrap().config.clone();
    if !test && !config.shortcut_enabled {
        return Ok(snapshot(app));
    }
    if !test && config.channels.is_empty() {
        return Err("voice.error.noChannels".into());
    }
    if voice.selection.lock().unwrap().open {
        return Err("voice.error.busy".into());
    }
    let insert = !test
        && !app
            .get_webview_window("main")
            .is_some_and(|window| window.is_focused().unwrap_or(false));
    let foreground = if insert { native::input_target() } else { None };
    let generation = {
        let mut runtime = voice.runtime.lock().unwrap();
        if runtime.active() {
            return Err("voice.error.busy".into());
        }
        runtime.generation += 1;
        runtime.cancel.cancel();
        runtime.cancel = CancellationToken::new();
        runtime.phase = "starting".into();
        runtime.origin = origin.into();
        runtime.text.clear();
        runtime.error = None;
        runtime.level = 0.0;
        runtime.elapsed_ms = 0;
        runtime.config = config.clone();
        runtime.active_channel = None;
        runtime.target = None;
        runtime.insert = insert;
        runtime.generation
    };
    if test && !island_visible(&config) {
        if let Some(window) = app.get_webview_window("voice-island") {
            let _ = window.hide();
        }
    }
    publish(app);
    // Capture accessibility state alongside microphone startup, so a slow
    // provider does not discard the first words spoken after clicking Record.
    let target_task = insert.then(|| {
        tauri::async_runtime::spawn_blocking(move || dictation_target::capture(foreground))
    });
    let level_app = app.clone();
    let result = {
        let voice = voice.clone();
        tauri::async_runtime::spawn_blocking(move || {
            voice.recorder.start(
                config.microphone,
                !test,
                Arc::new(move |level| {
                    let voice = shared(&level_app);
                    if level < 0.0 {
                        if voice.runtime.lock().unwrap().generation != generation {
                            return;
                        }
                        voice.recorder.cancel();
                        fail(
                            &level_app,
                            generation,
                            "voice.error.microphone".into(),
                            None,
                        );
                        return;
                    }
                    {
                        let mut runtime = voice.runtime.lock().unwrap();
                        if runtime.generation != generation
                            || !matches!(
                                runtime.phase.as_str(),
                                "starting" | "recording" | "testing"
                            )
                        {
                            return;
                        }
                        runtime.level = level;
                    }
                    publish(&level_app);
                }),
            )
        })
        .await
        .map_err(|_| "voice.error.microphone".to_string())?
    };
    if let Err(error) = result {
        voice.recorder.cancel();
        fail(app, generation, error.clone(), None);
        return Err(error);
    }
    let recorded_at = Instant::now();
    let target = match target_task {
        Some(task) => task.await.unwrap_or(None),
        None => None,
    };
    {
        let mut runtime = voice.runtime.lock().unwrap();
        if runtime.phase == "error" {
            return Err("voice.error.microphone".into());
        }
        if runtime.generation != generation || runtime.cancel.is_cancelled() {
            drop(runtime);
            voice.recorder.cancel();
            return Ok(snapshot(app));
        }
        runtime.target = target;
        if insert {
            log::info!(
                "[VoiceInput] start origin={} insert={} target_present={}",
                runtime.origin,
                runtime.insert,
                runtime.target.is_some()
            );
        }
        runtime.phase = if test {
            "testing".into()
        } else {
            "recording".into()
        };
        runtime.started = Some(recorded_at);
    }
    if !test {
        if let Some(channel) = config
            .channels
            .first()
            .filter(|channel| channel.kind == "workbuddy")
        {
            let cancel = voice.runtime.lock().unwrap().cancel.clone();
            *voice.stream.lock().unwrap() = Some(streaming::Session::start(
                channel.id.clone(),
                voice.recorder.clone(),
                cancel,
            ));
            voice.runtime.lock().unwrap().active_channel = Some(0);
        }
    }
    if !test {
        if let Err(error) = show_island(app) {
            shared(app).runtime.lock().unwrap().error = Some(error);
        }
    }
    publish(app);
    let timer_app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_secs(audio::MAX_SECONDS)).await;
        let runtime = snapshot(&timer_app);
        if shared(&timer_app).runtime.lock().unwrap().generation == generation
            && matches!(runtime.phase.as_str(), "recording" | "testing")
        {
            let _ = stop(&timer_app).await;
        }
    });
    Ok(snapshot(app))
}

fn fail(app: &AppHandle, generation: u64, error: String, audio: Option<Bytes>) {
    let voice = shared(app);
    {
        let mut runtime = voice.runtime.lock().unwrap();
        if !runtime.finish_audio(generation, audio) {
            return;
        }
        log::warn!("[VoiceInput] failed phase={} code={error}", runtime.phase);
        runtime.cancel.cancel();
        runtime.target = None;
        runtime.phase = "error".into();
        runtime.error = Some(error);
        runtime.level = 0.0;
        runtime.started = None;
    }
    voice.stream.lock().unwrap().take();
    publish(app);
}

pub async fn stop(app: &AppHandle) -> Result<Snapshot, String> {
    stop_capture(app, false).await
}

async fn stop_capture(app: &AppHandle, shortcut_only: bool) -> Result<Snapshot, String> {
    let voice = shared(app);
    let operation = voice.operation.lock().await;
    let (generation, config, cancel, test) = {
        let mut runtime = voice.runtime.lock().unwrap();
        if shortcut_only && runtime.origin != "shortcut" {
            return Ok(snapshot_after_unlock(app, runtime));
        }
        if !matches!(runtime.phase.as_str(), "recording" | "testing") {
            return Ok(snapshot_after_unlock(app, runtime));
        }
        let test = runtime.phase == "testing";
        runtime.elapsed_ms = runtime
            .started
            .take()
            .map(|time| time.elapsed().as_millis() as u64)
            .unwrap_or(0);
        runtime.phase = if test {
            "idle".into()
        } else {
            "transcribing".into()
        };
        runtime.level = 0.0;
        (
            runtime.generation,
            runtime.config.clone(),
            runtime.cancel.clone(),
            test,
        )
    };
    if test {
        {
            let mut runtime = voice.runtime.lock().unwrap();
            runtime.generation += 1;
            runtime.cancel.cancel();
            runtime.target = None;
        }
        voice.recorder.cancel();
        publish(app);
        return Ok(snapshot(app));
    }
    let captured = {
        let voice = voice.clone();
        tauri::async_runtime::spawn_blocking(move || voice.recorder.stop())
            .await
            .map_err(|_| "voice.error.microphone".to_string())?
    };
    drop(operation);
    publish(app);
    let audio = match captured {
        Ok(audio) => audio,
        Err(error) => {
            fail(app, generation, error, None);
            return Ok(snapshot(app));
        }
    };
    let peak = audio.wav[44..]
        .chunks_exact(2)
        .map(|sample| i16::from_le_bytes([sample[0], sample[1]]).unsigned_abs())
        .max()
        .unwrap_or(0);
    log::info!(
        "[VoiceInput] captured duration_ms={} wav_bytes={} raw_peak_pcm16={} peak_pcm16={peak}",
        audio.duration_ms,
        audio.wav.len(),
        (audio.raw_peak * i16::MAX as f32) as u16
    );
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .build()
        .map_err(|_| "voice.error.transcribe")?;
    let bytes = axum::body::Bytes::from(audio.wav.clone());
    let client_ref = &client;
    let audio_ref = &bytes;
    let api_options = &config.api_options;
    let channels = &config.channels;
    let runtime_voice = &voice;
    let stream = voice.stream.lock().unwrap().take();
    let streamed = if let Some(stream) = stream {
        Some(tokio::select! {
            biased;
            _ = cancel.cancelled() => return Ok(snapshot(app)),
            result = stream.finish(&audio.wav) => result,
        })
    } else {
        None
    };
    let recognized = tokio::select! {
        biased;
        _ = cancel.cancelled() => return Ok(snapshot(app)),
        result = pipeline::first_transcript(channels, streamed, |channel| async move {
            let selected = {
                let mut runtime = runtime_voice.runtime.lock().unwrap();
                if runtime.generation == generation && !runtime.cancel.is_cancelled() {
                    runtime.active_channel = channels.iter().position(|candidate| candidate == &channel);
                    true
                } else {
                    false
                }
            };
            if selected { publish(app); }
            tokio::time::timeout(pipeline::recognition_timeout(&channel, audio_ref), pipeline::recognize(&channel, api_options, audio_ref, client_ref))
                .await.map_err(|_| "voice.error.transcribe".to_string())?
        }) => result,
    };
    let mut text = match recognized {
        Ok(text) => text,
        Err(error) => {
            fail(app, generation, error, Some(bytes));
            return Ok(snapshot(app));
        }
    };
    log::info!(
        "[VoiceInput] recognized characters={}",
        text.chars().count()
    );
    {
        let mut runtime = voice.runtime.lock().unwrap();
        if runtime.generation != generation || cancel.is_cancelled() {
            return Ok(snapshot_after_unlock(app, runtime));
        }
        runtime.active_channel = None;
    }
    publish(app);
    let mut warning = None;
    let extensions = app.try_state::<crate::services::plugin_runtime::PluginRuntime>();
    if let Some(extensions) = &extensions {
        match tokio::select! {
            _=cancel.cancelled()=>return Ok(snapshot(app)),
            result=extensions.process_text(app,"voice.transcribed",&text)=>result,
        } {
            Ok(processed) => text = processed,
            Err(_) => warning = Some("voice.error.plugin".into()),
        }
    }
    if !text.trim().is_empty() && config.optimization_enabled {
        {
            let mut runtime = voice.runtime.lock().unwrap();
            if runtime.generation != generation {
                return Ok(snapshot_after_unlock(app, runtime));
            }
            runtime.phase = "optimizing".into();
        }
        publish(app);
        match tokio::select! {
            _ = cancel.cancelled() => return Ok(snapshot(app)),
            result = tokio::time::timeout(Duration::from_secs(45), pipeline::optimize(&config, &text, &audio)) => result,
        } {
            Ok(Ok(optimized)) => text = optimized,
            _ => warning = Some("voice.error.optimization".into()),
        }
    }
    if let Some(extensions) = &extensions {
        match tokio::select! {
            _=cancel.cancelled()=>return Ok(snapshot(app)),
            result=extensions.process_text(app,"voice.output",&text)=>result,
        } {
            Ok(processed) => text = processed,
            Err(_) => warning = Some("voice.error.plugin".into()),
        }
    }
    let (insert, target) = {
        let mut runtime = voice.runtime.lock().unwrap();
        if runtime.generation != generation || cancel.is_cancelled() {
            return Ok(snapshot_after_unlock(app, runtime));
        }
        runtime.text = text.clone();
        runtime.phase = "inserting".into();
        (runtime.insert, runtime.target.take())
    };
    if insert && !text.trim().is_empty() {
        let result = {
            let text = text.clone();
            let cancel = cancel.clone();
            tauri::async_runtime::spawn_blocking(move || {
                if cancel.is_cancelled() {
                    Err("voice.error.cancelled".into())
                } else {
                    dictation_target::insert(&text, target, &cancel)
                }
            })
            .await
        };
        match result {
            Ok(Err(error)) => {
                log::warn!("[VoiceInput] input failed code={error}");
                warning = Some(error);
            }
            Err(_) => {
                log::warn!("[VoiceInput] input worker failed");
                warning = Some("voice.error.insert".into());
            }
            _ => log::info!("[VoiceInput] text input dispatched"),
        }
    } else {
        log::info!("[VoiceInput] text input skipped insert_enabled={insert}");
    }
    let _completion = voice.operation.lock().await;
    {
        let mut runtime = voice.runtime.lock().unwrap();
        if runtime.generation != generation || cancel.is_cancelled() {
            return Ok(snapshot_after_unlock(app, runtime));
        }
        runtime.phase = "done".into();
        runtime.finish_audio(generation, Some(bytes));
        runtime.elapsed_ms = audio.duration_ms;
        runtime.error = warning;
    }
    if !text.trim().is_empty() {
        let mut saved = voice.saved.lock().unwrap();
        let date = chrono::Local::now().format("%Y-%m-%d").to_string();
        let characters = text
            .chars()
            .filter(|character| !character.is_whitespace())
            .count() as u64;
        *saved.days.entry(date).or_default() += characters;
        saved.usage.record(characters, audio.duration_ms);
        if config::save(&voice.path, &saved).is_err() {
            voice.runtime.lock().unwrap().error = Some("voice.error.save".into());
        }
    }
    publish(app);
    Ok(snapshot(app))
}

fn snapshot_after_unlock(app: &AppHandle, runtime: std::sync::MutexGuard<'_, Runtime>) -> Snapshot {
    drop(runtime);
    snapshot(app)
}

pub async fn cancel(app: &AppHandle) -> Snapshot {
    cancel_generation(app, None).await
}

async fn cancel_generation(app: &AppHandle, expected: Option<u64>) -> Snapshot {
    let voice = shared(app);
    let _operation = voice.operation.lock().await;
    {
        let mut runtime = voice.runtime.lock().unwrap();
        if !runtime.cancel(expected) {
            return snapshot_after_unlock(app, runtime);
        }
    }
    voice.recorder.cancel();
    voice.stream.lock().unwrap().take();
    if !island_visible(&voice.saved.lock().unwrap().config) {
        if let Some(window) = app.get_webview_window("voice-island") {
            let _ = window.hide();
        }
    }
    publish(app);
    snapshot(app)
}

fn escape(app: &AppHandle) {
    let voice = shared(app);
    if voice.suspended.load(Ordering::Acquire) {
        return;
    }
    let generation = voice.runtime.lock().unwrap().generation;
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        cancel_generation(&app, Some(generation)).await;
    });
}

#[derive(Debug, PartialEq, Eq)]
enum ShortcutAction {
    Start,
    Stop,
}

fn shortcut_action(mode: &str, phase: &str, origin: &str, down: bool) -> Option<ShortcutAction> {
    if mode == "toggle" && down {
        return None;
    }
    if origin == "shortcut"
        && !down
        && (phase == "recording" || (mode == "toggle" && phase == "starting"))
    {
        Some(ShortcutAction::Stop)
    } else if (down || mode == "toggle")
        && !matches!(
            phase,
            "starting" | "recording" | "testing" | "transcribing" | "optimizing" | "inserting"
        )
    {
        Some(ShortcutAction::Start)
    } else {
        None
    }
}

pub fn trigger(app: &AppHandle, binding: &config::Binding, down: bool) {
    let voice = shared(app);
    let config = voice.saved.lock().unwrap().config.clone();
    if config.shortcut != *binding {
        return;
    }
    if voice.selection.lock().unwrap().open {
        voice.held.store(false, Ordering::Release);
        return;
    }
    if voice.suspended.load(Ordering::Acquire) {
        return;
    }
    if down == voice.held.swap(down, Ordering::AcqRel) {
        return;
    }
    if !config.hotkey_active() {
        return;
    }
    let action = {
        let runtime = voice.runtime.lock().unwrap();
        shortcut_action(&config.hotkey_mode, &runtime.phase, &runtime.origin, down)
    };
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let result = match action {
            Some(ShortcutAction::Stop) => stop_capture(&app, true).await,
            Some(ShortcutAction::Start) => match begin(&app, "shortcut").await {
                Ok(_)
                    if config.hotkey_mode == "hold"
                        && !shared(&app).held.load(Ordering::Acquire) =>
                {
                    stop_capture(&app, true).await
                }
                result => result,
            },
            None => return,
        };
        if let Err(error) = result {
            if error == "voice.error.busy" {
                return;
            }
            let generation = shared(&app).runtime.lock().unwrap().generation;
            fail(&app, generation, error, None);
            let _ = show_island(&app);
        }
    });
}

fn island_visible(config: &Config) -> bool {
    config.shortcut_enabled && !config.channels.is_empty()
}

fn island_position(
    area: &tauri::PhysicalRect<i32, u32>,
    scale: f64,
) -> tauri::PhysicalPosition<i32> {
    tauri::PhysicalPosition::new(
        (f64::from(area.position.x) + (f64::from(area.size.width) - ISLAND_WIDTH * scale) / 2.0)
            .round() as i32,
        (f64::from(area.position.y)
            + (f64::from(area.size.height) - (ISLAND_HEIGHT + island_input::BOTTOM_GAP) * scale)
                .max(0.0))
        .round() as i32,
    )
}

fn show_island(app: &AppHandle) -> Result<(), String> {
    let status = snapshot(app);
    if !island_visible(&status.config) {
        return Ok(());
    }
    if let Some(window) = app.get_webview_window("voice-island") {
        return window.show().map_err(|_| "voice.error.island".into());
    }
    let window = tauri::WebviewWindowBuilder::new(
        app,
        "voice-island",
        tauri::WebviewUrl::App("index.html".into()),
    )
    .title("EchoBird")
    .inner_size(ISLAND_WIDTH, ISLAND_HEIGHT)
    .decorations(false)
    .transparent(true)
    .shadow(false)
    .always_on_top(true)
    .skip_taskbar(true)
    .resizable(false)
    .focusable(false)
    .focused(false)
    .visible(false)
    .build()
    .map_err(|_| "voice.error.island")?;
    island_input::configure(&window).map_err(|_| "voice.error.island")?;
    let moved_app = app.clone();
    window.on_window_event(move |event| {
        if matches!(event, tauri::WindowEvent::Moved(_)) {
            selection::close(&moved_app);
        }
    });
    if let Some(monitor) = app
        .get_webview_window("main")
        .and_then(|main| main.current_monitor().ok().flatten())
        .or_else(|| window.current_monitor().ok().flatten())
    {
        let _ = window.set_position(island_position(monitor.work_area(), monitor.scale_factor()));
    }
    Ok(())
}

pub fn island_ready(app: &AppHandle) -> Snapshot {
    let status = snapshot(app);
    if island_visible(&status.config) {
        if let Some(window) = app.get_webview_window("voice-island") {
            let _ = window.show();
        }
    }
    status
}

pub fn replay_audio(app: &AppHandle, id: u64) -> Result<tauri::ipc::Response, String> {
    let voice = shared(app);
    let bytes = voice.runtime.lock().unwrap().replay_audio(id)?;
    Ok(tauri::ipc::Response::new(bytes.to_vec()))
}

pub fn shutdown(app: &AppHandle) {
    let voice = shared(app);
    voice.stopped.store(true, Ordering::Release);
    selection::close(app);
    {
        let mut runtime = voice.runtime.lock().unwrap();
        runtime.cancel.cancel();
        runtime.target = None;
        runtime.result_audio = None;
    }
    voice.recorder.cancel();
    if let Ok(mut registration) = voice.registration.try_lock() {
        if let Some(registration) = registration.take() {
            registration.close(app);
        }
    };
}

#[cfg(test)]
mod tests;
