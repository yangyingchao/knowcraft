//! 知识库写锁：进程内互斥加 `data_dir/.write.lock` 跨进程文件锁。
//!
//! 所有写文件、数据库与索引的操作都先取得该锁。锁顺序固定为写锁 → Store 连接锁 →
//! tantivy 写锁。`WriteLock` 不可重入：同一线程重复获取会一直等待。

use std::{
    fs::{File, OpenOptions},
    path::{Path, PathBuf},
    sync::{Mutex, MutexGuard},
    time::{Duration, Instant},
};

use rustix::fs::{flock, FlockOperation};

use crate::error::{Error, Result};

const LOCK_FILE: &str = ".write.lock";
const ACQUIRE_TIMEOUT: Duration = Duration::from_secs(5);
const POLL_INTERVAL: Duration = Duration::from_millis(50);

pub struct WriteLock {
    process: Mutex<()>,
    path: PathBuf,
}

impl WriteLock {
    pub fn new(data_dir: &Path) -> WriteLock {
        WriteLock {
            process: Mutex::new(()),
            path: data_dir.join(LOCK_FILE),
        }
    }

    pub fn acquire(&self) -> Result<WriteGuard<'_>> {
        let process = self
            .process
            .lock()
            .map_err(|_| Error::Store("write lock poisoned".into()))?;
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(&self.path)?;
        let deadline = Instant::now() + ACQUIRE_TIMEOUT;
        loop {
            match flock(&file, FlockOperation::NonBlockingLockExclusive) {
                Ok(()) => {
                    return Ok(WriteGuard {
                        _process: process,
                        _file: file,
                    });
                }
                Err(rustix::io::Errno::WOULDBLOCK) if Instant::now() < deadline => {
                    std::thread::sleep(POLL_INTERVAL);
                }
                Err(rustix::io::Errno::WOULDBLOCK) => {
                    return Err(Error::Store(
                        "another instance is writing to this knowledge base".into(),
                    ));
                }
                Err(error) => return Err(Error::Io(error.into())),
            }
        }
    }
}

pub struct WriteGuard<'a> {
    _process: MutexGuard<'a, ()>,
    _file: File,
}

#[cfg(test)]
mod tests {
    use std::{
        sync::Arc,
        thread,
        time::{Duration, Instant},
    };

    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("kc-lock-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn second_thread_waits_for_the_first_guard() {
        let lock = Arc::new(WriteLock::new(&temp_dir("wait")));
        let first = lock.acquire().unwrap();
        let started = Instant::now();

        let handle = thread::spawn({
            let lock = lock.clone();
            move || {
                let _guard = lock.acquire().unwrap();
                started.elapsed()
            }
        });

        thread::sleep(Duration::from_millis(150));
        drop(first);
        let waited = handle.join().unwrap();
        assert!(waited >= Duration::from_millis(100));
    }

    #[test]
    fn guard_releases_the_file_lock() {
        let dir = temp_dir("release");
        let lock = WriteLock::new(&dir);
        drop(lock.acquire().unwrap());
        let file = File::open(dir.join(LOCK_FILE)).unwrap();
        rustix::fs::flock(&file, FlockOperation::NonBlockingLockExclusive).unwrap();
    }
}
