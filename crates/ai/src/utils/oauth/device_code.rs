use std::thread;
use std::time::{Duration, Instant};

use crate::{AiError, AiResult};

pub const CANCEL_MESSAGE: &str = "Login cancelled";
pub const TIMEOUT_MESSAGE: &str = "Device flow timed out";
pub const SLOW_DOWN_TIMEOUT_MESSAGE: &str = "Device flow timed out after one or more slow_down responses. This is often caused by clock drift in WSL or VM environments. Please sync or restart the VM clock and try again.";
pub const MINIMUM_INTERVAL_MS: u64 = 1000;
pub const DEFAULT_POLL_INTERVAL_SECONDS: u64 = 5;
pub const SLOW_DOWN_INTERVAL_INCREMENT_MS: u64 = 5000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OAuthDeviceCodePollResult {
    Pending,
    SlowDown,
    Complete { access_token: String },
    Failed { message: String },
}

pub struct OAuthDeviceCodePollOptions<P>
where
    P: FnMut() -> OAuthDeviceCodePollResult,
{
    pub interval_seconds: Option<u64>,
    pub expires_in_seconds: Option<u64>,
    pub poll: P,
    pub is_cancelled: Option<Box<dyn Fn() -> bool + Send + Sync>>,
}

pub fn poll_oauth_device_code_flow<P>(options: OAuthDeviceCodePollOptions<P>) -> AiResult<String>
where
    P: FnMut() -> OAuthDeviceCodePollResult,
{
    poll_oauth_device_code_flow_with_runtime(
        options,
        || Instant::now(),
        |duration| {
            thread::sleep(duration);
            Ok(())
        },
    )
}

fn poll_oauth_device_code_flow_with_runtime<P, N, S>(
    mut options: OAuthDeviceCodePollOptions<P>,
    mut now: N,
    mut sleep: S,
) -> AiResult<String>
where
    P: FnMut() -> OAuthDeviceCodePollResult,
    N: FnMut() -> Instant,
    S: FnMut(Duration) -> AiResult<()>,
{
    let start = now();
    let deadline = options
        .expires_in_seconds
        .and_then(|seconds| start.checked_add(Duration::from_secs(seconds)));
    let mut interval = Duration::from_millis(
        MINIMUM_INTERVAL_MS.max(
            options
                .interval_seconds
                .unwrap_or(DEFAULT_POLL_INTERVAL_SECONDS)
                * 1000,
        ),
    );
    let mut slow_down_responses = 0;

    while deadline.is_none_or(|deadline| now() < deadline) {
        if options
            .is_cancelled
            .as_ref()
            .is_some_and(|is_cancelled| is_cancelled())
        {
            return Err(AiError::InvalidResponse(CANCEL_MESSAGE.to_string()));
        }

        let sleep_duration = deadline
            .map(|deadline| deadline.saturating_duration_since(now()).min(interval))
            .unwrap_or(interval);
        sleep(sleep_duration)?;

        match (options.poll)() {
            OAuthDeviceCodePollResult::Complete { access_token } => return Ok(access_token),
            OAuthDeviceCodePollResult::Pending => {}
            OAuthDeviceCodePollResult::SlowDown => {
                slow_down_responses += 1;
                interval += Duration::from_millis(SLOW_DOWN_INTERVAL_INCREMENT_MS);
                if interval < Duration::from_millis(MINIMUM_INTERVAL_MS) {
                    interval = Duration::from_millis(MINIMUM_INTERVAL_MS);
                }
            }
            OAuthDeviceCodePollResult::Failed { message } => {
                return Err(AiError::InvalidResponse(message));
            }
        }
    }

    if slow_down_responses > 0 {
        Err(AiError::InvalidResponse(
            SLOW_DOWN_TIMEOUT_MESSAGE.to_string(),
        ))
    } else {
        Err(AiError::InvalidResponse(TIMEOUT_MESSAGE.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::rc::Rc;

    #[test]
    fn returns_access_token_when_poll_completes() {
        let start = Instant::now();
        let elapsed = Rc::new(Cell::new(Duration::ZERO));
        let elapsed_for_now = Rc::clone(&elapsed);
        let elapsed_for_sleep = Rc::clone(&elapsed);
        let mut polls = 0;

        let token = poll_oauth_device_code_flow_with_runtime(
            OAuthDeviceCodePollOptions {
                interval_seconds: Some(1),
                expires_in_seconds: Some(10),
                poll: move || {
                    polls += 1;
                    if polls == 2 {
                        OAuthDeviceCodePollResult::Complete {
                            access_token: "token".to_string(),
                        }
                    } else {
                        OAuthDeviceCodePollResult::Pending
                    }
                },
                is_cancelled: None,
            },
            move || start + elapsed_for_now.get(),
            move |duration| {
                elapsed_for_sleep.set(elapsed_for_sleep.get() + duration);
                Ok(())
            },
        )
        .expect("token");

        assert_eq!(token, "token");
        assert_eq!(elapsed.get(), Duration::from_secs(2));
    }

    #[test]
    fn slow_down_increases_following_interval() {
        let start = Instant::now();
        let elapsed = Rc::new(Cell::new(Duration::ZERO));
        let elapsed_for_now = Rc::clone(&elapsed);
        let elapsed_for_sleep = Rc::clone(&elapsed);
        let sleeps = Rc::new(std::cell::RefCell::new(Vec::new()));
        let sleeps_for_sleep = Rc::clone(&sleeps);
        let mut polls = 0;

        let token = poll_oauth_device_code_flow_with_runtime(
            OAuthDeviceCodePollOptions {
                interval_seconds: Some(1),
                expires_in_seconds: Some(20),
                poll: move || {
                    polls += 1;
                    match polls {
                        1 => OAuthDeviceCodePollResult::SlowDown,
                        2 => OAuthDeviceCodePollResult::Complete {
                            access_token: "token".to_string(),
                        },
                        _ => OAuthDeviceCodePollResult::Pending,
                    }
                },
                is_cancelled: None,
            },
            move || start + elapsed_for_now.get(),
            move |duration| {
                sleeps_for_sleep.borrow_mut().push(duration);
                elapsed_for_sleep.set(elapsed_for_sleep.get() + duration);
                Ok(())
            },
        )
        .expect("token");

        assert_eq!(token, "token");
        assert_eq!(
            sleeps.borrow().as_slice(),
            [Duration::from_secs(1), Duration::from_secs(6)]
        );
    }

    #[test]
    fn reports_failed_poll_message() {
        let error = poll_oauth_device_code_flow_with_runtime(
            OAuthDeviceCodePollOptions {
                interval_seconds: Some(1),
                expires_in_seconds: Some(10),
                poll: || OAuthDeviceCodePollResult::Failed {
                    message: "denied".to_string(),
                },
                is_cancelled: None,
            },
            Instant::now,
            |_| Ok(()),
        )
        .expect_err("failed poll");

        assert!(error.to_string().contains("denied"));
    }

    #[test]
    fn reports_slow_down_timeout_message() {
        let start = Instant::now();
        let elapsed = Rc::new(Cell::new(Duration::ZERO));
        let elapsed_for_now = Rc::clone(&elapsed);
        let elapsed_for_sleep = Rc::clone(&elapsed);

        let error = poll_oauth_device_code_flow_with_runtime(
            OAuthDeviceCodePollOptions {
                interval_seconds: Some(1),
                expires_in_seconds: Some(2),
                poll: || OAuthDeviceCodePollResult::SlowDown,
                is_cancelled: None,
            },
            move || start + elapsed_for_now.get(),
            move |duration| {
                elapsed_for_sleep.set(elapsed_for_sleep.get() + duration);
                Ok(())
            },
        )
        .expect_err("timeout");

        assert!(error.to_string().contains(SLOW_DOWN_TIMEOUT_MESSAGE));
    }

    #[test]
    fn supports_cancellation_before_sleep() {
        let error = poll_oauth_device_code_flow_with_runtime(
            OAuthDeviceCodePollOptions {
                interval_seconds: Some(1),
                expires_in_seconds: Some(10),
                poll: || OAuthDeviceCodePollResult::Pending,
                is_cancelled: Some(Box::new(|| true)),
            },
            Instant::now,
            |_| Ok(()),
        )
        .expect_err("cancelled");

        assert!(error.to_string().contains(CANCEL_MESSAGE));
    }
}
