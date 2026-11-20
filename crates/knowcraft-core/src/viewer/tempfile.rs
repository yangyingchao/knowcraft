//! Temporary file creation, mirroring `mktempfile`'s observable behaviour.

use std::ffi::CString;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// Create a uniquely named, empty temporary file with mode 0600.
/// Returns `None` after three failed attempts.
pub fn create() -> Option<PathBuf> {
    let dir = std::env::temp_dir();
    for _ in 0..3 {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos() as u64)
            .unwrap_or(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let name = format!("kc{}{}{}", std::process::id(), nanos, n);
        let path = dir.join(name);
        let c = match CString::new(path.to_string_lossy().as_bytes()) {
            Ok(c) => c,
            Err(_) => continue,
        };
        // SAFETY: `c` is a valid path and the mode is a valid creation mode.
        let fd = unsafe {
            libc::open(
                c.as_ptr(),
                libc::O_CREAT | libc::O_EXCL | libc::O_RDWR,
                libc::S_IRUSR | libc::S_IWUSR,
            )
        };
        if fd >= 0 {
            // SAFETY: the descriptor was just opened by us.
            unsafe { libc::close(fd) };
            return Some(path);
        }
    }
    eprintln!("Unable to create tempfile");
    None
}

pub fn path_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}
