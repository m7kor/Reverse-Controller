//! Controller Companion — native Rust port (core engine).
//!
//! Steam features are removed by request. See docs/PORT_NOTES.md for the full
//! feature inventory, what is implemented, and what is best-effort.

// The Win32 FFI layer deliberately mirrors the spelling of the Windows API
// (HWND, MSG, KEYBDINPUT, ...). Renaming them would make the bindings harder to
// match against Microsoft's documentation, so this lint is off crate-wide.
#![windows_subsystem = "windows"]
#![allow(clippy::upper_case_acronyms)]

// Console output is diagnostics only. std's print macros panic when stdout or
// stderr is a closed pipe (e.g. the terminal that started the app was closed),
// and release builds abort on panic, so every module uses these versions,
// which drop the text instead.
macro_rules! println {
    ($($t:tt)*) => {{
        use std::io::Write as _;
        let _ = writeln!(std::io::stdout(), $($t)*);
    }};
}
macro_rules! eprintln {
    ($($t:tt)*) => {{
        use std::io::Write as _;
        let _ = writeln!(std::io::stderr(), $($t)*);
    }};
}
macro_rules! print {
    ($($t:tt)*) => {{
        use std::io::Write as _;
        let _ = write!(std::io::stdout(), $($t)*);
    }};
}

mod actions;
pub mod amgp;
#[cfg(feature = "gui")]
pub mod arabic;
mod bindings;
mod config;
#[cfg(feature = "gui")]
mod calibration;
mod doctor;
mod engine;
mod gamepads;
mod install;
#[cfg(feature = "gui")]
mod gui;
mod keys;
mod mouse;
pub mod osd;
mod platform;
pub mod predict;
mod profiles;
mod runtime_status;
mod storage;
pub mod sound;
mod tray;
mod vknav;
#[cfg(feature = "gui")]
mod vkeyboard;
mod win32;

use config::{default_config_path, Config};
use engine::Engine;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

fn print_usage() {
    println!(
        "controller-companion (Rust port)\n\n\
         USAGE:\n  \
         controller-companion [OPTIONS]\n\n\
         OPTIONS:\n  \
         --config <path>   Path to config.json (default: %APPDATA%\\ControllerCompanionRust\\config.json)\n  \
         --write-default   Write the default config file (with default bindings) and exit\n  \
         --print-default   Print the default binding table as JSON and exit\n  \
         --status          Print XInput controller status and exit\n  \
         --validate-config Validate the selected config without starting input\n  \
         --doctor          Write a troubleshooting report (also saved next to config.json)\n  \
         --install         Install for this user (Start menu, Installed apps) and start\n  \
         --uninstall       Remove the installed copy (settings are kept); --quiet skips prompts\n  \
         --help            Show this help\n"
    );
}

fn main() {
    std::panic::set_hook(Box::new(|info| {
        let msg = format!("[PANIC] {info}");
        platform::log_debug(&msg);
        eprintln!("{msg}");
        // Release builds abort right after this hook; let go of anything the
        // app was holding so no key or button stays stuck system-wide.
        platform::release_stuck_input();
    }));
    platform::attach_to_interactive_desktop();
    unsafe {
        #[link(name = "kernel32")]
        extern "system" {
            fn GetConsoleWindow() -> *mut std::ffi::c_void;
        }
        #[link(name = "user32")]
        extern "system" {
            fn ShowWindow(hwnd: *mut std::ffi::c_void, cmd: i32) -> i32;
        }
        let console = GetConsoleWindow();
        if !console.is_null() {
            let has_cli = std::env::args().any(|a| matches!(a.as_str(), "--help" | "-h" | "--status" | "--doctor"));
            if !has_cli {
                ShowWindow(console, 0); // SW_HIDE
            }
        }
    }
    let elevated = platform::is_elevated();
    platform::log_debug(&format!("[main] Starting Controller Companion PID={} elevated={elevated}", std::process::id()));
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut config_path = default_config_path();
    let mut write_default = false;
    let mut print_default = false;
    let mut status_only = false;
    let mut validate_config = false;
    let mut doctor = false;
    let mut stdin_exit = false;
    #[cfg(feature = "gui")]
    let mut demo_kb = false;
    #[cfg(feature = "gui")]
    let mut demo_kb_ar = false;
    let mut open_standalone_settings = false;
    let mut trace_pad = false;
    let mut no_elevate = false;
    let mut from_autostart = false;
    let mut install_now = false;
    let mut uninstall_now = false;
    let mut quiet = false;
    let mut inject_pad: Option<(i16, i16, i16, i16, u16)> = None;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--help" | "-h" => {
                print_usage();
                return;
            }
            "--write-default" => write_default = true,
            "--print-default" => print_default = true,
            "--status" => status_only = true,
            "--validate-config" => validate_config = true,
            "--doctor" => doctor = true,
            "--stdin-exit" => stdin_exit = true,
            "--caret-probe" => {
                // Diagnostics: sample the caret heuristic for a few seconds.
                for _ in 0..15 {
                    println!("caret_active = {}", platform::caret_active());
                    std::thread::sleep(Duration::from_millis(200));
                }
                return;
            }
            #[cfg(feature = "gui")]
            "--demo-kb" => demo_kb = true,
            // Diagnostics: print the raw pad state once per second so it is
            // obvious whether input is lost at XInput, the dead zone, or the
            // fullscreen/disabled gate.
            "--trace-pad" => trace_pad = true,
            // `--inject-pad <lx,ly,rx,ry[,buttons]>` replaces the real gamepad
            // with a fixed state, so the whole input pipeline can be exercised
            // without a controller attached (and so a direction regression is a
            // test failure rather than something only the user can notice).
            "--inject-pad" => {
                i += 1;
                let spec = args.get(i).cloned().unwrap_or_default();
                match parse_inject_spec(&spec) {
                    Some((lx, ly, rx, ry, buttons)) => {
                        inject_pad = Some((lx, ly, rx, ry, buttons));
                    }
                    None => {
                        eprintln!(
                            "[inject-pad] bad spec {spec:?}; expected lx,ly,rx,ry[,buttons] in -32767..32767"
                        );
                        std::process::exit(2);
                    }
                }
            }
            #[cfg(feature = "gui")]
            "--trace-keys" => vkeyboard::set_trace(true),
            #[cfg(feature = "gui")]
            "--demo-kb-ar" => {
                demo_kb = true;
                demo_kb_ar = true;
            }
            "--import-amgp" => {
                i += 1;
                if let Some(p) = args.get(i) {
                    let amgp_path = std::path::Path::new(p);
                    match amgp::load_amgp_file(amgp_path) {
                        Ok(cfg) => {
                            if let Err(e) = cfg.save(&config_path) {
                                eprintln!("[main] failed to save imported config to {}: {e}", config_path.display());
                            } else {
                                println!("[main] successfully imported {} and saved to {}", amgp_path.display(), config_path.display());
                            }
                        }
                        Err(e) => eprintln!("[main] failed to parse {}: {e}", amgp_path.display()),
                    }
                }
                return;
            }
            "--convert" | "--convert-amgp" => {
                i += 1;
                if let Some(input_str) = args.get(i) {
                    let in_path = std::path::Path::new(input_str);
                    let out_path = if let Some(out_str) = args.get(i + 1).filter(|s| !s.starts_with("--")) {
                        std::path::PathBuf::from(out_str)
                    } else {
                        // Default output: same name with .json in profiles folder
                        let stem = in_path.file_stem().unwrap_or_default();
                        crate::config::profiles_dir().join(format!("{}.json", stem.to_string_lossy()))
                    };
                    match amgp::convert_amgp_file(in_path, &out_path) {
                        Ok(()) => println!("[converter] Successfully converted {} -> {}", in_path.display(), out_path.display()),
                        Err(e) => eprintln!("[converter] Conversion error: {e}"),
                    }
                } else {
                    eprintln!("[converter] Usage: controller-companion.exe --convert <input.amgp> [output.json/ccp]");
                }
                return;
            }
            "--config" => {
                i += 1;
                if let Some(p) = args.get(i) {
                    config_path = std::path::PathBuf::from(p);
                } else {
                    eprintln!("--config requires a file path");
                    std::process::exit(2);
                }
            }
            "--settings" => open_standalone_settings = true,
            "--install" => install_now = true,
            "--uninstall" => uninstall_now = true,
            "--quiet" => quiet = true,
            "--no-elevate" => no_elevate = true,
            // Set by the Windows startup entries, so a second copy started at
            // sign-in stays quiet instead of opening Settings.
            "--autostart" => from_autostart = true,
            "--elevate" => {
                if !platform::is_elevated() {
                    println!("[main] Requesting administrator elevation via UAC...");
                    let remaining: Vec<String> = args.iter().filter(|a| *a != "--elevate").cloned().collect();
                    if let Err(e) = platform::relaunch_elevated(&remaining) {
                        eprintln!("[main] Elevation request failed: {e}");
                    }
                    return;
                }
            }
            "--install-task" => {
                if let Ok(exe) = std::env::current_exe() {
                    match platform::install_scheduled_task(&exe.to_string_lossy()) {
                        Ok(_) => println!("[main] Successfully installed elevated Scheduled Task for logon!"),
                        Err(e) => eprintln!("[main] Failed to install scheduled task: {e}"),
                    }
                }
                return;
            }
            other => eprintln!("[main] ignoring unknown arg: {other}"),
        }
        i += 1;
    }

    if uninstall_now {
        install::uninstall_command(quiet);
        return;
    }
    if install_now {
        install::install_command();
        return;
    }

    // Automatic Administrator elevation: if not elevated, relaunch elevated via UAC immediately!
    if validate_config {
        match Config::try_load(&config_path) {
            Ok(_) => println!("Configuration is valid: {}", config_path.display()),
            Err(error) => { eprintln!("{error}"); std::process::exit(2); }
        }
        return;
    }
    if doctor {
        let report = doctor::run(&config_path);
        print!("{}", report.text);
        match doctor::save(&config_path, &report.text) {
            Ok(path) => println!("\nReport saved to {}", path.display()),
            Err(error) => eprintln!("\nCould not save the report: {error}"),
        }
        std::process::exit(if report.problems == 0 { 0 } else { 1 });
    }
    let is_cli_helper = status_only || print_default || write_default;
    // A downloaded copy started by hand offers to install itself (once; the
    // answer "no" is remembered). Asked before elevation, so the installed
    // copy belongs to the user, not to an administrator prompt.
    #[cfg(feature = "gui")]
    let demo_or_test = demo_kb || inject_pad.is_some();
    #[cfg(not(feature = "gui"))]
    let demo_or_test = inject_pad.is_some();
    if !is_cli_helper && !open_standalone_settings && !from_autostart && !demo_or_test && !elevated && install::offer_on_launch() {
        return;
    }
    if !elevated && !no_elevate && !is_cli_helper {
        println!("[main] Elevating process to run as Administrator...");
        if let Ok(()) = platform::relaunch_elevated(&args) {
            return;
        }
    }

    // Ensure Windows built-in XAML/UWP gamepad navigation hijacking is disabled
    // so mouse control is always 100% active in Windows Settings, Start Menu, etc.
    if !is_cli_helper { platform::disable_windows_gamepad_navigation(); }

    if open_standalone_settings {
        #[cfg(feature = "gui")]
        {
            let cfg = Config::load(&config_path);
            let xinput = win32::XInputApi::load().unwrap_or_else(|e| {
                eprintln!("[main] XInput warning: {e}");
                std::process::exit(1);
            });
            let eng = Engine::new(xinput, cfg);
            let eng = Arc::new(Mutex::new(eng));
            gui::run_settings_standalone(eng, config_path);
            return;
        }
    }

    if print_default {
        let cfg = Config::default();
        match serde_json::to_string_pretty(&cfg) {
            Ok(text) => println!("{text}"),
            Err(error) => eprintln!("{error}"),
        }
        return;
    }

    if write_default {
        let cfg = Config::default();
        match cfg.save(&config_path) {
            Ok(_) => println!("[main] wrote default config to {}", config_path.display()),
            Err(e) => eprintln!("[main] failed to write config: {e}"),
        }
        return;
    }

    if status_only {
        let config = Config::load(&config_path);
        let xinput = match win32::XInputApi::load() {
            Ok(api) => api,
            Err(error) => {
                eprintln!("[status] XInput unavailable: {error}");
                std::process::exit(1);
            }
        };
        println!("configured controller slot: {}", config.controller.controller_index);
        let mut found = false;
        for idx in 0..4 {
            if xinput.get_state(idx).is_some() {
                found = true;
                println!("controller {idx}: CONNECTED");
                if let Some(battery) = xinput.battery(idx) {
                    println!("battery: type=0x{:02X} level=0x{:02X}", battery.BatteryType, battery.BatteryLevel);
                }
            }
        }
        if !found {
            println!("no XInput controller connected (slots 0-3)");
        }
        return;
    }

    // Single-instance enforcement: prevent duplicate background instances
    let (mut instance_mutex, mut already_running) = win32::create_named_mutex("Local\\ControllerCompanionRust_InstanceLock");
    if already_running {
        for _ in 0..6 {
            std::thread::sleep(std::time::Duration::from_millis(100));
            let (m2, a2) = win32::create_named_mutex("Local\\ControllerCompanionRust_InstanceLock");
            if !a2 {
                instance_mutex = m2;
                already_running = false;
                break;
            }
        }
    }
    let _instance_mutex = instance_mutex;
    if already_running {
        eprintln!("[main] Another instance of Controller Companion is already running. Exiting.");
        // Starting the app again by hand is a request to see it: open Settings
        // for the running copy rather than exiting silently.
        #[cfg(feature = "gui")]
        if !from_autostart {
            gui::open_settings_process(&config_path);
        }
        let _ = from_autostart;
        return;
    }
    // A previous run that crashed or was killed may have left an injected
    // modifier or mouse button down; clear it before taking control.
    platform::release_stuck_input();

    // Never refuse to start over a damaged settings file: fall back to the
    // last good copy (or defaults) and tell the user once the OSD is up.
    let (config, load_outcome) = config::load_resilient(&config_path);
    match &load_outcome {
        config::LoadOutcome::RestoredBackup { broken } | config::LoadOutcome::ResetToDefaults { broken } => {
            platform::log_debug(&format!(
                "[main] settings file was damaged; moved to {} ({:?})",
                broken.display(),
                load_outcome
            ));
        }
        _ => {}
    }
    println!("[main] config: {}", config_path.display());
    println!(
        "[main] mouse speed={} accel={} wheel={} | deadzones L={} R={} | enabled={}",
        config.mouse.speed,
        config.mouse.acceleration,
        config.mouse.wheel_speed,
        config.controller.left_stick_dead_zone,
        config.controller.right_stick_dead_zone,
        config.enabled
    );

    // Match the Windows startup entries to the setting. `schtasks` can take a
    // moment, so this runs in the background instead of delaying input.
    {
        let autostart = config.autostart;
        std::thread::spawn(move || {
            let Ok(exe) = std::env::current_exe() else { return };
            if elevated {
                if autostart {
                    let _ = platform::install_scheduled_task(&exe.to_string_lossy());
                } else if let Err(error) = platform::remove_scheduled_task() {
                    platform::log_debug(&format!("[main] could not remove logon task: {error}"));
                }
            }
            let command = format!("\"{}\" --autostart", exe.display());
            if let Err(e) = platform::set_autostart(autostart, &command) {
                platform::log_debug(&format!("[main] autostart apply failed: {e}"));
            }
        });
    }

    let xinput = match win32::XInputApi::load() {
        Ok(x) => {
            println!("[main] XInput loaded: {}", x.dll_name);
            x
        }
        Err(e) => {
            eprintln!("[main] FATAL: {e}");
            std::process::exit(1);
        }
    };

    // Shared, reloadable running flag between the control thread and the engine.
    let running = Arc::new(AtomicBool::new(true));

    // Spawn minimalist native OSD / HUD overlay
    osd::spawn();
    osd::set_osd_enabled(config.osd_enabled);
    sound::set_sfx_enabled(config.keyboard.mechanical_sfx);
    match &load_outcome {
        config::LoadOutcome::RestoredBackup { .. } => {
            std::thread::spawn(|| {
                std::thread::sleep(Duration::from_millis(800));
                osd::show_info("🛟", "Settings restored", "The settings file was damaged; the last good copy is in use");
            });
        }
        config::LoadOutcome::ResetToDefaults { .. } => {
            std::thread::spawn(|| {
                std::thread::sleep(Duration::from_millis(800));
                osd::show_info("🛟", "Settings reset", "The settings file was damaged; defaults are in use");
            });
        }
        _ => {}
    }

    #[cfg(feature = "gui")]
    predict::init_learning(predict::memory_path_for(&config_path));
    let mut engine = Engine::new(xinput, config);
    engine.set_trace_pad(trace_pad);
    if let Some((lx, ly, rx, ry, buttons)) = inject_pad {
        engine.set_enabled(true);
        engine.set_injected_gamepad(Some(win32::XINPUT_GAMEPAD {
            wButtons: buttons,
            bLeftTrigger: 0,
            bRightTrigger: 0,
            sThumbLX: lx,
            sThumbLY: ly,
            sThumbRX: rx,
            sThumbRY: ry,
        }));
        eprintln!(
            "[inject-pad] L=({lx},{ly}) R=({rx},{ry}) buttons=0x{buttons:04X} — no controller needed"
        );
    }
    let engine = Arc::new(Mutex::new(engine));

    // --- On-screen virtual keyboard overlay (gui feature) ---
    #[cfg(feature = "gui")]
    let vk_flag: Option<Arc<AtomicBool>> = {
        let vk_nav = Arc::new(Mutex::new(crate::vknav::VkNav::default()));
        if let Ok(mut e) = engine.lock() {
            e.set_vk_nav(vk_nav.clone());
        }
        // `--demo-kb` forces a deterministic English keyboard for screenshots.
        let mut prefs = engine.lock().map(|e| e.config().keyboard.clone()).unwrap_or_default();
        if demo_kb {
            prefs.start_arabic = demo_kb_ar;
        }
        Some(vkeyboard::spawn(vk_nav, prefs, demo_kb, demo_kb))
    };
    #[cfg(not(feature = "gui"))]
    let vk_flag: Option<Arc<AtomicBool>> = None;
    #[cfg(feature = "gui")]
    if let Some(f) = vk_flag.clone() {
        if let Ok(mut e) = engine.lock() {
            e.set_vk_handle(f.clone());
        }
        if !demo_kb {
            // The overlay is opened when the user clicks into a text box (a
            // low-level mouse hook reports clicks landing on Edit/RichEdit/IBEAM
            // classes) and is dismissed when user presses B or L3.
            platform::mouse_watch_start();
            let f2 = f.clone();
            let run2 = running.clone();
            let eng2 = engine.clone();
            std::thread::spawn(move || {
                // Text-box detection uses accessibility (COM) queries here,
                // away from the input thread.
                platform::com_init_thread();
                let mut opened_at: Option<Instant> = None;
                while run2.load(Ordering::Relaxed) {
                    let enabled = eng2.lock().map(|e| e.config().vk_auto_show).unwrap_or(false);
                    if enabled {
                        let is_vis = f2.load(Ordering::Relaxed);
                        if is_vis {
                            // If user clicked outside with their physical mouse, dismiss the on-screen keyboard.
                            // - Immunity guard: Must have been opened for at least 800ms so the opening click cannot self-dismiss.
                            // - Outside guard: Cursor must not be over the virtual keyboard overlay itself.
                            // - Physical mouse guard: Handled inside is_physical_mouse_button_down() which ignores controller clicks.
                            let immunity_passed = opened_at.map(|t| t.elapsed() >= Duration::from_millis(800)).unwrap_or(false);
                            let outside_overlay = !platform::cursor_is_inside_overlay();
                            if immunity_passed && outside_overlay && platform::is_physical_mouse_button_down() {
                                f2.store(false, Ordering::SeqCst);
                                platform::set_window_visible("CCVKbd", false);
                                vkeyboard::request_vkeyboard_repaint();
                                opened_at = None;
                            }
                        } else {
                            opened_at = None;
                        }

                        let immediate = platform::take_text_click();
                        let is_ctrl = platform::last_click_was_controller();
                        let recent_click = platform::last_click_within_ms(350);
                        let is_text = is_ctrl && recent_click && (platform::is_cursor_ibeam() || platform::caret_active());
                        if is_ctrl && (immediate || is_text) {
                            let was_vis = f2.load(Ordering::SeqCst);
                            if !was_vis {
                                crate::platform::log_debug("[main] Text click detected -> surfacing virtual keyboard");
                                vkeyboard::show_keyboard(&f2);
                                opened_at = Some(Instant::now());
                            }
                        }
                    }
                    std::thread::sleep(Duration::from_millis(50));
                }
            });
        }
    }

    // --- System-wide hotkey: toggle input remapping on/off ---
    // Re-registered by the config watcher whenever Settings changes it.
    {
        let hotkey = engine.lock().map(|e| e.config().toggle_hotkey.clone()).unwrap_or_default();
        platform::set_hotkey(crate::config::parse_hotkey(&hotkey));
        let hk_engine = engine.clone();
        let hk_running = running.clone();
        std::thread::spawn(move || {
            while hk_running.load(Ordering::Relaxed) {
                if platform::take_hotkey() {
                    let now = hk_engine.lock().map(|mut e| e.toggle_enabled()).unwrap_or(false);
                    println!("[main] hotkey -> input {}", if now { "ON" } else { "OFF" });
                }
                std::thread::sleep(Duration::from_millis(40));
            }
        });
    }

    // --- Engine thread: fixed-rate poll loop (~125 Hz) ---
    let eng = engine.clone();
    let run = running.clone();
    let heartbeat = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let beat = heartbeat.clone();
    let engine_thread = std::thread::spawn(move || {
        platform::attach_thread_to_desktop();
        let target = Duration::from_micros(8_000); // 125 Hz
        let mut last = Instant::now();
        while run.load(Ordering::Relaxed) {
            beat.store((steady_seconds() * 1000.0) as u64, Ordering::Relaxed);
            let start = Instant::now();
            let elapsed = start.duration_since(last).as_secs_f64();
            last = start;
            // Monotonic seconds for repeaters/mouse.
            let steady = steady_seconds();
            let is_connected = if let Ok(mut e) = eng.lock() {
                e.update(steady, elapsed.min(0.1));
                e.connected_cached()
            } else {
                false
            };
            let spent = start.elapsed();
            // Drop polling frequency from 125 Hz to 25 Hz when controller is disconnected to save CPU/battery
            let sleep_target = if is_connected { target } else { Duration::from_millis(40) };
            if spent < sleep_target {
                std::thread::sleep(sleep_target - spent);
            }
        }
        if let Ok(mut e) = eng.lock() {
            e.shutdown();
        }
    });

    // Watchdog: if the input loop stops ticking (stuck in a system call or
    // waiting on a lock), let go of everything it might be holding so the user
    // keeps a working mouse and keyboard, and record it for diagnosis.
    {
        let watch_running = running.clone();
        std::thread::Builder::new()
            .name("cc-input-watchdog".into())
            .spawn(move || {
                let mut stalled = false;
                while watch_running.load(Ordering::Relaxed) {
                    std::thread::sleep(Duration::from_millis(500));
                    let last = heartbeat.load(Ordering::Relaxed);
                    let now = (steady_seconds() * 1000.0) as u64;
                    let silent_ms = now.saturating_sub(last);
                    if last != 0 && silent_ms > 2000 {
                        if !stalled {
                            stalled = true;
                            platform::log_debug(&format!("[watchdog] input loop silent for {silent_ms} ms; releasing held input"));
                            platform::release_stuck_input();
                        }
                    } else if stalled {
                        stalled = false;
                        platform::log_debug("[watchdog] input loop recovered");
                    }
                }
            })
            .ok();
    }

    // Publish the engine's real state for the separate settings process.
    let status_engine = engine.clone();
    let status_running = running.clone();
    let status_path = runtime_status::path_for_config(&config_path);
    let status_thread = std::thread::spawn(move || {
        let mut publish_failed = false;
        let mut last: Option<(runtime_status::RuntimeStatus, Instant)> = None;
        while status_running.load(Ordering::Relaxed) {
            let current = status_engine.lock().ok().map(|engine| engine.runtime_status());
            if let Some(status) = current {
                // Write when something changed, plus a heartbeat so readers can
                // tell a running engine from a stale file.
                if runtime_status::should_publish(last.as_ref(), &status) {
                    match runtime_status::publish(&status_path, &status) {
                        Ok(()) => publish_failed = false,
                        Err(error) if !publish_failed => {
                            eprintln!("[status] publish failed: {error}");
                            publish_failed = true;
                        }
                        Err(_) => {}
                    }
                    last = Some((status.clone(), Instant::now()));
                }
                tray::refresh_status(&status);
            }
            std::thread::sleep(Duration::from_millis(250));
        }
        let _ = std::fs::remove_file(status_path);
    });

    // Auto-switch the active profile when a program listed in a profile's
    // process rules takes focus. Polled rather than event-driven: it is a
    // foreground-window check, and a slow poll costs nothing at this scale.
    {
        let sw_engine = engine.clone();
        let sw_running = running.clone();
        let _sw_path = config_path.clone();
        std::thread::spawn(move || {
            let dir = crate::config::profiles_dir();
            // Our own windows (keyboard, settings) must not count as switching apps.
            let own_exe = std::env::current_exe()
                .ok()
                .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_lowercase()));
            let mut cached: Option<(String, Option<crate::profiles::ProfileEntry>, Instant)> = None;
            while sw_running.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_millis(900));
                let enabled = sw_engine
                    .lock()
                    .map(|e| e.config().auto_profile_switch)
                    .unwrap_or(false);
                if !enabled {
                    if let Ok(mut e) = sw_engine.lock() {
                        e.clear_auto_profile();
                    }
                    continue;
                }
                let Some(exe) = crate::platform::foreground_process_name() else {
                    continue;
                };
                if own_exe.as_deref() == Some(exe.as_str()) {
                    continue;
                }
                // Re-read the profile folder when focus moves to another
                // program, and every few seconds in case profiles were edited.
                let stale = cached
                    .as_ref()
                    .is_none_or(|(e, _, at)| *e != exe || at.elapsed() > Duration::from_secs(10));
                if stale {
                    cached = Some((exe.clone(), crate::profiles::match_process(&dir, &exe), Instant::now()));
                }
                let Some((_, hit, _)) = &cached else { continue };
                let Ok(mut e) = sw_engine.lock() else { continue };
                match hit {
                    Some(h) => {
                        let stem = h.path.file_stem().unwrap_or_default().to_string_lossy().to_string();
                        if e.auto_profile() == Some(stem.as_str()) {
                            continue;
                        }
                        match crate::profiles::load(&h.path) {
                            Ok(p) => {
                                println!("[main] auto-switch -> '{stem}' for {exe}");
                                e.set_auto_profile(&stem, p.bindings);
                                crate::osd::show_profile(&stem);
                            }
                            Err(err) => eprintln!("[main] auto-switch to '{stem}' failed: {err}"),
                        }
                    }
                    None if e.auto_profile().is_some() => {
                        e.clear_auto_profile();
                        let name = if e.config().active_profile.is_empty() {
                            crate::profiles::DEFAULT_NAME.to_string()
                        } else {
                            e.config().active_profile.clone()
                        };
                        println!("[main] auto-switch back to '{name}'");
                        crate::osd::show_profile(&name);
                    }
                    None => {}
                }
            }
        });
    }

    // Auto-reload config whenever config.json is modified (e.g. by Settings UI or profile switcher)
    let watch_engine = engine.clone();
    let watch_path = config_path.clone();
    let watch_running = running.clone();
    std::thread::spawn(move || {
        let mut last_mod = std::fs::metadata(&watch_path).and_then(|m| m.modified()).ok();
        while watch_running.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(400));
            if let Ok(m) = std::fs::metadata(&watch_path).and_then(|m| m.modified()) {
                if Some(m) != last_mod {
                    if let Ok(mut e) = watch_engine.lock() {
                        match e.reload_from_disk(&watch_path) {
                            Ok(()) => {
                                last_mod = Some(m);
                                platform::set_hotkey(crate::config::parse_hotkey(&e.config().toggle_hotkey));
                                osd::set_osd_enabled(e.config().osd_enabled);
                                sound::set_sfx_enabled(e.config().keyboard.mechanical_sfx);
                                println!("[main] reloaded updated config from {}", watch_path.display());
                            }
                            Err(error) => eprintln!("[main] config reload deferred: {error}"),
                        }
                    }
                }
            }
        }
    });

    // First run: open the tutorial so a new user learns the controller in a
    // minute instead of guessing.
    #[cfg(feature = "gui")]
    if load_outcome == config::LoadOutcome::Fresh && !demo_kb {
        gui::open_settings_on(&config_path, Some("tutorial"));
    }

    // Reclaim cold startup memory pages so background memory footprint drops immediately
    crate::platform::trim_working_set();

    println!("[main] engine running. Connect an Xbox controller (slots 0-3).");
    println!("[main] tray icon active; right-click for menu. Press Enter (or Quit) to exit.");

    // Optional: Enter in this console quits when explicitly requested via --stdin-exit.
    if stdin_exit {
        let run_stdin = running.clone();
        std::thread::spawn(move || {
            let mut line = String::new();
            if let Ok(n) = std::io::stdin().read_line(&mut line) {
                if n > 0 {
                    run_stdin.store(false, Ordering::Relaxed);
                }
            }
        });
    }

    // Tray on its own thread with its own message loop.
    let tray_engine = engine.clone();
    let tray_running = running.clone();
    let tray_config_path = config_path.clone();
    let tray_vk = vk_flag.clone();
    let settings_path = config_path.clone();
    let open_engine = engine.clone();
    let open_settings = Box::new(move || {
        #[cfg(feature = "gui")]
        {
            gui::open_settings(open_engine.clone(), settings_path.clone());
        }
        #[cfg(not(feature = "gui"))]
        {
            let _ = &open_engine; // unused in headless mode
            let _ = std::process::Command::new("notepad.exe")
                .arg(settings_path.clone())
                .spawn();
        }
    });
    let tray_thread = std::thread::spawn(move || {
        tray::run(tray_engine, tray_running, tray_config_path, open_settings, tray_vk);
    });

    // Safety net: guarantee the process actually closes after a quit request,
    // even if a thread is slow to join (user asked for tray Quit to end it).
    let run_watch = running.clone();
    std::thread::spawn(move || {
        while run_watch.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(100));
        }
        platform::log_debug("[main] run_watch detected running=false, exiting");
        // Give threads a moment to release keys / exit their loops.
        std::thread::sleep(Duration::from_millis(1500));
        std::process::exit(0);
    });

    // Wait for the running flag to drop (Quit via tray or Enter).
    while running.load(Ordering::Relaxed) {
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
    platform::log_debug("[main] running flag dropped, joining threads");
    predict::flush();
    let _ = tray_thread.join();
    let _ = engine_thread.join();
    let _ = status_thread.join();

    // Write back only what changed at runtime (on/off, a profile chosen from
    // the controller, the keyboard position). Everything else stays exactly as
    // saved, so an auto-switched game profile or a scanned controller slot can
    // never overwrite the user's settings.
    if let Ok(e) = engine.lock() {
        #[cfg(feature = "gui")]
        let keyboard_position = vkeyboard::last_position();
        #[cfg(not(feature = "gui"))]
        let keyboard_position = None;
        let changes = config::RuntimeChanges {
            enabled: e.is_enabled(),
            profile: e.changed_base_profile(),
            keyboard_position,
        };
        let saved = if config_path.exists() {
            Config::try_load(&config_path).map(|disk| changes.apply_to(disk))
        } else {
            Ok(changes.apply_to(e.config().clone()))
        };
        match saved {
            Ok(cfg) => {
                if let Err(error) = cfg.save(&config_path) {
                    platform::log_debug(&format!("[main] saving settings on exit failed: {error}"));
                }
            }
            Err(error) => platform::log_debug(&format!("[main] settings left untouched on exit: {error}")),
        }
    }
    platform::log_debug("[main] bye");
    println!("[main] bye");
    std::process::exit(0);
}

/// A monotonically increasing seconds counter for repeater/mouse timing.
fn steady_seconds() -> f64 {
    use std::sync::OnceLock;
    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_secs_f64()
}

/// Parse `--inject-pad lx,ly,rx,ry[,buttons]` into a synthetic gamepad state.
///
/// Axes are XInput units (0..32767, centred), buttons a bitmask. This is what
/// makes the input pipeline verifiable without a controller: the unit tests use
/// the same shape, and the flag exposes it at runtime.
fn parse_inject_spec(spec: &str) -> Option<(i16, i16, i16, i16, u16)> {
    let parts: Vec<&str> = spec.split(',').map(|s| s.trim()).collect();
    if parts.len() < 4 {
        return None;
    }
    let axis = |s: &str| -> Option<i16> {
        s.parse::<i32>().ok().map(|v| v.clamp(-32767, 32767) as i16)
    };
    let lx = axis(parts[0])?;
    let ly = axis(parts[1])?;
    let rx = axis(parts[2])?;
    let ry = axis(parts[3])?;
    let buttons = parts.get(4).and_then(|s| s.parse::<u32>().ok()).unwrap_or(0) as u16;
    Some((lx, ly, rx, ry, buttons))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inject_spec_parses_four_axes() {
        assert_eq!(parse_inject_spec("30000,0,0,0"), Some((30000, 0, 0, 0, 0)));
    }

    #[test]
    fn inject_spec_accepts_optional_buttons() {
        assert_eq!(parse_inject_spec("0,-20000,0,0,16"), Some((0, -20000, 0, 0, 16)));
    }

    #[test]
    fn inject_spec_rejects_garbage() {
        assert_eq!(parse_inject_spec("1,2"), None);
        assert_eq!(parse_inject_spec("a,b,c,d"), None);
    }

    #[test]
    fn inject_spec_clamps_out_of_range_axes() {
        let (lx, _, _, _, _) = parse_inject_spec("999999,0,0,0").expect("parsed");
        assert_eq!(lx, 32767, "must clamp to the XInput rail");
    }
}
