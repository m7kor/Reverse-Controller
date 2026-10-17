//! Native offline Voice Typing / Speech-to-Text using Whisper and CPAL.
//! 100% Open Source, runs entirely on CPU without Windows Voice Typing (Win+H)
//! and without requiring online cloud services. Fully supports Arabic (Saudi Arabia) and English!

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

/// In-memory cached Whisper context. Preloaded at startup so inference begins immediately
/// without having to re-read the 77MB model from disk on every sentence.
static WHISPER_CTX: Mutex<Option<WhisperContext>> = Mutex::new(None);

/// Peak audio level scaled 0..1000 for UI meters and pulse animations.
static AUDIO_LEVEL: AtomicU32 = AtomicU32::new(0);

/// True when actively recording from the microphone.
static IS_RECORDING: AtomicBool = AtomicBool::new(false);

/// True when running Whisper inference.
static IS_TRANSCRIBING: AtomicBool = AtomicBool::new(false);
// UI and VAD can stop the same recording at the same time.
static RECORDING_TRANSITION: Mutex<()> = Mutex::new(());
static RECORDING_SESSION: AtomicU64 = AtomicU64::new(0);

fn session_matches(expected: Option<u64>, current: u64) -> bool {
    expected.is_none_or(|id| id == current)
}

/// Status message for UI feedback.
static STATUS_MSG: Mutex<Option<String>> = Mutex::new(None);

/// Last successfully transcribed text.
static LAST_TRANSCRIBED: Mutex<Option<String>> = Mutex::new(None);

/// Global handle to active input stream (kept alive while recording).
static ACTIVE_STREAM: Mutex<Option<cpal::Stream>> = Mutex::new(None);

/// Buffer of raw audio samples captured during recording.
static CAPTURED_SAMPLES: Mutex<Vec<f32>> = Mutex::new(Vec::new());

/// Active capture format (channels, sample_rate).
static CAPTURE_SPEC: Mutex<(u16, u32)> = Mutex::new((1, 48000));

// --- Voice Activity Detection (VAD) and Target Window Preservation ---
static TARGET_HWND: AtomicUsize = AtomicUsize::new(0);
static HAD_SPEECH: AtomicBool = AtomicBool::new(false);
static LAST_SPEECH_INSTANT: Mutex<Option<Instant>> = Mutex::new(None);
static RECORD_START_INSTANT: Mutex<Option<Instant>> = Mutex::new(None);

/// Global cached voice settings synchronized from active Config.
static VOICE_SETTINGS: Mutex<Option<crate::config::VoiceSettings>> = Mutex::new(None);

pub fn set_voice_settings(settings: crate::config::VoiceSettings) {
    if let Ok(mut lock) = VOICE_SETTINGS.lock() {
        *lock = Some(settings);
    }
}

pub fn get_voice_settings() -> crate::config::VoiceSettings {
    VOICE_SETTINGS.lock().ok().and_then(|l| l.clone()).unwrap_or_default()
}

pub fn is_recording() -> bool {
    IS_RECORDING.load(Ordering::SeqCst)
}

pub fn is_transcribing() -> bool {
    IS_TRANSCRIBING.load(Ordering::SeqCst)
}

pub fn had_speech() -> bool {
    HAD_SPEECH.load(Ordering::SeqCst)
}

pub fn audio_level() -> f32 {
    AUDIO_LEVEL.load(Ordering::Relaxed) as f32 / 1000.0
}

pub fn status_message() -> Option<String> {
    STATUS_MSG.lock().ok()?.clone()
}

pub fn last_transcribed() -> Option<String> {
    LAST_TRANSCRIBED.lock().ok()?.clone()
}

pub fn take_last_transcribed() -> Option<String> {
    LAST_TRANSCRIBED.lock().ok()?.take()
}

/// Locate the Whisper model file on disk.
pub fn model_path() -> Option<PathBuf> {
    let mut search_dirs = Vec::new();

    if let Ok(appdata) = std::env::var("APPDATA") {
        search_dirs.push(PathBuf::from(appdata).join("ControllerCompanionRust").join("models"));
    }

    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            search_dirs.push(parent.join("models"));
            search_dirs.push(parent.to_path_buf());
        }
    }

    search_dirs.push(PathBuf::from("models"));

    let settings = get_voice_settings();
    let mut candidates = Vec::new();
    let pref = settings.model.trim();
    if !pref.is_empty() {
        candidates.push(pref.to_string());
    }
    for &default_name in &["ggml-base.bin", "ggml-tiny.bin", "ggml-small.bin", "ggml-medium.bin"] {
        if !candidates.iter().any(|c| c == default_name) {
            candidates.push(default_name.to_string());
        }
    }

    for dir in &search_dirs {
        for name in &candidates {
            let p = dir.join(name);
            if p.exists() {
                return Some(p);
            }
        }
    }
    None
}

static LAST_VOICE_ACTIVITY: Mutex<Option<Instant>> = Mutex::new(None);

pub fn touch_voice_activity() {
    if let Ok(mut lock) = LAST_VOICE_ACTIVITY.lock() {
        *lock = Some(Instant::now());
    }
}

/// Unload the Whisper model from RAM when idle to reclaim ~120MB of memory.
pub fn unload_model() {
    if let Ok(mut lock) = WHISPER_CTX.lock() {
        if lock.take().is_some() {
            crate::platform::log_debug("[voice] Whisper model unloaded from RAM (memory freed)");
            crate::platform::trim_working_set();
        }
    }
}

/// Check if voice has been idle for more than 75 seconds, and if so, unload it to save RAM.
pub fn check_idle_unload() {
    if is_recording() || is_transcribing() {
        return;
    }
    let should_unload = {
        if let Ok(lock) = LAST_VOICE_ACTIVITY.lock() {
            if let Some(t) = *lock {
                t.elapsed() > Duration::from_secs(75)
            } else {
                false
            }
        } else {
            false
        }
    };
    if should_unload {
        unload_model();
        if let Ok(mut lock) = LAST_VOICE_ACTIVITY.lock() {
            *lock = None;
        }
    }
}

/// Pre-warm the Whisper model in the background only if explicitly requested.
pub fn prewarm_model() {
    let _ = ensure_model_loaded();
}

/// Ensure the Whisper context is loaded into RAM.
pub fn ensure_model_loaded() -> bool {
    touch_voice_activity();
    let mut lock = match WHISPER_CTX.lock() {
        Ok(l) => l,
        Err(_) => return false,
    };
    if lock.is_some() {
        return true;
    }
    if let Some(path) = model_path() {
        if let Some(path_str) = path.to_str() {
            crate::platform::log_debug(&format!("[voice] Loading Whisper model into RAM: {path_str}"));
            let params = WhisperContextParameters::default();
            match WhisperContext::new_with_params(path_str, params) {
                Ok(ctx) => {
                    *lock = Some(ctx);
                    crate::platform::log_debug("[voice] Whisper model cached in RAM successfully");
                    return true;
                }
                Err(e) => {
                    crate::platform::log_debug(&format!("[voice] Failed to initialize WhisperContext: {e:?}"));
                }
            }
        }
    }
    false
}

/// Put Unicode text onto the Windows Clipboard using native Win32 API.
pub fn set_clipboard_text(text: &str) -> bool {
    #[link(name = "user32")]
    extern "system" {
        fn OpenClipboard(hWndNewOwner: *mut std::ffi::c_void) -> i32;
        fn CloseClipboard() -> i32;
        fn EmptyClipboard() -> i32;
        fn SetClipboardData(uFormat: u32, hMem: *mut std::ffi::c_void) -> *mut std::ffi::c_void;
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn GlobalAlloc(uFlags: u32, dwBytes: usize) -> *mut std::ffi::c_void;
        fn GlobalLock(hMem: *mut std::ffi::c_void) -> *mut std::ffi::c_void;
        fn GlobalUnlock(hMem: *mut std::ffi::c_void) -> i32;
    }

    const CF_UNICODETEXT: u32 = 13;
    const GMEM_MOVEABLE: u32 = 0x0002;

    let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    let bytes = wide.len() * 2;

    unsafe {
        let mut opened = false;
        for _ in 0..10 {
            if OpenClipboard(std::ptr::null_mut()) != 0 {
                opened = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        if !opened {
            return false;
        }

        EmptyClipboard();
        let hmem = GlobalAlloc(GMEM_MOVEABLE, bytes);
        if !hmem.is_null() {
            let ptr = GlobalLock(hmem) as *mut u16;
            if !ptr.is_null() {
                std::ptr::copy_nonoverlapping(wide.as_ptr(), ptr, wide.len());
                GlobalUnlock(hmem);
                let _ = SetClipboardData(CF_UNICODETEXT, hmem);
            }
        }
        CloseClipboard();
    }
    true
}

/// Directly inject raw text into the focused application using batched KEYEVENTF_UNICODE keystrokes.
/// This works across 100% of Windows apps (Chrome, Discord, Notepad, Games, Terminals)
/// and does not get locked by OS clipboard listeners.
pub fn send_unicode_text(text: &str) {
    let wide_units: Vec<u16> = text.encode_utf16().collect();
    if wide_units.is_empty() {
        return;
    }
    let mut inputs = Vec::with_capacity(wide_units.len() * 2);
    for &u in &wide_units {
        inputs.push(crate::win32::key_input(0, u, crate::win32::KEYEVENTF_UNICODE));
        inputs.push(crate::win32::key_input(
            0,
            u,
            crate::win32::KEYEVENTF_UNICODE | crate::win32::KEYEVENTF_KEYUP,
        ));
    }
    crate::win32::send_inputs(&inputs);
}

pub struct ProcessedVoice {
    pub text: String,
    pub auto_send: bool,
}

/// Parse and apply natural voice command macros (e.g. "ارسل" -> send Enter, "سطر جديد" -> newline).
pub fn process_voice_macros(raw: &str) -> ProcessedVoice {
    let mut text = raw.trim().to_string();
    let mut auto_send = false;
    let settings = get_voice_settings();
    if !settings.enable_macros {
        return ProcessedVoice { text, auto_send };
    }

    // Detect spoken send trigger at the end of the sentence
    let send_keywords = [
        "ارسل", "إرسال", "أرسل", "انتر", "إدخال", "ادخال",
        "send", "Send", "enter", "Enter",
    ];
    for kw in send_keywords {
        if text.ends_with(kw) {
            let cut = text.len() - kw.len();
            text.truncate(cut);
            text = text.trim_end_matches([' ', '،', ',', '.', '!', '؟', '?']).to_string();
            auto_send = true;
            break;
        }
    }

    // Replace spoken newlines (cleaning surrounding spaces)
    text = text
        .replace(" سطر جديد ", "\n")
        .replace(" سطر جديد", "\n")
        .replace("سطر جديد ", "\n")
        .replace("سطر جديد", "\n")
        .replace(" السطر الجديد ", "\n")
        .replace("السطر الجديد", "\n")
        .replace(" new line ", "\n")
        .replace(" new line", "\n")
        .replace("new line ", "\n")
        .replace("new line", "\n")
        .replace(" New line ", "\n")
        .replace(" New line", "\n")
        .replace("New line ", "\n")
        .replace("New line", "\n")
        .replace("newline", "\n");

    // Replace spoken punctuation and formatting if explicitly spoken
    text = text
        .replace(" فاصلة منقوطة", "؛")
        .replace("فاصلة منقوطة", "؛")
        .replace(" فاصلة", "،")
        .replace("فاصلة", "،")
        .replace(" نقطتان", ":")
        .replace("نقطتان", ":")
        .replace(" نقطتين", ":")
        .replace("نقطتين", ":")
        .replace(" نقطة", ".")
        .replace(" علامة استفهام", "؟")
        .replace("علامة استفهام", "؟")
        .replace(" علامة تعجب", "!")
        .replace("علامة تعجب", "!")
        .replace(" بين قوسين", " ()")
        .replace("بين قوسين", "()")
        .replace(" شرطة", " -")
        .replace("شرطة ", "- ")
        .replace(" خط مائل", " /")
        .replace(" مسافة", " ");

    ProcessedVoice { text, auto_send }
}

/// Inject voice text directly into the target application.
/// Ensures the active window is restored, updates the clipboard, and delivers the keystrokes.
pub fn inject_voice_text(text: &str) {
    crate::platform::attach_to_interactive_desktop();

    let processed = process_voice_macros(text);
    if processed.text.is_empty() {
        return;
    }

    // 1. Restore the application that had keyboard focus when dictation was started
    let target = TARGET_HWND.load(Ordering::SeqCst);
    if target != 0 {
        crate::platform::set_foreground_window_force(target);
    } else {
        crate::platform::restore_foreground();
    }
    std::thread::sleep(Duration::from_millis(40));

    // 2. Keep text in Windows clipboard as backup
    let copied = set_clipboard_text(&processed.text);
    crate::platform::log_debug(&format!(
        "[voice] Direct injection of {} chars into fg={target} (clipboard={copied}, auto_send={})",
        processed.text.chars().count(),
        processed.auto_send
    ));

    // 3. Audio & Haptic confirmation
    crate::sound::play_sfx(crate::sound::SoundEffect::SendSuccess);
    crate::engine::trigger_haptic(10000, 14000, 0.04);

    // 4. Directly type Unicode characters into the focused control!
    send_unicode_text(&processed.text);

    // 5. If "ارسل" / "send" macro was requested, send Enter key!
    if processed.auto_send {
        std::thread::sleep(Duration::from_millis(35));
        let enter_down = crate::win32::key_input(0x0D, 0x1C, 0);
        let enter_up = crate::win32::key_input(0x0D, 0x1C, crate::win32::KEYEVENTF_KEYUP);
        crate::win32::send_inputs(&[enter_down, enter_up]);
        crate::platform::log_debug("[voice] Voice macro: sent Enter to submit message");
    }
}

/// Start recording audio from the default microphone with real-time VAD.
pub fn start_recording(lang_ar: bool) -> Result<(), String> {
    let _transition = RECORDING_TRANSITION.lock().map_err(|_| "Recording state unavailable")?;
    if is_transcribing() {
        return Err("Wait for the current transcription to finish".into());
    }
    if is_recording() {
        return Ok(());
    }
    if model_path().is_none() {
        return Err("Install ggml-base.bin or ggml-tiny.bin in %APPDATA%\\ControllerCompanionRust\\models before recording".into());
    }

    // Play listening chime and subtle haptic pulse
    crate::sound::play_sfx(crate::sound::SoundEffect::ListenStart);
    crate::engine::trigger_haptic(8000, 10000, 0.035);

    // Save target window before keyboard or UI steals focus
    let fg = crate::platform::push_foreground();
    TARGET_HWND.store(fg, Ordering::SeqCst);

    let host = cpal::default_host();
    let device = host
        .default_input_device()
        .ok_or_else(|| "No microphone input device found".to_string())?;

    let dev_name = device
        .description()
        .map(|d| d.name().to_string())
        .unwrap_or_else(|_| "Microphone".to_string());

    let config = device
        .default_input_config()
        .map_err(|e| format!("Failed to get microphone config: {e}"))?;

    let sample_rate = config.sample_rate();
    let channels = config.channels();

    {
        if let Ok(mut samples) = CAPTURED_SAMPLES.lock() {
            samples.clear();
            samples.reserve(48000 * 30); // Pre-allocate buffer for up to 30 seconds
        }
        if let Ok(mut spec) = CAPTURE_SPEC.lock() {
            *spec = (channels, sample_rate);
        }
    }

    HAD_SPEECH.store(false, Ordering::SeqCst);
    if let Ok(mut t) = LAST_SPEECH_INSTANT.lock() {
        *t = None;
    }
    if let Ok(mut t) = RECORD_START_INSTANT.lock() {
        *t = Some(Instant::now());
    }

    let err_fn = |err| {
        let s = format!("{err}");
        if !s.contains("underrun") && !s.contains("overrun") {
            crate::platform::log_debug(&format!("[voice] Audio stream error: {err}"));
        }
    };

    const SPEECH_THRESHOLD: f32 = 0.022;

    let stream = match config.sample_format() {
        cpal::SampleFormat::F32 => device.build_input_stream(
            config.into(),
            move |data: &[f32], _| {
                let mut max_val: f32 = 0.0;
                let mut sum_sq: f32 = 0.0;
                if let Ok(mut lock) = CAPTURED_SAMPLES.lock() {
                    lock.extend_from_slice(data);
                }
                for &s in data {
                    let a = s.abs();
                    if a > max_val {
                        max_val = a;
                    }
                    sum_sq += s * s;
                }
                let rms = (sum_sq / data.len().max(1) as f32).sqrt();
                let scaled = (max_val.min(1.0) * 1000.0) as u32;
                AUDIO_LEVEL.store(scaled, Ordering::Relaxed);

                if rms > SPEECH_THRESHOLD || max_val > 0.045 {
                    HAD_SPEECH.store(true, Ordering::SeqCst);
                    if let Ok(mut t) = LAST_SPEECH_INSTANT.lock() {
                        *t = Some(Instant::now());
                    }
                }
            },
            err_fn,
            None,
        ),
        cpal::SampleFormat::I16 => device.build_input_stream(
            config.into(),
            move |data: &[i16], _| {
                let mut max_val: f32 = 0.0;
                let mut sum_sq: f32 = 0.0;
                if let Ok(mut lock) = CAPTURED_SAMPLES.lock() {
                    for &s in data {
                        let f = s as f32 / 32768.0;
                        lock.push(f);
                        let a = f.abs();
                        if a > max_val {
                            max_val = a;
                        }
                        sum_sq += f * f;
                    }
                }
                let rms = (sum_sq / data.len().max(1) as f32).sqrt();
                let scaled = (max_val.min(1.0) * 1000.0) as u32;
                AUDIO_LEVEL.store(scaled, Ordering::Relaxed);

                if rms > SPEECH_THRESHOLD || max_val > 0.045 {
                    HAD_SPEECH.store(true, Ordering::SeqCst);
                    if let Ok(mut t) = LAST_SPEECH_INSTANT.lock() {
                        *t = Some(Instant::now());
                    }
                }
            },
            err_fn,
            None,
        ),
        sample_format => {
            return Err(format!("Unsupported audio sample format: {sample_format:?}"));
        }
    }
    .map_err(|e| format!("Failed to build audio stream: {e}"))?;

    stream
        .play()
        .map_err(|e| format!("Failed to start recording stream: {e}"))?;
    *ACTIVE_STREAM.lock().map_err(|_| "Audio stream state unavailable")? = Some(stream);
    let session = RECORDING_SESSION.fetch_add(1, Ordering::SeqCst).wrapping_add(1);
    IS_RECORDING.store(true, Ordering::SeqCst);
    if let Ok(mut msg) = STATUS_MSG.lock() {
        *msg = Some(format!("🎙 {dev_name} (Listening...)"));
    }
    crate::platform::log_debug(&format!(
        "[voice] Recording started on {dev_name} ({sample_rate}Hz, {channels}ch) target_fg={fg}"
    ));

    // VAD Auto-Stop Monitor thread: Automatically stops and injects when user pauses
    std::thread::spawn(move || {
        while is_recording() && session_matches(Some(session), RECORDING_SESSION.load(Ordering::SeqCst)) {
            std::thread::sleep(Duration::from_millis(50));
            if !is_recording() || !session_matches(Some(session), RECORDING_SESSION.load(Ordering::SeqCst)) {
                break;
            }

            let now = Instant::now();
            let had_speech = HAD_SPEECH.load(Ordering::SeqCst);

            // 1. Auto-stop when silence exceeds configured vad_silence_ms after speech detected
            if had_speech {
                let silence_limit = Duration::from_millis(get_voice_settings().vad_silence_ms.clamp(400, 5000));
                let last = LAST_SPEECH_INSTANT.lock().ok().and_then(|t| *t);
                if let Some(last_time) = last {
                    if now.saturating_duration_since(last_time) >= silence_limit {
                        crate::platform::log_debug(&format!(
                            "[voice] VAD: {}ms silence after speech -> auto-stopping & sending text",
                            silence_limit.as_millis()
                        ));
                        stop_session(lang_ar, Some(session));
                        break;
                    }
                }
            }

            // 2. Safety max recording duration: 25 seconds
            let start = RECORD_START_INSTANT.lock().ok().and_then(|t| *t);
            if let Some(start_time) = start {
                if now.saturating_duration_since(start_time) >= Duration::from_secs(25) {
                    crate::platform::log_debug("[voice] Max recording duration (25s) reached -> auto-stopping");
                    stop_session(lang_ar, Some(session));
                    break;
                }
            }
        }
    });

    Ok(())
}

/// Stop recording and kick off Whisper transcription on a background worker thread.
pub fn stop_and_transcribe(lang_ar: bool) {
    stop_session(lang_ar, None);
}

fn stop_session(lang_ar: bool, expected_session: Option<u64>) {
    let Ok(_transition) = RECORDING_TRANSITION.lock() else { return; };
    if !is_recording() || !session_matches(expected_session, RECORDING_SESSION.load(Ordering::SeqCst)) {
        return;
    }

    if let Ok(mut stream_lock) = ACTIVE_STREAM.lock() {
        let _ = stream_lock.take();
    }
    IS_RECORDING.store(false, Ordering::SeqCst);
    AUDIO_LEVEL.store(0, Ordering::Relaxed);

    let (samples, channels, sample_rate) = {
        let s = if let Ok(mut raw) = CAPTURED_SAMPLES.lock() {
            std::mem::take(&mut *raw)
        } else {
            Vec::new()
        };
        let spec = CAPTURE_SPEC.lock().ok().map(|s| *s).unwrap_or((1, 48000));
        (s, spec.0, spec.1)
    };

    if samples.is_empty() {
        if let Ok(mut msg) = STATUS_MSG.lock() {
            *msg = Some("No audio recorded".to_string());
        }
        return;
    }

    IS_TRANSCRIBING.store(true, Ordering::SeqCst);
    if let Ok(mut msg) = STATUS_MSG.lock() {
        *msg = Some("Transcribing...".to_string());
    }

    std::thread::spawn(move || {
        crate::platform::attach_to_interactive_desktop();
        let t0 = Instant::now();
        let result = process_and_transcribe(samples, channels, sample_rate, lang_ar);
        let elapsed = t0.elapsed();
        touch_voice_activity();

        match result {
            Ok(text) => {
                let trimmed = text.trim();
                if !trimmed.is_empty() {
                    crate::platform::log_debug(&format!(
                        "[voice] Transcription completed in {:.2}s",
                        elapsed.as_secs_f32()
                    ));
                    if let Ok(mut last) = LAST_TRANSCRIBED.lock() {
                        *last = Some(trimmed.to_string());
                    }
                    if let Ok(mut msg) = STATUS_MSG.lock() {
                        *msg = Some(format!("Typed: {trimmed}"));
                    }
                    // Reliably write text directly into the active target application!
                    inject_voice_text(trimmed);
                } else {
                    crate::platform::log_debug(&format!(
                        "[voice] No speech detected (elapsed={:.2}s)",
                        elapsed.as_secs_f32()
                    ));
                    if let Ok(mut msg) = STATUS_MSG.lock() {
                        *msg = Some("No speech detected".to_string());
                    }
                }
            }
            Err(e) => {
                crate::platform::log_debug(&format!("[voice] Error: {e}"));
                if let Ok(mut msg) = STATUS_MSG.lock() {
                    *msg = Some(format!("Error: {e}"));
                }
            }
        }
        IS_TRANSCRIBING.store(false, Ordering::SeqCst);
        crate::platform::trim_working_set();
    });
}

/// Toggle voice typing: starts recording if idle, stops and transcribes if recording.
pub fn toggle_voice_typing(lang_ar: bool) {
    if is_recording() {
        stop_and_transcribe(lang_ar);
    } else if !is_transcribing() {
        if let Err(e) = start_recording(lang_ar) {
            if let Ok(mut msg) = STATUS_MSG.lock() {
                *msg = Some(format!("Mic error: {e}"));
            }
        }
    }
}

/// Resample incoming audio from any rate and channel count to 16,000 Hz mono f32.
fn resample_to_16k_mono(samples: &[f32], channels: u16, sample_rate: u32) -> Vec<f32> {
    if samples.is_empty() || channels == 0 || sample_rate == 0 {
        return Vec::new();
    }

    let ch = channels.max(1) as usize;
    let mono_len = samples.len() / ch;
    if mono_len == 0 { return Vec::new(); }
    let mut mono = Vec::with_capacity(mono_len);
    for frame in samples.chunks_exact(ch) {
        let sum: f32 = frame.iter().sum();
        mono.push(sum / ch as f32);
    }

    if sample_rate == 16000 {
        return mono;
    }

    let ratio = sample_rate as f64 / 16000.0;
    let target_len = ((mono.len() as f64) / ratio).round() as usize;
    let mut resampled = Vec::with_capacity(target_len);

    for i in 0..target_len {
        let src_idx = (i as f64) * ratio;
        let idx0 = src_idx.floor() as usize;
        let idx1 = (idx0 + 1).min(mono.len() - 1);
        let frac = (src_idx - idx0 as f64) as f32;
        if idx0 < mono.len() {
            let s0 = mono[idx0];
            let s1 = mono[idx1];
            resampled.push(s0 + (s1 - s0) * frac);
        }
    }

    resampled
}

/// Remove DC offset and normalize peak amplitude to optimal Whisper input levels (-1.0 to 1.0).
fn normalize_audio(samples: &mut [f32]) {
    if samples.is_empty() {
        return;
    }
    // 1. Remove DC bias (centre around 0.0)
    let sum: f32 = samples.iter().sum();
    let mean = sum / samples.len() as f32;
    for s in samples.iter_mut() {
        *s -= mean;
    }

    // 2. Find peak absolute level
    let mut max_abs: f32 = 0.0;
    for &s in samples.iter() {
        let a = s.abs();
        if a > max_abs {
            max_abs = a;
        }
    }

    // 3. Apply soft AGC gain so quiet microphones deliver crisp, clear speech to Whisper
    if max_abs > 0.005 {
        let target_peak = 0.85;
        let gain = (target_peak / max_abs).clamp(1.0, 12.0);
        for s in samples.iter_mut() {
            *s = (*s * gain).clamp(-1.0, 1.0);
        }
    }
}

fn process_and_transcribe(
    raw_samples: Vec<f32>,
    channels: u16,
    sample_rate: u32,
    lang_ar: bool,
) -> Result<String, String> {
    let mut pcm_16k = resample_to_16k_mono(&raw_samples, channels, sample_rate);
    // Ignore recordings shorter than 0.25 seconds
    if pcm_16k.len() < 4000 {
        return Ok(String::new());
    }

    // Normalize audio signal (DC offset removal + soft gain)
    normalize_audio(&mut pcm_16k);

    // Ensure model is resident in RAM
    if !ensure_model_loaded() {
        return Err("Whisper model not found in %APPDATA%\\ControllerCompanionRust\\models".to_string());
    }

    let lock = WHISPER_CTX.lock().map_err(|e| format!("Whisper lock error: {e}"))?;
    let ctx = lock
        .as_ref()
        .ok_or_else(|| "WhisperContext not available".to_string())?;

    let mut state = ctx
        .create_state()
        .map_err(|e| format!("Failed to create Whisper state: {e:?}"))?;

    let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
    // Force specific language code: "ar" or "en". Never pass "auto" on tiny models for short dictation!
    let settings = get_voice_settings();
    let effective_ar = match settings.language.as_str() {
        "ar" => true,
        "en" => false,
        _ => lang_ar,
    };
    let target_lang = if effective_ar { "ar" } else { "en" };
    params.set_language(Some(target_lang));
    params.set_translate(false);
    params.set_no_context(true);
    params.set_single_segment(true);
    params.set_print_special(false);
    params.set_print_progress(false);
    params.set_print_realtime(false);
    params.set_print_timestamps(false);
    params.set_suppress_blank(true);
    params.set_temperature(0.0);

    // Initial prompt primes Whisper's token priors for the exact language domain and vocabulary
    let mut initial_prompt = if effective_ar {
        "مرحبا، السلام عليكم، أكتب نصاً باللغة العربية بوضوح ودقة.".to_string()
    } else {
        "Hello, typing clear and accurate English text.".to_string()
    };
    if !settings.custom_vocabulary.is_empty() {
        initial_prompt.push(' ');
        initial_prompt.push_str(&settings.custom_vocabulary.join(", "));
    }
    params.set_initial_prompt(&initial_prompt);

    let n_threads = std::thread::available_parallelism()
        .map(|n| (n.get() as i32).clamp(1, 8))
        .unwrap_or(4);
    params.set_n_threads(n_threads);

    state
        .full(params, &pcm_16k)
        .map_err(|e| format!("Whisper inference error: {e:?}"))?;

    let mut out = String::new();
    for segment in state.as_iter() {
        if let Ok(seg_text) = segment.to_str() {
            out.push_str(seg_text);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stale_monitor_cannot_stop_a_new_recording() {
        assert!(session_matches(Some(7), 7));
        assert!(!session_matches(Some(7), 8));
        assert!(session_matches(None, 8));
    }

    #[test]
    fn resampling_rejects_invalid_formats_and_incomplete_frames() {
        assert!(resample_to_16k_mono(&[0.5], 2, 48000).is_empty());
        assert!(resample_to_16k_mono(&[0.5], 1, 0).is_empty());
        assert!(resample_to_16k_mono(&[0.5], 0, 16000).is_empty());
        assert!(resample_to_16k_mono(&[], 1, 16000).is_empty());
    }

    #[test]
    fn resampling_downmixes_stereo_and_preserves_duration() {
        assert_eq!(resample_to_16k_mono(&[1.0, -1.0, 0.5, 0.5], 2, 16000), vec![0.0, 0.5]);
        let mono = resample_to_16k_mono(&vec![0.25; 48000], 1, 48000);
        assert_eq!(mono.len(), 16000);
        assert!(mono.iter().all(|&sample| (sample - 0.25).abs() < f32::EPSILON));
    }

    #[test]
    fn test_voice_macros_arabic_send() {
        let res = process_voice_macros("السلام عليكم ارسل");
        assert!(res.auto_send);
        assert_eq!(res.text, "السلام عليكم");

        let res2 = process_voice_macros("هلا والله إرسال");
        assert!(res2.auto_send);
        assert_eq!(res2.text, "هلا والله");
    }

    #[test]
    fn test_voice_macros_english_send() {
        let res = process_voice_macros("Hello everyone send");
        assert!(res.auto_send);
        assert_eq!(res.text, "Hello everyone");
    }

    #[test]
    fn test_voice_macros_newline_and_punctuation() {
        let res = process_voice_macros("مرحبا سطر جديد كيف الحال فاصلة تمام علامة استفهام ممتاز علامة تعجب");
        assert!(!res.auto_send);
        assert_eq!(res.text, "مرحبا\nكيف الحال، تمام؟ ممتاز!");

        let res2 = process_voice_macros("النتيجة نقطتان خمسة بين قوسين");
        assert_eq!(res2.text, "النتيجة: خمسة ()");

        // When macros are disabled
        let mut settings = get_voice_settings();
        settings.enable_macros = false;
        set_voice_settings(settings);
        let res_off = process_voice_macros("مرحبا ارسل");
        assert!(!res_off.auto_send);
        assert_eq!(res_off.text, "مرحبا ارسل");

        // Restore
        let mut settings_restore = get_voice_settings();
        settings_restore.enable_macros = true;
        set_voice_settings(settings_restore);
    }

    #[test]
    fn test_normalize_audio() {
        let mut samples = vec![0.05, 0.15, 0.05, -0.05];
        normalize_audio(&mut samples);
        let max_abs = samples.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
        assert!((max_abs - 0.85).abs() < 0.02, "audio normalization must boost quiet peak to target level");
        let sum: f32 = samples.iter().sum();
        assert!(sum.abs() < 1e-5, "DC offset must be eliminated");
    }
}

