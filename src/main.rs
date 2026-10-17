//! Controller Companion — native Rust port (core engine).
//!
//! Steam features are removed by request. See docs/PORT_NOTES.md for the full
//! feature inventory, what is implemented, and what is best-effort.

// The Win32 FFI layer deliberately mirrors the spelling of the Windows API
// (HWND, MSG, KEYBDINPUT, ...). Renaming them would make the bindings harder to
// match against Microsoft's documentation, so this lint is off crate-wide.
#![windows_subsystem = "windows"]
#![allow(clippy::upper_case_acronyms)]

mod actions;
pub mod amgp;
#[cfg(feature = "gui")]
pub mod arabic;
mod bindings;
mod config;
#[cfg(feature = "gui")]
mod calibration;
mod engine;
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
#[cfg(feature = "voice")]
pub mod voice;
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
         --help            Show this help\n"
    );
}

fn main() {
    std::panic::set_hook(Box::new(|info| {
        let msg = format!("[PANIC] {info}");
        platform::log_debug(&msg);
        eprintln!("{msg}");
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
            let has_cli = std::env::args().any(|a| a == "--help" || a == "-h" || a == "--status");
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
    let mut stdin_exit = false;
    #[cfg(feature = "gui")]
    let mut demo_kb = false;
    #[cfg(feature = "gui")]
    let mut demo_kb_ar = false;
    let mut open_standalone_settings = false;
    let mut trace_pad = false;
    let mut no_elevate = false;
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
            "--no-elevate" => no_elevate = true,
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

    // Automatic Administrator elevation: if not elevated, relaunch elevated via UAC immediately!
    if validate_config {
        match Config::try_load(&config_path) {
            Ok(_) => println!("Configuration is valid: {}", config_path.display()),
            Err(error) => { eprintln!("{error}"); std::process::exit(2); }
        }
        return;
    }
    let is_cli_helper = status_only || print_default || write_default;
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
        println!("{}", serde_json::to_string_pretty(&cfg).unwrap());
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
        return;
    }

    let config = if config_path.exists() {
        Config::try_load(&config_path).unwrap_or_else(|error| {
            platform::log_debug(&format!("[main] Startup stopped: {error}"));
            eprintln!("{error}");
            std::process::exit(2);
        })
    } else {
        Config::default()
    };
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
    #[cfg(feature = "voice")]
    crate::voice::set_voice_settings(config.voice.clone());

    if elevated && config.autostart {
        if let Ok(exe) = std::env::current_exe() {
            let _ = platform::install_scheduled_task(&exe.to_string_lossy());
        }
    }

    // Apply autostart registration to match the persisted setting.
    if let Ok(exe) = std::env::current_exe() {
        let quoted = format!("\"{}\"", exe.display());
        if let Err(e) = platform::set_autostart(config.autostart, &quoted) {
            eprintln!("[main] autostart apply failed: {e}");
        }
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

    // Voice model is lazily loaded on first dictation and automatically unloaded on idle
    #[cfg(feature = "voice")]
    crate::platform::log_debug("[main] Voice feature active: lazy loading on-demand with auto-unload");

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
                                f2.store(true, Ordering::SeqCst);
                                let work = platform::work_area();
                                let origin = crate::config::KeyboardPrefs::default().anchor(work, None);
                                let base_w = (crate::config::KB_BASE_W).round() as i32;
                                let base_h = (crate::config::KB_BASE_H).round() as i32;
                                platform::place_window("CCVKbd", origin.0, origin.1, base_w, base_h);
                                platform::set_window_visible("CCVKbd", true);
                                vkeyboard::request_vkeyboard_repaint();
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
    {
        let hotkey = engine.lock().map(|e| e.config().toggle_hotkey.clone()).unwrap_or_default();
        if let Some((mods, vk)) = crate::config::parse_hotkey(&hotkey) {
            println!("[main] hotkey {hotkey} toggles input");
            platform::hotkey_watch_start(mods, vk);
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
    }

    // --- Engine thread: fixed-rate poll loop (~125 Hz) ---
    let eng = engine.clone();
    let run = running.clone();
    let engine_thread = std::thread::spawn(move || {
        platform::attach_thread_to_desktop();
        let target = Duration::from_micros(8_000); // 125 Hz
        let mut last = Instant::now();
        while run.load(Ordering::Relaxed) {
            let start = Instant::now();
            let elapsed = start.duration_since(last).as_secs_f64();
            last = start;
            // Monotonic seconds for repeaters/mouse.
            let steady = steady_seconds();
            let is_connected = if let Ok(mut e) = eng.lock() {
                e.update(steady, elapsed.min(0.1));
                e.is_connected()
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

    // Publish the engine's real state for the separate settings process.
    let status_engine = engine.clone();
    let status_running = running.clone();
    let status_path = runtime_status::path_for_config(&config_path);
    let status_thread = std::thread::spawn(move || {
        let mut publish_failed = false;
        while status_running.load(Ordering::Relaxed) {
            if let Ok(engine) = status_engine.lock() {
                match runtime_status::publish(&status_path, &engine.runtime_status()) {
                    Ok(()) => publish_failed = false,
                    Err(error) if !publish_failed => {
                        eprintln!("[status] publish failed: {error}");
                        publish_failed = true;
                    }
                    Err(_) => {}
                }
            }
            std::thread::sleep(Duration::from_millis(500));
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
            let mut current = String::new();
            while sw_running.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_millis(900));
                let enabled = sw_engine
                    .lock()
                    .map(|e| e.config().auto_profile_switch)
                    .unwrap_or(false);
                if !enabled {
                    continue;
                }
                let Some(exe) = crate::platform::foreground_process_name() else {
                    continue;
                };
                if exe == current {
                    continue;
                }
                let hit = crate::profiles::match_process(&dir, &exe);
                current = exe.clone();
                if let Ok(mut e) = sw_engine.lock() {
                    let target_stem = match &hit {
                        Some(h) => h
                            .path
                            .file_stem()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .to_string(),
                        None => crate::profiles::DEFAULT_NAME.to_string(),
                    };
                    if e.config().active_profile == target_stem {
                        continue;
                    }
                    if target_stem == crate::profiles::DEFAULT_NAME {
                        println!("[main] auto-switch -> '{target_stem}' for {exe}");
                        e.set_profile(crate::profiles::DEFAULT_NAME, crate::bindings::default_profile());
                    } else if let Some(h) = hit {
                        match crate::profiles::load(&h.path) {
                            Ok(p) => {
                                println!("[main] auto-switch -> '{target_stem}' for {exe}");
                                e.set_profile(&target_stem, p.bindings);
                                crate::osd::show_profile(&target_stem);
                            }
                            Err(err) => eprintln!("[main] auto-switch to '{target_stem}' failed: {err}"),
                        }
                    }
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
            #[cfg(feature = "voice")]
            crate::voice::check_idle_unload();
            if let Ok(m) = std::fs::metadata(&watch_path).and_then(|m| m.modified()) {
                if Some(m) != last_mod {
                    if let Ok(mut e) = watch_engine.lock() {
                        match e.reload_from_disk(&watch_path) {
                            Ok(()) => {
                                last_mod = Some(m);
                                #[cfg(feature = "voice")]
                                crate::voice::set_voice_settings(e.config().voice.clone());
                                println!("[main] reloaded updated config from {}", watch_path.display());
                            }
                            Err(error) => eprintln!("[main] config reload deferred: {error}"),
                        }
                    }
                }
            }
        }
    });

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
    let _ = tray_thread.join();
    let _ = engine_thread.join();
    let _ = status_thread.join();

    // Persist current runtime config (including enabled state) to disk.
    if let Ok(e) = engine.lock() {
        let _ = e.config().save(&config_path);
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
