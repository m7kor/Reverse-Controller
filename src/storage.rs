//! Replace persistent files only after their complete contents reach disk.
use std::{io::{self, Write}, path::Path, sync::atomic::{AtomicU64, Ordering}};
use std::os::windows::ffi::OsStrExt;

pub fn write_atomic(path: &Path, data: &[u8]) -> io::Result<()> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    // Keep the full destination filename, and skip abandoned files left by a
    // crashed process whose PID has since been reused.
    let (temporary, mut file) = loop {
        let mut name = path.as_os_str().to_os_string();
        name.push(format!(".{}-{}.tmp", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        let candidate = std::path::PathBuf::from(name);
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&candidate) {
            Ok(file) => break (candidate, file),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    };
    let result = (|| {
        file.write_all(data)?;
        file.sync_all()?;
        drop(file);
        let from: Vec<u16> = temporary.as_os_str().encode_wide().chain(Some(0)).collect();
        let to: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        #[link(name = "kernel32")]
        extern "system" { fn MoveFileExW(from: *const u16, to: *const u16, flags: u32) -> i32; }
        if unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), 0x1 | 0x8) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    })();
    if result.is_err() { let _ = std::fs::remove_file(&temporary); }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn replacement_is_complete_and_failure_preserves_original() {
        let dir = std::env::temp_dir().join(format!("cc-atomic-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.json");
        write_atomic(&path, b"old").unwrap();
        write_atomic(&path, b"new complete data").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"new complete data");
        // A handle denying delete sharing makes replacement fail on Windows.
        use std::os::windows::fs::OpenOptionsExt;
        let held = std::fs::OpenOptions::new().read(true).share_mode(1).open(&path).unwrap();
        assert!(write_atomic(&path, b"must not replace").is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"new complete data");
        drop(held);
        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir(dir).unwrap();
    }
}
