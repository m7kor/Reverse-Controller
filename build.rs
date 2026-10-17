//! Embeds the app icon and version information into the Windows executable.
//!
//! Uses the Windows SDK resource compiler (`rc.exe`). Without it the build
//! still succeeds, just without the icon, so nobody is blocked by a missing
//! SDK component.

use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=assets/icon.ico");
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() != Ok("msvc") {
        return;
    }
    let Some(rc) = find_rc() else {
        println!("cargo:warning=rc.exe (Windows SDK) not found; building without the app icon");
        return;
    };
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").expect("cargo sets OUT_DIR"));
    let icon = Path::new(&std::env::var("CARGO_MANIFEST_DIR").expect("cargo sets CARGO_MANIFEST_DIR"))
        .join("assets")
        .join("icon.ico");
    let version = std::env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "0.0.0".into());
    let mut parts: Vec<u32> = version.split('.').map(|p| p.parse().unwrap_or(0)).collect();
    parts.resize(4, 0);
    let commas = parts.iter().map(u32::to_string).collect::<Vec<_>>().join(",");
    let script = format!(
        r#"1 ICON "{icon}"
1 VERSIONINFO
FILEVERSION {commas}
PRODUCTVERSION {commas}
FILEOS 0x40004
FILETYPE 0x1
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904b0"
    BEGIN
      VALUE "CompanyName", "m7kor"
      VALUE "FileDescription", "Reverse Controller"
      VALUE "FileVersion", "{version}"
      VALUE "ProductName", "Reverse Controller"
      VALUE "ProductVersion", "{version}"
      VALUE "LegalCopyright", "MIT License"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x409, 1200
  END
END
"#,
        icon = icon.display().to_string().replace('\\', "\\\\"),
    );
    let rc_file = out_dir.join("app.rc");
    let res_file = out_dir.join("app.res");
    if std::fs::write(&rc_file, script).is_err() {
        println!("cargo:warning=could not write the resource script; building without the app icon");
        return;
    }
    let ok = Command::new(&rc)
        .arg("/nologo")
        .arg("/fo")
        .arg(&res_file)
        .arg(&rc_file)
        .status()
        .is_ok_and(|s| s.success());
    if ok {
        println!("cargo:rustc-link-arg-bins={}", res_file.display());
    } else {
        println!("cargo:warning=rc.exe failed; building without the app icon");
    }
}

/// The newest x64 `rc.exe` in the Windows 10/11 SDK (or `$RC`).
fn find_rc() -> Option<PathBuf> {
    if let Ok(rc) = std::env::var("RC") {
        return Some(PathBuf::from(rc));
    }
    let root = Path::new(r"C:\Program Files (x86)\Windows Kits\10\bin");
    let mut versions: Vec<PathBuf> = std::fs::read_dir(root)
        .ok()?
        .flatten()
        .map(|e| e.path().join("x64").join("rc.exe"))
        .filter(|p| p.exists())
        .collect();
    versions.sort();
    versions.pop()
}
