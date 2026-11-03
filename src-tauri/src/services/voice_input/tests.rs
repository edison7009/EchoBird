use super::*;

#[test]
fn island_defaults_to_bottom_center_of_the_work_area_across_monitors_and_dpi() {
    for (x, y, width, height, scale) in [
        (0, 0, 1920, 1040, 1.0),
        (48, 40, 1872, 1000, 1.0),
        (-1920, 80, 1920, 1000, 1.5),
        (2560, -1440, 2560, 1360, 2.0),
        (0, 0, 1920, 1030, 1.25),
    ] {
        let area = tauri::PhysicalRect {
            position: tauri::PhysicalPosition::new(x, y),
            size: tauri::PhysicalSize::new(width, height),
        };
        let position = island_position(&area, scale, 1.0);
        let center = f64::from(position.x)
            + (island_input::BODY_LEFT + island_input::BODY_WIDTH / 2.0) * scale;
        assert!((center - (f64::from(x) + f64::from(width) / 2.0)).abs() <= 0.5);
        let bottom =
            f64::from(position.y) + (island_input::BODY_TOP + island_input::BODY_HEIGHT) * scale;
        assert!((f64::from(y) + f64::from(height) - bottom - 56.0 * scale).abs() <= 0.5);
    }
    let tiny = tauri::PhysicalRect {
        position: tauri::PhysicalPosition::new(-10, -20),
        size: tauri::PhysicalSize::new(320, 100),
    };
    assert_eq!(island_position(&tiny, 1.0, 1.0).y, -20);
}

fn cancel_runtime(phase: &str, origin: &str) -> Runtime {
    Runtime {
        origin: origin.into(),
        phase: phase.into(),
        active_channel: None,
        level: 0.7,
        started: Some(Instant::now()),
        elapsed_ms: 0,
        text: String::new(),
        result_audio: None,
        error: None,
        revision: 0,
        generation: 7,
        cancel: CancellationToken::new(),
        config: Config::default(),
        insert: false,
        target: None,
        shortcut_active: true,
        shortcut_description: None,
        app_icon: None,
        app_icon_loading: false,
        app_icon_generation: 0,
    }
}

#[test]
fn replay_retains_only_the_latest_completed_audio_and_rejects_late_or_cancelled_results() {
    let mut runtime = cancel_runtime("done", "island");
    assert_eq!(runtime.replay_audio(7), Err("voice.error.noAudio".into()));
    let first = Bytes::from(audio::wav(&[0.1, -0.1], 16_000));
    assert!(runtime.finish_audio(7, Some(first.clone())));
    assert_eq!(runtime.replay_audio(7), Ok(first));
    runtime.generation = 8;
    let latest = Bytes::from(audio::wav(&[0.2, -0.2], 16_000));
    assert!(runtime.finish_audio(8, Some(latest.clone())));
    assert_eq!(runtime.replay_audio(7), Err("voice.error.noAudio".into()));
    assert!(!runtime.finish_audio(7, None));
    runtime.origin = "test".into();
    runtime.generation = 9;
    assert!(runtime.finish_audio(9, None));
    assert_eq!(runtime.replay_audio(8), Ok(latest.clone()));
    runtime.origin = "island".into();
    runtime.cancel(None);
    assert!(!runtime.finish_audio(runtime.generation, None));
    assert_eq!(runtime.replay_audio(8), Ok(latest));
    runtime.cancel = CancellationToken::new();
    assert!(runtime.finish_audio(runtime.generation, None));
    assert_eq!(runtime.replay_audio(8), Err("voice.error.noAudio".into()));
}

#[test]
fn replay_is_blocked_while_capturing_or_processing_but_available_after_recognition_errors() {
    let mut runtime = cancel_runtime("done", "island");
    let wav = Bytes::from(audio::wav(&[0.25; 16], 16_000));
    assert!(runtime.finish_audio(7, Some(wav.clone())));
    for phase in [
        "starting",
        "recording",
        "testing",
        "transcribing",
        "inserting",
    ] {
        runtime.phase = phase.into();
        assert_eq!(runtime.replay_audio(7), Err("voice.error.busy".into()));
    }
    runtime.cancel.cancel();
    for phase in ["idle", "done", "error"] {
        runtime.phase = phase.into();
        assert_eq!(runtime.replay_audio(7), Ok(wav.clone()));
    }
}

#[test]
fn channel_activity_stops_after_recognition_or_cancellation() {
    let mut runtime = cancel_runtime("recording", "shortcut");
    runtime.active_channel = Some(1);
    for phase in ["recording", "transcribing"] {
        runtime.phase = phase.into();
        assert_eq!(runtime.active_channel(), Some(1));
    }
    for phase in ["idle", "starting", "testing", "inserting", "done", "error"] {
        runtime.phase = phase.into();
        assert_eq!(runtime.active_channel(), None);
    }
    runtime.phase = "transcribing".into();
    runtime.cancel.cancel();
    assert_eq!(runtime.active_channel(), None);
}

#[test]
fn foreground_icon_loads_keep_the_old_icon_and_only_accept_the_latest_request() {
    let mut runtime = cancel_runtime("recording", "island");
    runtime.app_icon = Some("old".into());
    let first = runtime.begin_app_icon(true);
    assert!(runtime.app_icon_loading);
    assert_eq!(runtime.app_icon.as_deref(), Some("old"));
    let latest = runtime.begin_app_icon(true);
    assert!(!runtime.finish_app_icon(first, Some("stale".into())));
    assert!(runtime.app_icon_loading);
    assert_eq!(runtime.app_icon.as_deref(), Some("old"));
    assert!(runtime.finish_app_icon(latest, Some("latest".into())));
    assert_eq!(runtime.app_icon.as_deref(), Some("latest"));
    assert!(!runtime.app_icon_loading);
    assert_eq!(runtime.phase, "recording");
    assert_eq!(runtime.generation, 7);
    assert!(!runtime.cancel.is_cancelled());
}

#[test]
fn missing_foreground_or_disabling_invalidates_icons_and_failed_reads_settle() {
    let mut runtime = cancel_runtime("idle", "page");
    runtime.app_icon = Some("old".into());
    let pending = runtime.begin_app_icon(true);
    runtime.begin_app_icon(false);
    assert!(!runtime.app_icon_loading);
    assert!(runtime.app_icon.is_none());
    assert!(!runtime.finish_app_icon(pending, Some("late".into())));
    let next = runtime.begin_app_icon(true);
    assert!(runtime.finish_app_icon(next, None));
    assert!(!runtime.app_icon_loading);
    assert!(runtime.app_icon.is_none());
}

#[test]
fn escape_cancels_only_the_current_dictation_and_invalidates_pending_work() {
    for phase in ["starting", "recording", "transcribing", "inserting"] {
        for origin in ["shortcut", "island"] {
            let mut runtime = cancel_runtime(phase, origin);
            let pending = runtime.cancel.clone();
            assert!(runtime.cancel(Some(7)));
            assert!(pending.is_cancelled());
            assert_eq!(runtime.phase, "idle");
            assert_eq!(runtime.generation, 8);
            assert!(runtime.started.is_none());
            assert_eq!(runtime.level, 0.0);
        }
    }
    let mut next_recording = cancel_runtime("recording", "island");
    next_recording.generation = 8;
    assert!(!next_recording.cancel(Some(7)));
    assert!(!next_recording.cancel.is_cancelled());
    assert_eq!(next_recording.phase, "recording");
    for (phase, origin) in [
        ("idle", "island"),
        ("done", "shortcut"),
        ("error", "island"),
        ("starting", "test"),
        ("testing", "test"),
    ] {
        let mut runtime = cancel_runtime(phase, origin);
        assert!(!runtime.cancel(Some(7)));
        assert!(!runtime.cancel.is_cancelled());
        assert_eq!(runtime.generation, 7);
    }
    assert!(cancel_runtime("testing", "test").cancel(None));
}

#[test]
fn x11_icons_decode_argb_and_reject_truncated_or_overflowing_properties() {
    let icon = app_icon::x11_icon(&[1, 1, 0x80402010]).unwrap();
    assert_eq!(icon.get_pixel(0, 0).0, [0x40, 0x20, 0x10, 0x80]);
    assert!(app_icon::x11_icon(&[32, 32, 0xff000000]).is_none());
    assert!(app_icon::x11_icon(&[u32::MAX, 2]).is_none());
    assert!(app_icon::x11_icon(&[0, 0]).is_none());
    let mut sizes = vec![1, 1, 0xffff0000, 32, 32];
    sizes.extend(vec![0xff00ff00; 32 * 32]);
    let icon = app_icon::x11_icon(&sizes).unwrap();
    assert_eq!(icon.dimensions(), (32, 32));
    assert_eq!(icon.get_pixel(0, 0).0, [0, 255, 0, 255]);
    // Prefer shrinking an adequate source over enlarging a closer undersized one.
    sizes.extend([128, 128]);
    sizes.extend(vec![0xff0000ff; 128 * 128]);
    let icon = app_icon::x11_icon(&sizes).unwrap();
    assert_eq!(icon.dimensions(), (128, 128));
    assert_eq!(icon.get_pixel(0, 0).0, [0, 0, 255, 255]);
    sizes.extend([64, 64]);
    sizes.extend(vec![0xffff0000; 64 * 64]);
    assert_eq!(app_icon::x11_icon(&sizes).unwrap().dimensions(), (64, 64));
}

#[test]
#[ignore = "reads the current foreground desktop application's icon; no audio or network"]
fn live_foreground_application_icon_is_a_high_density_png() {
    use base64::Engine;
    let window = native::foreground().expect("select a foreground application");
    let icon = app_icon::read(window).expect("foreground application has no exposed icon");
    let png = base64::engine::general_purpose::STANDARD
        .decode(icon.strip_prefix("data:image/png;base64,").unwrap())
        .unwrap();
    let image = image::load_from_memory(&png).unwrap();
    assert!(image.width() > 0 && image.width() <= 64);
    assert!(image.height() > 0 && image.height() <= 64);
    assert!(image.to_rgba8().pixels().any(|pixel| pixel[3] > 0));
}

#[test]
#[ignore = "requires an explicitly selected physical microphone; measures levels only locally"]
fn live_microphone_level_callbacks() {
    let readings = Arc::new(Mutex::new((0_usize, 0.0_f32, false)));
    let observed = readings.clone();
    let recorder = audio::Recorder::new();
    recorder
        .start(
            std::env::var("ECHOBIRD_MIC_TEST_DEVICE").ok(),
            false,
            Arc::new(move |level| {
                let mut readings = observed.lock().unwrap();
                readings.0 += 1;
                readings.1 = readings.1.max(level);
                readings.2 |= level < 0.0;
            }),
        )
        .unwrap();
    std::thread::sleep(Duration::from_secs(2));
    assert_eq!(
        recorder.stop().err().as_deref(),
        Some("voice.error.noAudio")
    );
    let (callbacks, peak, failed) = *readings.lock().unwrap();
    eprintln!("Microphone level callbacks={callbacks}, peak={peak:.8}, failed={failed}");
    assert!(!failed);
    assert!(
        callbacks > 0,
        "The microphone did not deliver audio callbacks"
    );
}

#[test]
fn capture_gain_enhances_quiet_speech_without_boosting_silence_or_floor_noise() {
    for rate in [8_000, 16_000, 44_100, 48_000] {
        let mut gain = audio::InputGain::new(rate);
        for sample in [0.0, 0.0002, -0.0002]
            .into_iter()
            .cycle()
            .take(rate as usize)
        {
            assert_eq!(gain.process(sample), sample);
        }
        let peak = (0..rate)
            .map(|index| (index as f32 * 220.0 * std::f32::consts::TAU / rate as f32).sin() * 0.007)
            .map(|sample| gain.process(sample).abs())
            .fold(0.0_f32, f32::max);
        assert!(
            (0.25..0.5).contains(&peak),
            "enhanced peak={peak} at {rate}Hz"
        );
    }
}

#[test]
fn capture_gain_preserves_normal_levels_and_limits_sudden_loud_transients() {
    let mut normal = audio::InputGain::new(16_000);
    for sample in [0.6, -0.6, 1.0, -1.0].into_iter().cycle().take(16_000) {
        assert_eq!(normal.process(sample), sample);
    }
    let mut gain = audio::InputGain::new(16_000);
    for _ in 0..16_000 {
        let value = gain.process(0.003);
        assert!(value <= 0.003 * 64.001);
    }
    assert!(gain.process(0.003) > 0.15);
    for sample in [0.9_f32, -0.9, 1.0, -1.0, 0.6, -0.6] {
        let value = gain.process(sample);
        assert!(value.is_finite() && value.abs() <= 1.0);
        assert_eq!(value.signum(), sample.signum());
        if sample.abs() <= 0.95 {
            assert!(value.abs() <= 0.950_001);
        } else {
            assert_eq!(value, sample);
        }
    }
}

#[test]
fn capture_gain_is_continuous_across_callback_boundaries() {
    let samples: Vec<_> = (0..32_000)
        .map(|index| (index as f32 * 0.13).sin() * 0.007)
        .collect();
    let mut whole_gain = audio::InputGain::new(16_000);
    let whole: Vec<_> = samples
        .iter()
        .map(|&value| whole_gain.process(value))
        .collect();
    let mut chunk_gain = audio::InputGain::new(16_000);
    let chunked: Vec<_> = samples
        .chunks(137)
        .flat_map(|chunk| {
            chunk
                .iter()
                .map(|&value| chunk_gain.process(value))
                .collect::<Vec<_>>()
        })
        .collect();
    assert_eq!(whole, chunked);
    let wav = audio::wav(&whole, 16_000);
    let streamed: Vec<_> = (0..chunked.len())
        .step_by(1600)
        .flat_map(|start| audio::pcm(&chunked, 16_000, start, (start + 1600).min(chunked.len())))
        .collect();
    assert_eq!(&wav[44..], streamed);
}

#[test]
fn microphone_meter_makes_quiet_input_visible_and_keeps_silence_empty() {
    let start = Instant::now();
    let mut meter = audio::LevelMeter::new(start);
    assert_eq!(
        meter.update(0.0, start + Duration::from_millis(80)),
        Some(0.0)
    );
    let quiet = meter
        .update(0.001, start + Duration::from_millis(160))
        .unwrap();
    assert!(
        quiet >= 0.15,
        "Quiet speech should light several meter bars: {quiet}"
    );
    assert_eq!(
        meter.update(1.0, start + Duration::from_millis(240)),
        Some(1.0)
    );
}

#[test]
fn streamed_pcm_matches_complete_recording_across_non_aligned_sample_boundaries() {
    for rate in [8000, 16000, 44100, 48000] {
        let samples: Vec<_> = (0..rate / 3)
            .map(|index| (index as f32 * 0.17).sin() * 0.7)
            .collect();
        let complete = audio::wav(&samples, rate);
        let mut streamed = Vec::new();
        let mut sent = 0;
        for available in (1..samples.len()).step_by(137) {
            let count = (available - 1) as u64 * 16000 / u64::from(rate);
            streamed.extend(audio::pcm(
                &samples[..available],
                rate,
                sent,
                count as usize,
            ));
            sent = count as usize;
        }
        streamed.extend_from_slice(&complete[44 + sent * 2..]);
        assert_eq!(streamed, complete[44..], "sample rate {rate}");
    }
}

#[test]
fn microphone_meter_keeps_speech_between_ui_updates_and_clears_the_previous_window() {
    let start = Instant::now();
    let mut meter = audio::LevelMeter::new(start);
    assert_eq!(meter.update(0.1, start + Duration::from_millis(10)), None);
    assert_eq!(meter.update(0.0, start + Duration::from_millis(30)), None);
    assert!(
        meter
            .update(0.0, start + Duration::from_millis(80))
            .unwrap()
            > 0.5
    );
    assert_eq!(
        meter.update(0.0, start + Duration::from_millis(160)),
        Some(0.0)
    );
}

#[test]
fn invalid_sources_are_rejected_before_any_configuration_is_written() {
    let mut config = Config::default();
    config.channels = vec![
        config::Channel {
            kind: "workbuddy".into(),
            id: "saved-account".into()
        };
        2
    ];
    assert!(config.validate().is_err());
    config.channels = (0..4)
        .map(|index| config::Channel {
            kind: "model".into(),
            id: index.to_string(),
        })
        .collect();
    assert!(config.validate().is_err());
    config.channels.pop();
    assert!(config.validate().is_ok());
    config.channels[0].kind = "local".into();
    assert!(config.validate().is_err());
}

#[test]
fn missing_configuration_is_read_only_and_malformed_configuration_is_not_replaced() {
    let directory = std::env::temp_dir().join(format!("echobird-voice-{}", uuid::Uuid::new_v4()));
    let path = directory.join("voice.json");
    let saved = config::load(&path).unwrap();
    assert!(saved.config.channels.is_empty());
    assert!(!saved.config.shortcut_enabled);
    assert!(!saved.config.hotkey_enabled);
    assert!(!directory.exists());
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(&path, b"broken-config").unwrap();
    assert!(config::load(&path).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), b"broken-config");
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn keyboard_input_is_opt_in_and_independent_of_mouse_input() {
    let mut config: Config = serde_json::from_value(serde_json::json!({
        "shortcutEnabled": true,
        "shortcut": { "code": "AltRight", "modifiers": [] }
    }))
    .unwrap();
    assert!(config.shortcut_enabled);
    assert!(!config.hotkey_enabled);
    assert!(!config.hotkey_active());
    config.hotkey_enabled = true;
    assert!(config.hotkey_active());
    let restored: Config = serde_json::from_value(serde_json::to_value(&config).unwrap()).unwrap();
    assert!(restored.hotkey_active());
    config.hotkey_enabled = false;
    assert!(!config.hotkey_active());
    assert!(config.shortcut_enabled);
    config.hotkey_enabled = true;
    config.shortcut_enabled = false;
    assert!(!config.hotkey_active());
}

#[test]
fn legacy_settings_and_statistics_round_trip_without_retired_options_or_sensitive_data() {
    let directory = std::env::temp_dir().join(format!("echobird-voice-{}", uuid::Uuid::new_v4()));
    let path = directory.join("voice.json");
    let mut saved = Saved::default();
    saved.config.channels.push(config::Channel {
        kind: "model".into(),
        id: "model-reference".into(),
    });
    saved.config.island_theme = "light".into();
    saved.days.insert("2026-10-23".into(), 42);
    config::save(&path, &saved).unwrap();
    let mut legacy = serde_json::to_value(&saved).unwrap();
    legacy.as_object_mut().unwrap().remove("usage");
    legacy["config"]
        .as_object_mut()
        .unwrap()
        .remove("islandPalette");
    legacy["config"]["islandStyle"] = serde_json::json!("wave");
    legacy["config"]["shortcutEnabled"] = serde_json::json!(true);
    legacy["config"]["islandAlwaysVisible"] = serde_json::json!(false);
    legacy["config"]["islandDraggable"] = serde_json::json!(false);
    legacy["config"]["islandColor"] = serde_json::json!("green");
    legacy["config"]["showDuration"] = serde_json::json!(false);
    legacy["config"]["inputMode"] = serde_json::json!("toggle");
    legacy["config"]["optimizationMode"] = serde_json::json!("custom");
    legacy["config"]["optimizationRules"] = serde_json::json!("retired-user-instructions");
    legacy["config"]
        .as_object_mut()
        .unwrap()
        .remove("translationLanguage");
    legacy["config"]["skipSeconds"] = serde_json::json!(60);
    let legacy = serde_json::to_vec(&legacy).unwrap();
    std::fs::write(&path, &legacy).unwrap();
    let mut restored = config::load(&path).unwrap();
    assert_eq!(restored.config.channels, saved.config.channels);
    assert_eq!(restored.config.island_theme, "light");
    assert_eq!(restored.config.island_palette, "white");
    assert_eq!(restored.config.selection_model, None);
    assert_eq!(restored.config.island_scale, 100);
    assert!(island_visible(&restored.config));
    assert_eq!(restored.days, saved.days);
    assert_eq!(restored.usage.duration_ms, 0);
    assert_eq!(restored.usage.characters, 0);
    assert_eq!(std::fs::read(&path).unwrap(), legacy);
    restored.config.shortcut_enabled = false;
    config::save(&path, &restored).unwrap();
    let updated = config::load(&path).unwrap();
    assert!(!island_visible(&updated.config));
    assert_eq!(updated.config.channels, saved.config.channels);
    assert_eq!(updated.days, saved.days);
    let json = std::fs::read_to_string(&path).unwrap();
    for field in [
        "islandColor",
        "islandStyle",
        "showDuration",
        "inputMode",
        "islandAlwaysVisible",
        "islandDraggable",
        "optimizationRules",
        "optimizationMode",
        "translationLanguage",
        "optimizationEnabled",
        "skipSeconds",
        "apiKey",
        "accessToken",
        "audio",
        "transcript",
    ] {
        assert!(!json.contains(field));
    }
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn voice_palettes_round_trip_and_legacy_styles_default_to_plain_bars() {
    for style in ["compact", "wave", "caption"] {
        let config: config::Config = serde_json::from_value(serde_json::json!({
            "islandStyle": style,
            "islandTheme": "light",
            "microphone": "saved-microphone"
        }))
        .unwrap();
        assert_eq!(config.island_palette, "white");
        assert_eq!(config.island_theme, "light");
        assert_eq!(config.microphone.as_deref(), Some("saved-microphone"));
        config.validate().unwrap();
        assert!(serde_json::to_value(config)
            .unwrap()
            .get("islandStyle")
            .is_none());
    }
    for palette in ["white", "aurora", "ocean", "sunset", "iris"] {
        let config = config::Config {
            island_palette: palette.into(),
            ..Default::default()
        };
        config.validate().unwrap();
        let restored: config::Config =
            serde_json::from_slice(&serde_json::to_vec(&config).unwrap()).unwrap();
        assert_eq!(restored.island_palette, palette);
    }
    for palette in ["", "wave", "rainbow"] {
        let config = config::Config {
            island_palette: palette.into(),
            ..Default::default()
        };
        assert!(config.validate().is_err());
    }
}

#[test]
fn voice_theme_modes_preserve_app_following_and_round_trip_system_following() {
    assert_eq!(config::Config::default().island_theme, "auto");
    for theme in ["auto", "dark", "light", "system", "iridescent"] {
        let saved = Saved {
            config: config::Config {
                island_theme: theme.into(),
                ..Default::default()
            },
            ..Default::default()
        };
        saved.config.validate().unwrap();
        let restored: Saved = serde_json::from_slice(&serde_json::to_vec(&saved).unwrap()).unwrap();
        assert_eq!(restored.config.island_theme, theme);
        assert_eq!(restored.config.island_palette, "white");
    }
    let legacy: config::Config =
        serde_json::from_value(serde_json::json!({"islandTheme":"auto"})).unwrap();
    assert_eq!(legacy.island_theme, "auto");
    let invalid = config::Config {
        island_theme: "theme".into(),
        ..Default::default()
    };
    assert!(invalid.validate().is_err());
}

#[test]
fn empty_channels_disable_input_without_rewriting_the_loaded_file_or_enabling_new_sources() {
    let directory = std::env::temp_dir().join(format!("echobird-voice-{}", uuid::Uuid::new_v4()));
    let path = directory.join("voice.json");
    let mut saved = Saved::default();
    saved.config.shortcut_enabled = true;
    config::save(&path, &saved).unwrap();
    let original = std::fs::read(&path).unwrap();
    assert!(!island_visible(&saved.config));
    let mut loaded = config::load(&path).unwrap();
    assert!(!loaded.config.shortcut_enabled);
    assert!(!island_visible(&loaded.config));
    assert_eq!(std::fs::read(&path).unwrap(), original);
    loaded.config.channels.push(config::Channel {
        kind: "model".into(),
        id: "speech-model".into(),
    });
    loaded.config.disable_without_channels();
    assert!(!loaded.config.shortcut_enabled);
    loaded.config.shortcut_enabled = true;
    loaded.config.disable_without_channels();
    assert!(island_visible(&loaded.config));
    config::save(&path, &loaded).unwrap();
    assert!(config::load(&path).unwrap().config.shortcut_enabled);
    loaded.config.channels.clear();
    loaded.config.disable_without_channels();
    config::save(&path, &loaded).unwrap();
    let updated: Saved = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert!(!updated.config.shortcut_enabled);
    assert!(!island_visible(&updated.config));
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn usage_tracks_only_nonempty_completed_dictations_with_known_duration() {
    let mut usage = config::Usage::default();
    usage.record(0, 1000);
    usage.record(20, 0);
    assert_eq!((usage.characters, usage.duration_ms), (0, 0));
    usage.record(80, 30000);
    usage.record(20, 10000);
    assert_eq!((usage.characters, usage.duration_ms), (100, 40000));
    let saved = Saved {
        usage,
        ..Saved::default()
    };
    let json = serde_json::to_vec(&saved).unwrap();
    let restored: Saved = serde_json::from_slice(&json).unwrap();
    assert_eq!(
        (restored.usage.characters, restored.usage.duration_ms),
        (100, 40000)
    );
}

#[test]
fn keyboard_hold_and_release_only_control_keyboard_recording() {
    assert_eq!(
        shortcut_action("hold", "idle", "page", true),
        Some(ShortcutAction::Start)
    );
    assert_eq!(
        shortcut_action("hold", "done", "island", true),
        Some(ShortcutAction::Start)
    );
    assert_eq!(
        shortcut_action("hold", "recording", "shortcut", false),
        Some(ShortcutAction::Stop)
    );
    for origin in ["island", "test", "page"] {
        assert_eq!(shortcut_action("hold", "recording", origin, false), None);
    }
    for phase in [
        "starting",
        "recording",
        "testing",
        "transcribing",
        "inserting",
    ] {
        assert_eq!(shortcut_action("hold", phase, "island", true), None);
        assert_eq!(shortcut_action("hold", phase, "shortcut", true), None);
    }
    assert_eq!(shortcut_action("hold", "idle", "shortcut", false), None);
}

#[test]
fn keyboard_toggle_starts_and_finishes_on_separate_releases_without_stopping_mouse_input() {
    for phase in ["idle", "done", "error"] {
        assert_eq!(shortcut_action("toggle", phase, "page", true), None);
        assert_eq!(
            shortcut_action("toggle", phase, "page", false),
            Some(ShortcutAction::Start)
        );
    }
    for phase in ["starting", "recording"] {
        assert_eq!(shortcut_action("toggle", phase, "shortcut", true), None);
        assert_eq!(
            shortcut_action("toggle", phase, "shortcut", false),
            Some(ShortcutAction::Stop)
        );
        for origin in ["island", "page", "test"] {
            assert_eq!(shortcut_action("toggle", phase, origin, false), None);
        }
    }
    for phase in ["testing", "transcribing", "inserting"] {
        for down in [false, true] {
            assert_eq!(shortcut_action("toggle", phase, "shortcut", down), None);
        }
    }
}

#[test]
fn hotkey_mode_defaults_to_hold_and_toggle_survives_save_without_enabling_the_hotkey() {
    let legacy: Config = serde_json::from_value(serde_json::json!({
        "shortcut": { "code": "AltRight", "modifiers": [] }
    }))
    .unwrap();
    assert_eq!(legacy.hotkey_mode, "hold");
    let directory = std::env::temp_dir().join(format!("echobird-hotkey-{}", uuid::Uuid::new_v4()));
    let path = directory.join("voice.json");
    let mut saved = Saved::default();
    saved.config.hotkey_mode = "toggle".into();
    config::save(&path, &saved).unwrap();
    let restored = config::load(&path).unwrap().config;
    assert_eq!(restored.hotkey_mode, "toggle");
    assert!(!restored.hotkey_enabled);
    assert!(!restored.shortcut_enabled);
    saved.config.hotkey_mode = "invalid".into();
    assert_eq!(saved.config.validate().unwrap_err(), "voice.error.config");
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn legacy_island_settings_do_not_override_the_master_switch() {
    for enabled in [true, false] {
        for resident in [true, false] {
            for draggable in [true, false] {
                let config: Config = serde_json::from_value(serde_json::json!({
                    "channels": [{ "kind": "model", "id": "speech-model" }],
                    "shortcutEnabled": enabled,
                    "islandAlwaysVisible": resident,
                    "islandDraggable": draggable
                }))
                .unwrap();
                assert_eq!(island_visible(&config), enabled);
            }
        }
    }
}

#[test]
fn recorded_audio_is_mono_pcm16_wav_resampled_to_16khz() {
    let samples = vec![0.5; 48000];
    let wav = audio::wav(&samples, 48000);
    assert_eq!(wav.len(), 32044);
    assert_eq!(&wav[..4], b"RIFF");
    assert_eq!(&wav[8..12], b"WAVE");
    assert_eq!(u32::from_le_bytes(wav[24..28].try_into().unwrap()), 16000);
    assert_eq!(u16::from_le_bytes(wav[22..24].try_into().unwrap()), 1);
    assert_eq!(u16::from_le_bytes(wav[34..36].try_into().unwrap()), 16);
    assert_eq!(i16::from_le_bytes(wav[44..46].try_into().unwrap()), 16383);
}

#[test]
fn transcription_urls_keep_proxy_paths_and_do_not_repeat_the_endpoint() {
    for (base, expected) in [
        (
            "https://example.test/v1",
            "https://example.test/v1/audio/transcriptions",
        ),
        (
            "https://example.test/proxy/v1/",
            "https://example.test/proxy/v1/audio/transcriptions",
        ),
        (
            "https://example.test/v1/chat/completions",
            "https://example.test/v1/audio/transcriptions",
        ),
        (
            "https://example.test/v1/audio/transcriptions",
            "https://example.test/v1/audio/transcriptions",
        ),
    ] {
        assert_eq!(pipeline::transcription_url(base).unwrap(), expected);
    }
    for invalid in [
        "file:///tmp/audio",
        "https://secret@example.test/v1",
        "not-a-url",
    ] {
        assert!(pipeline::transcription_url(invalid).is_err());
    }
}

#[test]
fn physical_shortcuts_keep_platform_modifiers_and_reject_unknown_codes() {
    let binding = config::Binding {
        code: "Space".into(),
        modifiers: vec!["Control".into(), "Alt".into()],
    };
    assert!(shortcut::parse(&binding).is_ok());
    assert!(!native::modifier(&binding));
    assert!(native::modifier(&Config::default().shortcut));
    assert!(shortcut::parse(&config::Binding {
        code: "UnknownKey".into(),
        modifiers: vec![]
    })
    .is_err());
}

#[tokio::test]
async fn channel_failover_preserves_order_and_never_repeats_a_successful_result() {
    let channels: Vec<_> = (0..3)
        .map(|index| config::Channel {
            kind: "model".into(),
            id: index.to_string(),
        })
        .collect();
    for successful in ["recognized", " recognized "] {
        let mut calls = Vec::new();
        let result = pipeline::first_transcript(&channels, None, |channel| {
            calls.push(channel.id.clone());
            std::future::ready(if channel.id == "0" {
                Err("failed".into())
            } else {
                Ok(successful.into())
            })
        })
        .await
        .unwrap();
        assert_eq!(result, successful);
        assert_eq!(calls, ["0", "1"]);
    }
    let mut calls = Vec::new();
    let error = pipeline::first_transcript(&channels, None, |channel| {
        calls.push(channel.id);
        std::future::ready(Err("failed".into()))
    })
    .await
    .unwrap_err();
    assert_eq!(calls, ["0", "1", "2"]);
    assert_eq!(error, "voice.error.allChannels");
}

#[tokio::test]
async fn empty_transcripts_try_backups_and_preserve_the_no_transcript_error_if_none_succeed() {
    let channels: Vec<_> = (0..3)
        .map(|index| config::Channel {
            kind: "model".into(),
            id: index.to_string(),
        })
        .collect();
    for text in ["", " \n\t", "\u{3000}"] {
        for successful in [false, true] {
            let mut calls = Vec::new();
            let result = pipeline::first_transcript(&channels, None, |channel| {
                calls.push(channel.id.clone());
                std::future::ready(if channel.id == "0" {
                    Err("failed".into())
                } else if channel.id == "2" && successful {
                    Ok("recognized by third source".into())
                } else {
                    Ok(text.into())
                })
            })
            .await;
            assert_eq!(
                result,
                if successful {
                    Ok("recognized by third source".into())
                } else {
                    Err("voice.error.noTranscript".into())
                }
            );
            assert_eq!(calls, ["0", "1", "2"]);
        }
    }
}

#[tokio::test]
async fn realtime_results_use_backups_only_when_needed_and_keep_wav_recovery_for_network_errors() {
    let channels: Vec<_> = (0..3)
        .map(|index| config::Channel {
            kind: "model".into(),
            id: index.to_string(),
        })
        .collect();
    for (streamed, expected_calls, expected_text) in [
        (Ok("live transcript".to_string()), vec![], "live transcript"),
        (Ok(" \n\t".into()), vec!["1", "2"], "backup transcript"),
        (
            Err("voice.error.noTranscript".into()),
            vec!["1", "2"],
            "backup transcript",
        ),
        (
            Err("voice.error.transcribe".into()),
            vec!["0", "1", "2"],
            "backup transcript",
        ),
    ] {
        let mut calls = Vec::new();
        let result = pipeline::first_transcript(&channels, Some(streamed), |channel| {
            calls.push(channel.id.clone());
            std::future::ready(if channel.id == "2" {
                Ok("backup transcript".into())
            } else {
                Err("voice.error.transcribe".into())
            })
        })
        .await;
        assert_eq!(result, Ok(expected_text.into()));
        assert_eq!(calls, expected_calls);
    }
    let result = pipeline::first_transcript(
        &channels[..1],
        Some(Err("voice.error.noTranscript".into())),
        |_| -> std::future::Ready<Result<String, String>> {
            panic!("an empty realtime result must not reupload to the same source")
        },
    )
    .await;
    assert_eq!(result, Err("voice.error.noTranscript".into()));
}

#[tokio::test]
async fn cancellation_drops_the_pending_channel_without_requesting_backups() {
    let channels: Vec<_> = (0..3)
        .map(|index| config::Channel {
            kind: "model".into(),
            id: index.to_string(),
        })
        .collect();
    let cancelled = CancellationToken::new();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let pending = pipeline::first_transcript(&channels, None, |channel| {
        calls.lock().unwrap().push(channel.id);
        std::future::pending::<Result<String, String>>()
    });
    tokio::pin!(pending);
    tokio::select! { _ = &mut pending => panic!("unexpected completion"), _ = tokio::time::sleep(Duration::from_millis(1)) => {} }
    cancelled.cancel();
    tokio::select! { biased; _ = cancelled.cancelled() => {}, _ = &mut pending => panic!("cancelled work completed") }
    assert_eq!(*calls.lock().unwrap(), ["0"]);
}

fn model(base: &str) -> crate::models::model::ModelConfig {
    serde_json::from_value(serde_json::json!({"internalId":"fixture-model","name":"Fixture","modelId":"speech-model","baseUrl":base,"apiKey":"fixture-key"})).unwrap()
}

#[tokio::test]
async fn speech_api_upload_preserves_wav_model_and_authentication_without_exposing_upstream_errors()
{
    use axum::{
        extract::Multipart,
        http::{HeaderMap, StatusCode},
        routing::post,
        Json, Router,
    };
    let requests = Arc::new(Mutex::new(0));
    let count = requests.clone();
    let audio = audio::wav(&vec![0.25; 1600], 16000);
    let expected = audio.clone();
    let app = Router::new()
        .route(
            "/custom/v1/audio/transcriptions",
            post(move |headers: HeaderMap, mut multipart: Multipart| {
                let expected = expected.clone();
                let count = count.clone();
                async move {
                    *count.lock().unwrap() += 1;
                    assert_eq!(headers["authorization"], "Bearer fixture-key");
                    let mut fields = std::collections::HashMap::new();
                    while let Some(field) = multipart.next_field().await.unwrap() {
                        fields.insert(
                            field.name().unwrap().to_owned(),
                            field.bytes().await.unwrap(),
                        );
                    }
                    assert_eq!(fields["model"], "speech-model");
                    assert_eq!(fields["file"], expected);
                    Json(serde_json::json!({"text":"实际识别结果"}))
                }
            }),
        )
        .route(
            "/error/audio/transcriptions",
            post(|| async { (StatusCode::BAD_GATEWAY, "private-token=not-for-the-user") }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let client = reqwest::Client::new();
    assert_eq!(
        pipeline::recognize_model(
            &model(&format!("http://{address}/custom/v1")),
            &audio.clone().into(),
            &client
        )
        .await
        .unwrap(),
        "实际识别结果"
    );
    assert_eq!(*requests.lock().unwrap(), 1);
    let error = pipeline::recognize_model(
        &model(&format!("http://{address}/error")),
        &audio.into(),
        &client,
    )
    .await
    .unwrap_err();
    assert_eq!(error, "voice.error.transcribe");
    task.abort();
}

#[test]
fn input_target_checks_focus_within_the_original_window() {
    let target = native::InputTarget {
        window: 1,
        focus: Some(2),
    };
    assert!(target.matches(Some(target)));
    assert!(!target.matches(Some(native::InputTarget {
        window: 1,
        focus: Some(3),
    })));
    assert!(!target.matches(Some(native::InputTarget {
        window: 1,
        focus: None,
    })));
    assert!(!target.matches(Some(native::InputTarget {
        window: 4,
        focus: Some(2),
    })));
    assert!(!target.matches(None));

    let window_only = native::InputTarget {
        window: 1,
        focus: None,
    };
    assert!(window_only.matches(Some(target)));
    assert!(window_only.matches(Some(window_only)));
    assert!(!window_only.matches(None));
}

#[test]
fn menu_mask_only_applies_to_bare_alt_shortcuts() {
    for (code, key) in [("AltLeft", 0xA4), ("AltRight", 0xA5)] {
        let mut binding = config::Binding {
            code: code.into(),
            modifiers: Vec::new(),
        };
        assert_eq!(native::alt_menu_key(&binding), Some(key));
        binding.modifiers.push("Control".into());
        assert_eq!(native::alt_menu_key(&binding), None);
    }
    for code in ["ControlRight", "ShiftLeft", "MetaLeft", "KeyV"] {
        assert_eq!(
            native::alt_menu_key(&config::Binding {
                code: code.into(),
                modifiers: Vec::new(),
            }),
            None
        );
    }
}

#[tokio::test]
async fn translation_rejects_failed_or_empty_model_responses() {
    use axum::{http::StatusCode, routing::post, Json, Router};
    let app = Router::new().route(
        "/v1/chat/completions",
        post(|Json(body): Json<serde_json::Value>| async move {
            let (status, response) = if body["messages"][1]["content"] == "failed request" {
                (StatusCode::BAD_GATEWAY, "")
            } else {
                (StatusCode::OK, "data: [DONE]\n\n")
            };
            (status, [("content-type", "text/event-stream")], response)
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    for text in ["failed request", "empty response"] {
        assert!(
            pipeline::text_model(model(&format!("http://{address}/v1")), text, "Translate.")
                .await
                .is_err()
        );
    }
    task.abort();
}

#[test]
fn retired_optimization_settings_keep_the_selection_model_and_island_scale_is_bounded() {
    let legacy: Config = serde_json::from_value(serde_json::json!({
        "optimizationEnabled": true,
        "optimizationModel": "text-model",
        "optimizationMode": "semantic",
        "translationLanguage": "fr",
        "selectionTranslationEnabled": true
    }))
    .unwrap();
    legacy.validate().unwrap();
    assert_eq!(legacy.selection_model.as_deref(), Some("text-model"));
    assert!(legacy.selection_active());
    let json = serde_json::to_string(&legacy).unwrap();
    assert!(json.contains("\"selectionModel\":\"text-model\""));
    for field in [
        "optimizationModel",
        "optimizationEnabled",
        "translationLanguage",
    ] {
        assert!(!json.contains(field));
    }
    for scale in [70, 100, 150] {
        let config = Config {
            island_scale: scale,
            ..Config::default()
        };
        config.validate().unwrap();
    }
    for scale in [0, 60, 105, 160] {
        let config = Config {
            island_scale: scale,
            ..Config::default()
        };
        assert!(config.validate().is_err());
    }
    for zoom in [0.7, 1.5] {
        let area = tauri::PhysicalRect {
            position: tauri::PhysicalPosition::new(0, 0),
            size: tauri::PhysicalSize::new(1920, 1040),
        };
        let position = island_position(&area, 1.0, zoom);
        let center = f64::from(position.x)
            + (island_input::BODY_LEFT + island_input::BODY_WIDTH / 2.0) * zoom;
        assert!((center - 960.0).abs() <= 0.5);
        let bottom =
            f64::from(position.y) + (island_input::BODY_TOP + island_input::BODY_HEIGHT) * zoom;
        assert!((1040.0 - bottom - 56.0 * zoom).abs() <= 0.5);
    }
}

#[test]
fn interim_transcripts_only_update_the_current_live_recording() {
    let mut runtime = cancel_runtime("recording", "island");
    let generation = runtime.generation;
    assert!(runtime.accept_partial(generation, "早期"));
    assert!(!runtime.accept_partial(generation, "早期"));
    assert!(runtime.accept_partial(generation, "早期结果"));
    assert_eq!(runtime.text, "早期结果");
    assert!(!runtime.accept_partial(generation + 1, "旧的一次"));
    runtime.phase = "transcribing".into();
    assert!(runtime.accept_partial(generation, "修订结果"));
    for phase in ["inserting", "done", "error", "idle"] {
        runtime.phase = phase.into();
        assert!(!runtime.accept_partial(generation, phase));
    }
    runtime.phase = "recording".into();
    assert!(runtime.cancel(None));
    assert!(runtime.text.is_empty());
    assert!(!runtime.accept_partial(generation, "取消后迟到"));
}
