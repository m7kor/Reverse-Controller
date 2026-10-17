//! Per-user installation, built into the program itself.
//!
//! Running the downloaded `.exe` offers to install it: the app is copied to
//! `%LOCALAPPDATA%\Programs\Reverse Controller`, gets a Start-menu shortcut
//! and an entry in Windows "Installed apps", all without administrator
//! rights. `--uninstall` (what that entry runs) removes it again; settings in
//! `%APPDATA%\ControllerCompanionRust` are kept. Choosing not to install is
//! remembered, so a portable copy is never nagged.

use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

pub const APP_NAME: &str = "Reverse Controller";
const EXE_NAME: &str = "ReverseController.exe";
const UNINSTALL_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\ReverseController";
const PROJECT_URL: &str = "https://github.com/m7kor/Reverse-Controller";
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

type HKEY = *mut std::ffi::c_void;
const HKEY_CURRENT_USER: HKEY = 0x8000_0001u32 as usize as HKEY;
const KEY_ALL_ACCESS: u32 = 0xF003F;
const REG_SZ: u32 = 1;
const REG_DWORD: u32 = 4;
const RRF_RT_REG_SZ: u32 = 0x2;

#[link(name = "advapi32")]
extern "system" {
    fn RegCreateKeyExW(key: HKEY, sub: *const u16, r: u32, class: *mut u16, opt: u32, sam: u32, sec: *mut std::ffi::c_void, out: *mut HKEY, disp: *mut u32) -> i32;
    fn RegSetValueExW(key: HKEY, name: *const u16, r: u32, kind: u32, data: *const u8, len: u32) -> i32;
    fn RegCloseKey(key: HKEY) -> i32;
    fn RegDeleteTreeW(key: HKEY, sub: *const u16) -> i32;
    fn RegGetValueW(key: HKEY, sub: *const u16, value: *const u16, flags: u32, kind: *mut u32, data: *mut std::ffi::c_void, len: *mut u32) -> i32;
}

#[link(name = "user32")]
extern "system" {
    fn MessageBoxW(hwnd: *mut std::ffi::c_void, text: *const u16, caption: *const u16, kind: u32) -> i32;
}

const MB_YESNO: u32 = 0x4;
const MB_OK: u32 = 0x0;
const MB_ICONQUESTION: u32 = 0x20;
const MB_ICONINFORMATION: u32 = 0x40;
const MB_ICONWARNING: u32 = 0x30;
const MB_SETFOREGROUND: u32 = 0x1_0000;
const MB_RIGHT: u32 = 0x8_0000;
const MB_RTLREADING: u32 = 0x10_0000;
const IDYES: i32 = 6;

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

/// Show the user's language: the settings choice, else the Windows UI language.
fn arabic_ui() -> bool {
    let setting = crate::config::Config::load(&crate::config::default_config_path()).gui_language;
    match setting.to_ascii_lowercase().as_str() {
        "ar" => true,
        "en" => false,
        _ => crate::win32::user_ui_primary_language() == 0x01,
    }
}

fn message(en: &str, ar: &str, kind: u32) -> i32 {
    let rtl = arabic_ui();
    let (text, extra) = if rtl { (ar, MB_RIGHT | MB_RTLREADING) } else { (en, 0) };
    unsafe { MessageBoxW(std::ptr::null_mut(), wide(text).as_ptr(), wide(APP_NAME).as_ptr(), kind | extra | MB_SETFOREGROUND) }
}

/// `%LOCALAPPDATA%\Programs\Reverse Controller`.
pub fn install_dir() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA").map(|d| PathBuf::from(d).join("Programs").join(APP_NAME))
}

pub fn installed_exe() -> Option<PathBuf> {
    install_dir().map(|d| d.join(EXE_NAME))
}

fn start_menu_shortcut() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(|d| {
        PathBuf::from(d)
            .join(r"Microsoft\Windows\Start Menu\Programs")
            .join(format!("{APP_NAME}.lnk"))
    })
}

/// Marker meaning "the user chose to run without installing".
fn portable_marker() -> PathBuf {
    crate::config::default_config_path().with_file_name("portable.flag")
}

fn same_file(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(x), Ok(y)) => x == y,
        _ => a.as_os_str().eq_ignore_ascii_case(b.as_os_str()),
    }
}

/// Is this process the installed copy?
pub fn running_installed() -> bool {
    match (std::env::current_exe(), installed_exe()) {
        (Ok(me), Some(installed)) => same_file(&me, &installed),
        _ => false,
    }
}

fn installed_version() -> Option<String> {
    let mut buf = [0u16; 64];
    let mut len = (buf.len() * 2) as u32;
    let rc = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            wide(UNINSTALL_KEY).as_ptr(),
            wide("DisplayVersion").as_ptr(),
            RRF_RT_REG_SZ,
            std::ptr::null_mut(),
            buf.as_mut_ptr() as *mut _,
            &mut len,
        )
    };
    if rc != 0 {
        return None;
    }
    let n = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    Some(String::from_utf16_lossy(&buf[..n]))
}

/// Quote a value for a single-quoted PowerShell string.
fn ps_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

/// The PowerShell command that writes the Start-menu shortcut.
fn shortcut_command(lnk: &Path, exe: &Path) -> String {
    let dir = exe.parent().unwrap_or(Path::new("."));
    format!(
        "$s=(New-Object -ComObject WScript.Shell).CreateShortcut({lnk});$s.TargetPath={exe};$s.WorkingDirectory={dir};$s.IconLocation={icon};$s.Description={desc};$s.Save()",
        lnk = ps_quote(&lnk.display().to_string()),
        exe = ps_quote(&exe.display().to_string()),
        dir = ps_quote(&dir.display().to_string()),
        icon = ps_quote(&format!("{},0", exe.display())),
        desc = ps_quote("Use your game controller as a mouse and keyboard"),
    )
}

fn set_string(key: HKEY, name: &str, value: &str) {
    let data = wide(value);
    unsafe {
        RegSetValueExW(key, wide(name).as_ptr(), 0, REG_SZ, data.as_ptr() as *const u8, (data.len() * 2) as u32);
    }
}

fn set_dword(key: HKEY, name: &str, value: u32) {
    unsafe {
        RegSetValueExW(key, wide(name).as_ptr(), 0, REG_DWORD, &value as *const u32 as *const u8, 4);
    }
}

fn register_uninstall_entry(exe: &Path) -> Result<(), String> {
    let mut key: HKEY = std::ptr::null_mut();
    let rc = unsafe {
        RegCreateKeyExW(HKEY_CURRENT_USER, wide(UNINSTALL_KEY).as_ptr(), 0, std::ptr::null_mut(), 0, KEY_ALL_ACCESS, std::ptr::null_mut(), &mut key, std::ptr::null_mut())
    };
    if rc != 0 {
        return Err(format!("could not register in Installed apps (error {rc})"));
    }
    let exe_s = exe.display().to_string();
    let dir = exe.parent().map(|d| d.display().to_string()).unwrap_or_default();
    let size_kb = std::fs::metadata(exe).map(|m| (m.len() / 1024) as u32).unwrap_or(0);
    set_string(key, "DisplayName", APP_NAME);
    set_string(key, "DisplayVersion", env!("CARGO_PKG_VERSION"));
    set_string(key, "Publisher", "m7kor");
    set_string(key, "DisplayIcon", &format!("{exe_s},0"));
    set_string(key, "InstallLocation", &dir);
    set_string(key, "UninstallString", &format!("\"{exe_s}\" --uninstall"));
    set_string(key, "QuietUninstallString", &format!("\"{exe_s}\" --uninstall --quiet"));
    set_string(key, "URLInfoAbout", PROJECT_URL);
    set_string(key, "HelpLink", PROJECT_URL);
    set_dword(key, "NoModify", 1);
    set_dword(key, "NoRepair", 1);
    set_dword(key, "EstimatedSize", size_kb);
    unsafe {
        RegCloseKey(key);
    }
    Ok(())
}

/// Ask a running copy to quit through its tray "Quit" command (so it
/// releases input cleanly) and wait until it has gone.
fn stop_running_copy() {
    #[link(name = "user32")]
    extern "system" {
        fn FindWindowW(class: *const u16, title: *const u16) -> *mut std::ffi::c_void;
        fn PostMessageW(hwnd: *mut std::ffi::c_void, msg: u32, w: usize, l: isize) -> i32;
    }
    let class = wide("CCRustTrayHidden");
    let hwnd = unsafe { FindWindowW(class.as_ptr(), std::ptr::null()) };
    if hwnd.is_null() {
        return;
    }
    unsafe {
        PostMessageW(hwnd, 0x0111, 1004, 0); // WM_COMMAND, tray Quit
    }
    let deadline = Instant::now() + Duration::from_secs(8);
    while Instant::now() < deadline && !unsafe { FindWindowW(class.as_ptr(), std::ptr::null()) }.is_null() {
        std::thread::sleep(Duration::from_millis(100));
    }
    std::thread::sleep(Duration::from_millis(1500)); // the process exits shortly after its window
}

/// Copy this program into the install folder and register it.
pub fn install() -> Result<PathBuf, String> {
    let me = std::env::current_exe().map_err(|e| e.to_string())?;
    let target = installed_exe().ok_or("LOCALAPPDATA is not set")?;
    if let Some(dir) = target.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("could not create {}: {e}", dir.display()))?;
    }
    if !same_file(&me, &target) {
        stop_running_copy();
        let mut last = String::new();
        let copied = (0..20).any(|_| match std::fs::copy(&me, &target) {
            Ok(_) => true,
            Err(e) => {
                last = e.to_string();
                std::thread::sleep(Duration::from_millis(250));
                false
            }
        });
        if !copied {
            return Err(format!("could not copy the program: {last}"));
        }
    }
    if let Some(lnk) = start_menu_shortcut() {
        let _ = std::process::Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", &shortcut_command(&lnk, &target)])
            .creation_flags(CREATE_NO_WINDOW)
            .status();
    }
    register_uninstall_entry(&target)?;
    let _ = std::fs::remove_file(portable_marker());
    crate::platform::log_debug(&format!("[install] installed to {}", target.display()));
    Ok(target)
}

fn launch(exe: &Path) {
    let _ = std::process::Command::new(exe).spawn();
}

/// At start-up: offer to install (or update) when running a downloaded copy.
/// Returns true when this process should exit because the installed copy
/// takes over.
pub fn offer_on_launch() -> bool {
    if running_installed() || portable_marker().exists() {
        return false;
    }
    let installed = installed_exe().filter(|p| p.exists());
    let version = env!("CARGO_PKG_VERSION");
    match installed {
        Some(path) if installed_version().as_deref() == Some(version) => {
            // Same version already installed: just use that copy.
            launch(&path);
            true
        }
        Some(_) => {
            let answer = message(
                &format!("Update the installed Reverse Controller to version {version}?\n\nYes: update and start it.\nNo: run this copy without installing."),
                &format!("تحديث Reverse Controller المثبّت إلى الإصدار {version}؟\n\nنعم: تحديث وتشغيل.\nلا: تشغيل هذه النسخة بدون تثبيت."),
                MB_YESNO | MB_ICONQUESTION,
            );
            answer == IDYES && install_and_launch()
        }
        None => {
            let answer = message(
                "Install Reverse Controller for this user?\n\nIt is added to the Start menu and to Windows Installed apps. No administrator rights are needed.\n\nNo: run this copy without installing (you won't be asked again).",
                "تثبيت Reverse Controller لهذا المستخدم؟\n\nسيُضاف إلى قائمة ابدأ وإلى التطبيقات المثبتة في ويندوز، بدون صلاحيات مسؤول.\n\nلا: تشغيل هذه النسخة بدون تثبيت (ولن نسألك مرة أخرى).",
                MB_YESNO | MB_ICONQUESTION,
            );
            if answer == IDYES {
                install_and_launch()
            } else {
                let _ = std::fs::write(portable_marker(), b"run without installing\n");
                false
            }
        }
    }
}

fn install_and_launch() -> bool {
    match install() {
        Ok(path) => {
            launch(&path);
            true
        }
        Err(error) => {
            message(
                &format!("Installation failed: {error}\n\nThis copy will run without installing."),
                &format!("فشل التثبيت: {error}\n\nستعمل هذه النسخة بدون تثبيت."),
                MB_OK | MB_ICONWARNING,
            );
            false
        }
    }
}

/// `--install`: install without asking, then start the installed copy.
pub fn install_command() {
    match install() {
        Ok(path) => {
            println!("Installed to {}", path.display());
            launch(&path);
        }
        Err(error) => {
            eprintln!("Installation failed: {error}");
            std::process::exit(1);
        }
    }
}

/// `--uninstall [--quiet]`: what Windows "Installed apps" runs.
pub fn uninstall_command(quiet: bool) {
    if !quiet {
        let answer = message(
            "Remove Reverse Controller from this PC?\n\nYour settings stay in %APPDATA%\\ControllerCompanionRust in case you install it again.",
            "إزالة Reverse Controller من هذا الجهاز؟\n\nستبقى إعداداتك في %APPDATA%\\ControllerCompanionRust إذا ثبّته مرة أخرى.",
            MB_YESNO | MB_ICONQUESTION,
        );
        if answer != IDYES {
            return;
        }
    }
    stop_running_copy();
    let _ = crate::platform::set_autostart(false, "");
    let _ = crate::platform::remove_scheduled_task();
    if let Some(lnk) = start_menu_shortcut() {
        let _ = std::fs::remove_file(lnk);
    }
    unsafe {
        RegDeleteTreeW(HKEY_CURRENT_USER, wide(UNINSTALL_KEY).as_ptr());
    }
    // The running program cannot delete itself: remove the folder a moment
    // after this process has exited.
    if let Some(dir) = install_dir().filter(|d| d.exists()) {
        let _ = std::process::Command::new("cmd")
            .arg("/c")
            .arg(format!("ping 127.0.0.1 -n 3 >nul & rd /s /q \"{}\"", dir.display()))
            .creation_flags(CREATE_NO_WINDOW)
            .spawn();
    }
    if !quiet {
        message("Reverse Controller was removed.", "تمت إزالة Reverse Controller.", MB_OK | MB_ICONINFORMATION);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn powershell_values_survive_quotes_in_paths() {
        assert_eq!(ps_quote(r"C:\Users\O'Neil\x.lnk"), r"'C:\Users\O''Neil\x.lnk'");
        let cmd = shortcut_command(Path::new(r"C:\S\Reverse Controller.lnk"), Path::new(r"C:\P\Reverse Controller\ReverseController.exe"));
        assert!(cmd.contains(r"$s.TargetPath='C:\P\Reverse Controller\ReverseController.exe'"), "{cmd}");
        assert!(cmd.contains(r"$s.WorkingDirectory='C:\P\Reverse Controller'"), "{cmd}");
        assert!(cmd.ends_with("$s.Save()"));
    }

    #[test]
    fn install_paths_live_in_the_users_profile() {
        if let Some(exe) = installed_exe() {
            let s = exe.display().to_string();
            assert!(s.ends_with(r"Programs\Reverse Controller\ReverseController.exe"), "{s}");
        }
        assert!(portable_marker().ends_with("portable.flag"));
    }

    #[test]
    fn path_comparison_ignores_case() {
        assert!(same_file(Path::new(r"C:\Nope\A.exe"), Path::new(r"c:\nope\a.EXE")));
        assert!(!same_file(Path::new(r"C:\Nope\A.exe"), Path::new(r"C:\Nope\B.exe")));
    }
}
