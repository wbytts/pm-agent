use std::time::Duration;

pub const DEFAULT_HTTP_IDLE_TIMEOUT_MS: u64 = 300_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HttpIdleTimeoutChoice {
    pub label: &'static str,
    pub timeout_ms: u64,
}

pub const HTTP_IDLE_TIMEOUT_CHOICES: &[HttpIdleTimeoutChoice] = &[
    HttpIdleTimeoutChoice {
        label: "30 sec",
        timeout_ms: 30_000,
    },
    HttpIdleTimeoutChoice {
        label: "1 min",
        timeout_ms: 60_000,
    },
    HttpIdleTimeoutChoice {
        label: "2 min",
        timeout_ms: 120_000,
    },
    HttpIdleTimeoutChoice {
        label: "5 min",
        timeout_ms: 300_000,
    },
    HttpIdleTimeoutChoice {
        label: "disabled",
        timeout_ms: 0,
    },
];

pub trait HttpIdleTimeoutInput {
    fn parse_http_idle_timeout(self) -> Option<u64>;
}

impl HttpIdleTimeoutInput for u64 {
    fn parse_http_idle_timeout(self) -> Option<u64> {
        Some(self)
    }
}

impl HttpIdleTimeoutInput for i64 {
    fn parse_http_idle_timeout(self) -> Option<u64> {
        (self >= 0).then_some(self as u64)
    }
}

impl HttpIdleTimeoutInput for f64 {
    fn parse_http_idle_timeout(self) -> Option<u64> {
        (self.is_finite() && self >= 0.0).then_some(self.floor() as u64)
    }
}

impl HttpIdleTimeoutInput for &str {
    fn parse_http_idle_timeout(self) -> Option<u64> {
        let trimmed = self.trim();
        if trimmed.eq_ignore_ascii_case("disabled") {
            return Some(0);
        }
        if trimmed.is_empty() {
            return None;
        }
        trimmed
            .parse::<f64>()
            .ok()
            .and_then(HttpIdleTimeoutInput::parse_http_idle_timeout)
    }
}

impl HttpIdleTimeoutInput for String {
    fn parse_http_idle_timeout(self) -> Option<u64> {
        self.as_str().parse_http_idle_timeout()
    }
}

pub fn parse_http_idle_timeout_ms<T: HttpIdleTimeoutInput>(value: T) -> Option<u64> {
    value.parse_http_idle_timeout()
}

pub fn format_http_idle_timeout_ms(timeout_ms: u64) -> String {
    HTTP_IDLE_TIMEOUT_CHOICES
        .iter()
        .find(|choice| choice.timeout_ms == timeout_ms)
        .map(|choice| choice.label.to_string())
        .unwrap_or_else(|| format!("{} sec", timeout_ms as f64 / 1000.0))
}

pub fn build_http_client(timeout_ms: u64) -> Result<reqwest::blocking::Client, reqwest::Error> {
    let normalized_timeout_ms =
        parse_http_idle_timeout_ms(timeout_ms).expect("u64 HTTP timeout should always be valid");
    let builder = reqwest::blocking::Client::builder();
    let builder = if normalized_timeout_ms == 0 {
        builder
    } else {
        builder.timeout(Duration::from_millis(normalized_timeout_ms))
    };
    builder.build()
}

pub fn configure_http_dispatcher(
    timeout_ms: u64,
) -> Result<reqwest::blocking::Client, reqwest::Error> {
    build_http_client(timeout_ms)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_http_idle_timeout_values() {
        assert_eq!(parse_http_idle_timeout_ms("disabled"), Some(0));
        assert_eq!(parse_http_idle_timeout_ms(" 60000 "), Some(60_000));
        assert_eq!(parse_http_idle_timeout_ms(""), None);
        assert_eq!(parse_http_idle_timeout_ms(-1_i64), None);
        assert_eq!(parse_http_idle_timeout_ms(12.8_f64), Some(12));
    }

    #[test]
    fn formats_known_and_custom_timeouts() {
        assert_eq!(format_http_idle_timeout_ms(30_000), "30 sec");
        assert_eq!(format_http_idle_timeout_ms(60_000), "1 min");
        assert_eq!(format_http_idle_timeout_ms(0), "disabled");
        assert_eq!(format_http_idle_timeout_ms(90_000), "90 sec");
    }

    #[test]
    fn builds_reqwest_client() {
        build_http_client(DEFAULT_HTTP_IDLE_TIMEOUT_MS).expect("client");
        build_http_client(0).expect("disabled timeout client");
    }
}
