use std::collections::BTreeMap;
use std::panic::{self, AssertUnwindSafe};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use crate::{AiError, AiResult};

pub type SessionResourceCleanup =
    Arc<dyn Fn(Option<&str>) -> Result<(), String> + Send + Sync + 'static>;

static NEXT_CLEANUP_ID: AtomicU64 = AtomicU64::new(1);
static SESSION_RESOURCE_CLEANUPS: OnceLock<Mutex<BTreeMap<u64, SessionResourceCleanup>>> =
    OnceLock::new();

pub struct SessionResourceCleanupGuard {
    cleanup_id: u64,
}

impl Drop for SessionResourceCleanupGuard {
    fn drop(&mut self) {
        unregister_session_resource_cleanup(self.cleanup_id);
    }
}

pub fn register_session_resource_cleanup(
    cleanup: impl Fn(Option<&str>) -> Result<(), String> + Send + Sync + 'static,
) -> SessionResourceCleanupGuard {
    let cleanup_id = NEXT_CLEANUP_ID.fetch_add(1, Ordering::Relaxed);
    cleanup_registry()
        .lock()
        .expect("session resource cleanup registry poisoned")
        .insert(cleanup_id, Arc::new(cleanup));
    SessionResourceCleanupGuard { cleanup_id }
}

pub fn cleanup_session_resources(session_id: Option<&str>) -> AiResult<()> {
    let cleanups = cleanup_registry()
        .lock()
        .expect("session resource cleanup registry poisoned")
        .values()
        .cloned()
        .collect::<Vec<_>>();
    let mut errors = Vec::new();
    for cleanup in cleanups {
        match panic::catch_unwind(AssertUnwindSafe(|| cleanup(session_id))) {
            Ok(Ok(())) => {}
            Ok(Err(error)) => errors.push(error),
            Err(payload) => errors.push(panic_message(payload)),
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(AiError::SessionResourceCleanup(errors))
    }
}

fn unregister_session_resource_cleanup(cleanup_id: u64) {
    cleanup_registry()
        .lock()
        .expect("session resource cleanup registry poisoned")
        .remove(&cleanup_id);
}

fn cleanup_registry() -> &'static Mutex<BTreeMap<u64, SessionResourceCleanup>> {
    SESSION_RESOURCE_CLEANUPS.get_or_init(|| Mutex::new(BTreeMap::new()))
}

fn panic_message(payload: Box<dyn std::any::Any + Send>) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        return (*message).to_string();
    }
    if let Some(message) = payload.downcast_ref::<String>() {
        return message.clone();
    }
    "session resource cleanup panicked".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

    #[test]
    fn runs_registered_cleanups_with_session_id() {
        let _lock = test_lock();
        let calls = Arc::new(Mutex::new(Vec::new()));
        let calls_for_cleanup = Arc::clone(&calls);
        let _guard = register_session_resource_cleanup(move |session_id| {
            calls_for_cleanup
                .lock()
                .expect("calls poisoned")
                .push(session_id.unwrap_or_default().to_string());
            Ok(())
        });

        cleanup_session_resources(Some("session-1")).expect("cleanup");

        assert_eq!(
            calls.lock().expect("calls poisoned").as_slice(),
            ["session-1"]
        );
    }

    #[test]
    fn unregisters_cleanup_when_guard_is_dropped() {
        let _lock = test_lock();
        let calls = Arc::new(Mutex::new(0));
        {
            let calls_for_cleanup = Arc::clone(&calls);
            let _guard = register_session_resource_cleanup(move |_| {
                *calls_for_cleanup.lock().expect("calls poisoned") += 1;
                Ok(())
            });
        }

        cleanup_session_resources(None).expect("cleanup");

        assert_eq!(*calls.lock().expect("calls poisoned"), 0);
    }

    #[test]
    fn aggregates_cleanup_errors() {
        let _lock = test_lock();
        let _guard_a =
            register_session_resource_cleanup(|_| Err("first cleanup failed".to_string()));
        let _guard_b =
            register_session_resource_cleanup(|_| Err("second cleanup failed".to_string()));

        let error = cleanup_session_resources(None).expect_err("cleanup errors");

        match error {
            AiError::SessionResourceCleanup(errors) => {
                assert_eq!(errors.len(), 2);
                assert!(errors.iter().any(|error| error == "first cleanup failed"));
                assert!(errors.iter().any(|error| error == "second cleanup failed"));
            }
            other => panic!("unexpected error: {other}"),
        }
    }

    fn test_lock() -> MutexGuard<'static, ()> {
        static TEST_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        TEST_LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .expect("session resources test lock poisoned")
    }
}
