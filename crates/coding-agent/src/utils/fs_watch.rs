use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::thread::{self, JoinHandle};
use std::time::{Duration, SystemTime};

pub const FS_WATCH_RETRY_DELAY_MS: u64 = 5_000;
const POLL_INTERVAL_MS: u64 = 100;

#[derive(Debug, Clone, PartialEq, Eq)]
struct FileSnapshot {
    exists: bool,
    modified: Option<SystemTime>,
    len: Option<u64>,
}

pub struct FsWatcher {
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl FsWatcher {
    pub fn close(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for FsWatcher {
    fn drop(&mut self) {
        self.close();
    }
}

pub fn close_watcher(mut watcher: Option<FsWatcher>) {
    if let Some(watcher) = watcher.as_mut() {
        watcher.close();
    }
}

pub fn watch_with_error_handler<F, E>(
    path: impl AsRef<Path>,
    listener: F,
    on_error: E,
) -> Option<FsWatcher>
where
    F: Fn() + Send + Sync + 'static,
    E: Fn() + Send + Sync + 'static,
{
    let path = path.as_ref().to_path_buf();
    if fs::metadata(&path).is_err() {
        on_error();
        return None;
    }

    let stop = Arc::new(AtomicBool::new(false));
    let thread_stop = stop.clone();
    let listener = Arc::new(listener);
    let on_error = Arc::new(on_error);
    let handle = thread::spawn(move || watch_loop(path, thread_stop, listener, on_error));

    Some(FsWatcher {
        stop,
        handle: Some(handle),
    })
}

fn watch_loop(
    path: PathBuf,
    stop: Arc<AtomicBool>,
    listener: Arc<dyn Fn() + Send + Sync>,
    on_error: Arc<dyn Fn() + Send + Sync>,
) {
    let mut previous = snapshot(&path);
    while !stop.load(Ordering::SeqCst) {
        thread::sleep(Duration::from_millis(POLL_INTERVAL_MS));
        let current = snapshot(&path);
        if current.is_none() {
            on_error();
            break;
        }

        let current = current.expect("checked above");
        if previous.as_ref() != Some(&current) {
            listener();
            previous = Some(current);
        }
    }
}

fn snapshot(path: &Path) -> Option<FileSnapshot> {
    let metadata = fs::metadata(path).ok()?;
    Some(FileSnapshot {
        exists: true,
        modified: metadata.modified().ok(),
        len: metadata.is_file().then_some(metadata.len()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    use std::time::{Instant, UNIX_EPOCH};

    #[test]
    fn watcher_notifies_on_file_change_and_closes() {
        let path = test_file("change");
        fs::write(&path, "one").expect("write");
        let calls = Arc::new(AtomicUsize::new(0));
        let calls_for_listener = calls.clone();
        let mut watcher = watch_with_error_handler(
            &path,
            move || {
                calls_for_listener.fetch_add(1, Ordering::SeqCst);
            },
            || {},
        )
        .expect("watcher");

        thread::sleep(Duration::from_millis(POLL_INTERVAL_MS * 2));
        fs::write(&path, "two changed").expect("write changed");
        wait_until(Duration::from_secs(2), || calls.load(Ordering::SeqCst) > 0);
        watcher.close();
        let after_close = calls.load(Ordering::SeqCst);
        fs::write(&path, "three changed again").expect("write after close");
        thread::sleep(Duration::from_millis(POLL_INTERVAL_MS * 2));
        assert_eq!(calls.load(Ordering::SeqCst), after_close);
        fs::remove_file(path).ok();
    }

    #[test]
    fn missing_path_calls_error_handler() {
        let path = test_file("missing");
        fs::remove_file(&path).ok();
        let errors = Arc::new(AtomicUsize::new(0));
        let errors_for_handler = errors.clone();
        let watcher = watch_with_error_handler(
            &path,
            || {},
            move || {
                errors_for_handler.fetch_add(1, Ordering::SeqCst);
            },
        );
        assert!(watcher.is_none());
        assert_eq!(errors.load(Ordering::SeqCst), 1);
    }

    fn wait_until(timeout: Duration, condition: impl Fn() -> bool) {
        let started = Instant::now();
        while started.elapsed() < timeout {
            if condition() {
                return;
            }
            thread::sleep(Duration::from_millis(20));
        }
        assert!(condition());
    }

    fn test_file(name: &str) -> PathBuf {
        let id = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos());
        std::env::temp_dir().join(format!("pm-agent-fs-watch-{name}-{id}"))
    }
}
