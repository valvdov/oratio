use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter, Manager};

use oratio_core::audio::Recorder;
use oratio_core::polish::{
    openai_compat::OpenAiCompat, plausible_output, regex_clean, PolishProvider, PolishRequest,
};
use oratio_core::settings::Settings;
use oratio_core::stt::streaming::SegmentAssembler;
use oratio_core::stt::{whisper::WhisperEngine, SttEngine, TranscribeOptions};
use oratio_core::{dictionary, models};

use crate::{inject, tray};

/// Commands understood by the dedicated audio thread. The cpal stream is not
/// Send, so the recorder must live and die on that one thread.
enum AudioCmd {
    /// Begin a dictation; raw sample chunks flow into the given sender.
    Start(Sender<StreamMsg>),
    /// End the dictation; the ack fires after the final chunk was forwarded.
    Stop(Sender<()>),
    Cancel,
}

/// Audio stream messages from the audio thread to the per-session worker.
enum StreamMsg {
    Meta { rate: u32, channels: u16 },
    Raw(Vec<f32>),
    End,
}

/// Result of a streaming transcription session.
struct SessionResult {
    raw_text: String,
    audio16k: Vec<f32>,
    speech_ms: u32,
    stt_secs: f32,
    segments: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Idle,
    Recording,
    Processing,
}

struct Session {
    phase: Phase,
    pressed_at: Option<Instant>,
    /// True when a short press latched the recording (toggle mode).
    latched: bool,
    /// True when the raw-mode hotkey started this session (skip LLM polish).
    raw: bool,
    /// PID and bundle id of the app that was frontmost when dictation started.
    target: Option<(i32, Option<String>)>,
    /// Receives the transcription result of the in-flight session.
    result_rx: Option<Receiver<SessionResult>>,
}

pub struct AppState {
    pub settings: Mutex<Settings>,
    pub engine: Mutex<Option<WhisperEngine>>,
    pub history: Mutex<Option<oratio_core::history::History>>,
    /// The currently registered raw-mode (Shift) shortcut, for handler dispatch.
    pub raw_shortcut: Mutex<Option<tauri_plugin_global_shortcut::Shortcut>>,
    session: Mutex<Session>,
    audio_tx: Sender<AudioCmd>,
}

impl AppState {
    pub fn new(settings: Settings, app: AppHandle) -> Self {
        let (tx, rx) = mpsc::channel();
        std::thread::Builder::new()
            .name("oratio-audio".into())
            .spawn(move || audio_thread(rx, app))
            .expect("spawn audio thread");
        let history_path = oratio_core::paths::data_dir().join("history.db");
        let history = match oratio_core::history::History::open(&history_path) {
            Ok(h) => Some(h),
            Err(e) => {
                tracing::warn!("history disabled: {e}");
                None
            }
        };
        Self {
            settings: Mutex::new(settings),
            engine: Mutex::new(None),
            history: Mutex::new(history),
            raw_shortcut: Mutex::new(None),
            session: Mutex::new(Session {
                phase: Phase::Idle,
                pressed_at: None,
                latched: false,
                raw: false,
                target: None,
                result_rx: None,
            }),
            audio_tx: tx,
        }
    }
}

fn audio_thread(rx: Receiver<AudioCmd>, app: AppHandle) {
    // The stream is opened per dictation and closed right after: a permanently
    // open mic keeps the orange indicator on and forces Bluetooth headsets
    // (AirPods) into the low-quality HFP profile even when idle.
    let mut recorder: Option<Recorder> = None;
    let mut session_tx: Option<Sender<StreamMsg>> = None;
    loop {
        match rx.recv_timeout(Duration::from_millis(80)) {
            Ok(AudioCmd::Start(tx)) => {
                match Recorder::open() {
                    Ok(r) => recorder = Some(r),
                    Err(e) => {
                        tracing::error!("failed to open microphone: {e}");
                        set_phase(&app, Phase::Idle);
                        continue;
                    }
                }
                let r = recorder.as_ref().unwrap();
                r.begin();
                let _ = tx.send(StreamMsg::Meta {
                    rate: r.device_rate(),
                    channels: r.channels(),
                });
                session_tx = Some(tx);
            }
            Ok(AudioCmd::Stop(ack)) => {
                // `take()` drops the recorder = closes the stream, releasing the mic.
                if let (Some(r), Some(tx)) = (recorder.take(), session_tx.take()) {
                    let rest = r.end_raw();
                    if !rest.is_empty() {
                        let _ = tx.send(StreamMsg::Raw(rest));
                    }
                    let _ = tx.send(StreamMsg::End);
                }
                let _ = ack.send(());
            }
            Ok(AudioCmd::Cancel) => {
                recorder.take();
                // Dropping the sender terminates the session worker.
                session_tx = None;
            }
            Err(RecvTimeoutError::Timeout) => {
                if let Some(r) = &recorder {
                    if r.is_recording() {
                        let _ = app.emit("pill://level", r.level());
                        if let Some(tx) = &session_tx {
                            let chunk = r.drain_recorded();
                            if !chunk.is_empty() {
                                let _ = tx.send(StreamMsg::Raw(chunk));
                            }
                        }
                    }
                }
            }
            Err(RecvTimeoutError::Disconnected) => return,
        }
    }
}

/// Per-session worker: assembles audio into VAD segments and transcribes them
/// while the user is still speaking, so stopping only costs the tail segment.
fn streaming_session(app: AppHandle, rx: Receiver<StreamMsg>, result_tx: Sender<SessionResult>) {
    let mut assembler: Option<SegmentAssembler> = None;
    let mut texts: Vec<String> = Vec::new();
    let mut stt_secs = 0.0f32;
    let mut segments = 0usize;

    loop {
        match rx.recv() {
            Ok(StreamMsg::Meta { rate, channels }) => {
                match SegmentAssembler::new(rate, channels) {
                    Ok(a) => assembler = Some(a),
                    Err(e) => {
                        tracing::error!("assembler init failed: {e}");
                        return;
                    }
                }
            }
            Ok(StreamMsg::Raw(chunk)) => {
                let Some(asm) = assembler.as_mut() else { continue };
                match asm.feed(&chunk) {
                    Ok(completed) => {
                        for segment in completed {
                            if segment.speech_ms < 250 {
                                continue;
                            }
                            segments += 1;
                            if let Some((text, secs)) =
                                transcribe_segment(&app, &segment.samples, last_tail(&texts))
                            {
                                stt_secs += secs;
                                texts.push(text);
                            }
                        }
                    }
                    Err(e) => tracing::warn!("audio feed failed: {e}"),
                }
            }
            Ok(StreamMsg::End) => {
                let Some(asm) = assembler.take() else { return };
                match asm.finish() {
                    Ok((tail, audio16k, speech_ms)) => {
                        if let Some(segment) = tail {
                            if segment.speech_ms >= 250 {
                                segments += 1;
                                if let Some((text, secs)) =
                                    transcribe_segment(&app, &segment.samples, last_tail(&texts))
                                {
                                    stt_secs += secs;
                                    texts.push(text);
                                }
                            }
                        }
                        let _ = result_tx.send(SessionResult {
                            raw_text: texts.join(" "),
                            audio16k,
                            speech_ms,
                            stt_secs,
                            segments,
                        });
                    }
                    Err(e) => tracing::error!("assembler finish failed: {e}"),
                }
                return;
            }
            // Sender dropped: session was cancelled.
            Err(_) => return,
        }
    }
}

fn last_tail(texts: &[String]) -> Option<String> {
    texts.last().map(|t| {
        let chars: Vec<char> = t.chars().collect();
        let start = chars.len().saturating_sub(150);
        chars[start..].iter().collect()
    })
}

/// Transcribe one segment. Returns None when the segment is empty/hallucinated.
fn transcribe_segment(
    app: &AppHandle,
    segment: &[f32],
    prev_tail: Option<String>,
) -> Option<(String, f32)> {
    let state = app.state::<AppState>();
    let (language, dict) = {
        let s = state.settings.lock().unwrap();
        (s.stt.language.clone(), s.dictionary.clone())
    };
    let mut prompt = dictionary::build_initial_prompt(&dict);
    if let Some(tail) = prev_tail {
        prompt.push(' ');
        prompt.push_str(&tail);
    }
    // whisper needs at least ~1s of audio; pad short tails with silence.
    let min_len = (oratio_core::SAMPLE_RATE as usize * 11) / 10;
    let padded;
    let samples = if segment.len() < min_len {
        padded = {
            let mut v = segment.to_vec();
            v.resize(min_len, 0.0);
            v
        };
        &padded[..]
    } else {
        segment
    };

    let mut engine_guard = state.engine.lock().unwrap();
    if engine_guard.is_none() {
        match load_engine(app) {
            Ok(engine) => *engine_guard = Some(engine),
            Err(e) => {
                tracing::error!("engine load failed: {e}");
                return None;
            }
        }
    }
    let opts = TranscribeOptions {
        language: if language == "auto" { None } else { Some(language) },
        initial_prompt: Some(prompt),
    };
    match engine_guard.as_mut().unwrap().transcribe(samples, &opts) {
        Ok(t) => {
            tracing::debug!(
                "segment transcribed in {:.2}s: {} chars",
                t.stt_time.as_secs_f32(),
                t.text.len()
            );
            Some((t.text, t.stt_time.as_secs_f32()))
        }
        Err(oratio_core::Error::NoSpeech) => None,
        Err(e) => {
            tracing::warn!("segment transcription failed: {e}");
            None
        }
    }
}

pub fn on_hotkey_press(app: &AppHandle, raw: bool) {
    let state = app.state::<AppState>();
    let mut session = state.session.lock().unwrap();
    match session.phase {
        Phase::Idle => {
            session.phase = Phase::Recording;
            session.pressed_at = Some(Instant::now());
            session.latched = false;
            session.raw = raw;
            session.target = crate::frontmost::frontmost_app();
            if let Some((pid, bundle)) = &session.target {
                tracing::debug!("dictation target: pid {pid}, {bundle:?}");
            }
            let (stream_tx, stream_rx) = mpsc::channel();
            let (result_tx, result_rx) = mpsc::channel();
            session.result_rx = Some(result_rx);
            let worker_app = app.clone();
            std::thread::Builder::new()
                .name("oratio-session".into())
                .spawn(move || streaming_session(worker_app, stream_rx, result_tx))
                .expect("spawn session thread");
            let _ = state.audio_tx.send(AudioCmd::Start(stream_tx));
            drop(session);
            play_sound(app, SoundCue::Start);
            show_pill(app, "recording");
            tray::set_recording(app, true);
        }
        Phase::Recording if session.latched => {
            drop(session);
            finish(app);
        }
        _ => {}
    }
}

pub fn on_hotkey_release(app: &AppHandle) {
    let state = app.state::<AppState>();
    let mut session = state.session.lock().unwrap();
    if session.phase != Phase::Recording || session.latched {
        return;
    }
    let threshold = state.settings.lock().unwrap().hotkeys.toggle_threshold_ms;
    let held = session
        .pressed_at
        .map(|t| t.elapsed().as_millis() as u64)
        .unwrap_or(u64::MAX);
    if held < threshold {
        session.latched = true;
    } else {
        drop(session);
        finish(app);
    }
}

fn finish(app: &AppHandle) {
    let state = app.state::<AppState>();
    let (raw_mode, target, result_rx) = {
        let mut session = state.session.lock().unwrap();
        session.phase = Phase::Processing;
        (session.raw, session.target.clone(), session.result_rx.take())
    };
    let _ = app.emit("pill://state", "processing");
    tray::set_recording(app, false);
    play_sound(app, SoundCue::Stop);

    let (ack_tx, _ack_rx) = mpsc::channel();
    let _ = state.audio_tx.send(AudioCmd::Stop(ack_tx));

    let app = app.clone();
    std::thread::spawn(move || {
        let result = match result_rx {
            Some(rx) => process(&app, rx, raw_mode, target),
            None => Err("no active session".into()),
        };
        if let Err(e) = &result {
            tracing::warn!("dictation failed: {e}");
        }
        let state = app.state::<AppState>();
        state.session.lock().unwrap().phase = Phase::Idle;
        hide_pill(&app);
    });
}

fn process(
    app: &AppHandle,
    result_rx: Receiver<SessionResult>,
    raw_mode: bool,
    target: Option<(i32, Option<String>)>,
) -> Result<(), String> {
    let target_pid = target.as_ref().map(|(pid, _)| *pid);
    let target_bundle = target.and_then(|(_, bundle)| bundle);
    let total = Instant::now();

    // The session worker only owes us the tail segment — this should be fast.
    let result = result_rx
        .recv_timeout(Duration::from_secs(120))
        .map_err(|_| "session worker did not deliver a result".to_string())?;
    let audio_secs = result.audio16k.len() as f32 / oratio_core::SAMPLE_RATE as f32;

    let peak = result.audio16k.iter().fold(0.0f32, |a, s| a.max(s.abs()));
    if peak < 1e-5 && audio_secs > 0.5 {
        return Err(format!(
            "microphone delivered pure silence ({audio_secs:.1}s, peak {peak:e}) — \
             macOS microphone permission is likely missing for this process"
        ));
    }

    // Keep the last raw recording on disk for debugging STT/VAD issues.
    let debug_wav = oratio_core::paths::data_dir().join("last_recording.wav");
    if let Err(e) = oratio_core::audio::write_wav_16k(&debug_wav, &result.audio16k) {
        tracing::warn!("could not save debug wav: {e}");
    }

    let state = app.state::<AppState>();
    let (min_speech_ms, dict, restore_ms, polish_cfg, snippets, style) = {
        let s = state.settings.lock().unwrap();
        (
            s.stt.min_speech_ms,
            s.dictionary.clone(),
            s.insertion.restore_clipboard_ms,
            s.polish.clone(),
            s.snippets.clone(),
            s.styles
                .instruction_for(target_bundle.as_deref())
                .map(str::to_string),
        )
    };

    tracing::info!(
        "recording: {audio_secs:.1}s, peak {peak:.4}, speech {}ms, {} segment(s), stt {:.2}s",
        result.speech_ms,
        result.segments,
        result.stt_secs
    );

    if result.speech_ms < min_speech_ms || result.raw_text.trim().is_empty() {
        tracing::info!(
            "no speech in {audio_secs:.1}s recording, ignoring (wav saved to {})",
            debug_wav.display()
        );
        return Ok(());
    }
    let raw_text = result.raw_text;

    // Store the raw transcript immediately — polish can fail, raw must survive.
    let history_id = {
        let stt_model = {
            let s = state.settings.lock().unwrap();
            s.stt.model.clone()
        };
        state.history.lock().unwrap().as_ref().and_then(|h| {
            h.insert_raw(
                &raw_text,
                target_bundle.as_deref(),
                (audio_secs * 1000.0) as i64,
                &stt_model,
            )
            .map_err(|e| tracing::warn!("history insert failed: {e}"))
            .ok()
        })
    };

    let polish_start = Instant::now();
    let final_text = if let Some(snippet) =
        oratio_core::snippets::match_snippet(&raw_text, &snippets)
    {
        tracing::info!("snippet '{}' matched, inserting expansion", snippet.trigger);
        snippet.expansion.clone()
    } else if raw_mode {
        raw_text.clone()
    } else {
        polish_text(&raw_text, &polish_cfg, &dict, style.as_deref())
    };
    let polish_secs = polish_start.elapsed().as_secs_f32();

    if let Some(id) = history_id {
        if final_text != raw_text {
            if let Some(h) = state.history.lock().unwrap().as_ref() {
                let _ = h.set_polished(id, &final_text, None);
            }
        }
    }

    if !crate::permissions::accessibility_ok() {
        crate::permissions::ensure_accessibility_prompt();
        let _ = inject::copy_only(&final_text);
        return Err(format!(
            "text is ready ({} chars) but Accessibility permission is missing — \
             macOS drops synthesized Cmd+V without it. Enable Oratio in System \
             Settings → Privacy & Security → Accessibility. \
             The text was left in the clipboard: paste it with Cmd+V.",
            final_text.chars().count()
        ));
    }
    // Return focus to the app the user dictated into — overlays or pill-button
    // clicks may have stolen it while we were transcribing.
    if let Some(pid) = target_pid {
        if crate::frontmost::activate(pid) {
            std::thread::sleep(Duration::from_millis(180));
        }
    }
    inject::insert_text(&final_text, restore_ms, target_pid).map_err(|e| e.to_string())?;
    tracing::info!(
        "dictation done: {:.1}s audio, stt {:.2}s ({} seg), polish {:.2}s, stop-to-paste {:.2}s, {} chars",
        audio_secs,
        result.stt_secs,
        result.segments,
        polish_secs,
        total.elapsed().as_secs_f32(),
        final_text.chars().count()
    );
    Ok(())
}

/// LLM polish with regex fallback. Never fails: worst case returns the
/// regex-cleaned raw transcript.
fn polish_text(
    raw: &str,
    cfg: &oratio_core::settings::PolishSettings,
    dict: &[String],
    style: Option<&str>,
) -> String {
    if !cfg.enabled {
        return regex_clean::clean(raw);
    }
    let Some(provider_cfg) = cfg
        .providers
        .iter()
        .find(|p| p.id == cfg.active_provider)
        .cloned()
    else {
        tracing::warn!("active polish provider '{}' not found", cfg.active_provider);
        return regex_clean::clean(raw);
    };

    // Self-heal: restart the local Ollama if it died since the last dictation.
    if !oratio_core::polish::ollama::ensure_local_running(&provider_cfg.base_url, 10) {
        tracing::warn!("local polish server unavailable, using regex fallback");
        return regex_clean::clean(raw);
    }

    let provider = OpenAiCompat::new(provider_cfg, cfg.timeout_ms);
    let req = PolishRequest {
        raw,
        style,
        dictionary: dict,
    };
    match provider.polish(&req) {
        Ok(polished) if plausible_output(raw, &polished) => polished,
        Ok(polished) => {
            tracing::warn!(
                "polish output rejected as implausible ({} -> {} chars), using regex fallback",
                raw.chars().count(),
                polished.chars().count()
            );
            regex_clean::clean(raw)
        }
        Err(e) => {
            tracing::warn!("polish failed ({e}), using regex fallback");
            regex_clean::clean(raw)
        }
    }
}

fn load_engine(app: &AppHandle) -> oratio_core::Result<WhisperEngine> {
    let model = {
        let state = app.state::<AppState>();
        let s = state.settings.lock().unwrap();
        s.stt.model.clone()
    };
    let path = models::require(&model)?;
    let start = Instant::now();
    let engine = WhisperEngine::load(&path)?;
    tracing::info!("whisper model '{model}' loaded in {:.2}s", start.elapsed().as_secs_f32());
    Ok(engine)
}

pub fn preload_engine(app: AppHandle) {
    std::thread::spawn(move || {
        let state = app.state::<AppState>();
        let mut guard = state.engine.lock().unwrap();
        if guard.is_none() {
            match load_engine(&app) {
                Ok(engine) => *guard = Some(engine),
                Err(e) => tracing::warn!("model preload failed: {e}"),
            }
        }
    });
}

/// Page the polish LLM into RAM so the first dictation doesn't hit the
/// 10-15s cold-load of a local model.
pub fn warm_up_polish(app: AppHandle) {
    std::thread::spawn(move || {
        let provider_cfg = {
            let state = app.state::<AppState>();
            let s = state.settings.lock().unwrap();
            if !s.polish.enabled {
                return;
            }
            s.polish
                .providers
                .iter()
                .find(|p| p.id == s.polish.active_provider)
                .cloned()
        };
        if let Some(cfg) = provider_cfg {
            oratio_core::polish::ollama::ensure_local_running(&cfg.base_url, 15);
            let start = Instant::now();
            match oratio_core::polish::openai_compat::warm_up(&cfg) {
                Ok(()) => tracing::info!(
                    "polish provider '{}' warmed up in {:.1}s",
                    cfg.id,
                    start.elapsed().as_secs_f32()
                ),
                Err(e) => tracing::warn!("polish warm-up failed ({e}) — first dictation will use regex fallback"),
            }
        }
    });
}

enum SoundCue {
    Start,
    Stop,
}

/// Subtle system-sound cues on record start/stop (macOS).
fn play_sound(app: &AppHandle, cue: SoundCue) {
    let enabled = {
        let state = app.state::<AppState>();
        let s = state.settings.lock().unwrap();
        s.sound_cues
    };
    if !enabled {
        return;
    }
    #[cfg(target_os = "macos")]
    {
        let file = match cue {
            SoundCue::Start => "/System/Library/Sounds/Tink.aiff",
            SoundCue::Stop => "/System/Library/Sounds/Pop.aiff",
        };
        let _ = std::process::Command::new("afplay")
            .args(["-v", "0.3", file])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
    }
}

fn show_pill(app: &AppHandle, state: &str) {
    let _ = app.emit("pill://state", state);
    if let Some(pill) = app.get_webview_window("pill") {
        let _ = pill.show();
    }
}

fn hide_pill(app: &AppHandle) {
    if let Some(pill) = app.get_webview_window("pill") {
        let _ = pill.hide();
    }
}

/// Tray-menu fallback: start a (latched) dictation or finish the current one.
pub fn toggle_dictation(app: &AppHandle) {
    let state = app.state::<AppState>();
    let phase = state.session.lock().unwrap().phase;
    match phase {
        Phase::Idle => {
            on_hotkey_press(app, false);
            let state = app.state::<AppState>();
            state.session.lock().unwrap().latched = true;
        }
        Phase::Recording => finish(app),
        Phase::Processing => {}
    }
}

#[tauri::command]
pub fn stop_dictation(app: AppHandle) {
    let state = app.state::<AppState>();
    let is_recording = state.session.lock().unwrap().phase == Phase::Recording;
    if is_recording {
        finish(&app);
    }
}

#[tauri::command]
pub fn cancel_dictation(app: AppHandle) {
    let state = app.state::<AppState>();
    let mut session = state.session.lock().unwrap();
    if session.phase == Phase::Recording {
        session.phase = Phase::Idle;
        session.latched = false;
        session.result_rx = None;
        drop(session);
        let _ = state.audio_tx.send(AudioCmd::Cancel);
        hide_pill(&app);
        tray::set_recording(&app, false);
        tracing::info!("dictation cancelled");
    }
}

#[derive(serde::Serialize)]
pub struct AppStatus {
    model: String,
    model_loaded: bool,
    hotkey: String,
}

#[tauri::command]
pub fn app_status(state: tauri::State<'_, AppState>) -> AppStatus {
    let settings = state.settings.lock().unwrap();
    AppStatus {
        model: settings.stt.model.clone(),
        model_loaded: state.engine.lock().unwrap().is_some(),
        hotkey: settings.hotkeys.main.clone(),
    }
}

fn set_phase(app: &AppHandle, phase: Phase) {
    let state = app.state::<AppState>();
    state.session.lock().unwrap().phase = phase;
    if phase == Phase::Idle {
        hide_pill(app);
        tray::set_recording(app, false);
    }
}
