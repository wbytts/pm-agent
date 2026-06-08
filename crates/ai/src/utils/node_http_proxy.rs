pub const UNSUPPORTED_PROXY_PROTOCOL_MESSAGE: &str =
    "Unsupported proxy protocol. SOCKS and PAC proxy URLs are not supported; use an HTTP or HTTPS proxy URL.";

const DEFAULT_PROXY_PORTS: &[(&str, u16)] = &[
    ("ftp", 21),
    ("gopher", 70),
    ("http", 80),
    ("https", 443),
    ("ws", 80),
    ("wss", 443),
];

pub fn resolve_http_proxy_url_for_target(target_url: &str) -> Result<Option<String>, String> {
    let Some(proxy) = get_proxy_for_url(target_url) else {
        return Ok(None);
    };

    let proxy_url = reqwest::Url::parse(&proxy)
        .map_err(|error| format!("Invalid proxy URL {proxy:?}: {error}"))?;
    match proxy_url.scheme() {
        "http" | "https" => Ok(Some(proxy_url.to_string())),
        protocol => Err(format!(
            "{UNSUPPORTED_PROXY_PROTOCOL_MESSAGE} Got {protocol}:"
        )),
    }
}

fn get_proxy_for_url(target_url: &str) -> Option<String> {
    let parsed_url = reqwest::Url::parse(target_url).ok()?;
    let protocol = parsed_url.scheme();
    let hostname = parsed_url.host_str()?;
    let port = parsed_url
        .port()
        .or_else(|| default_proxy_port(protocol))
        .unwrap_or(0);

    if !should_proxy_hostname(hostname, port) {
        return None;
    }

    let mut proxy = get_proxy_env(&format!("{protocol}_proxy"))
        .or_else(|| get_proxy_env("all_proxy"))
        .unwrap_or_default();
    if proxy.is_empty() {
        return None;
    }
    if !proxy.contains("://") {
        proxy = format!("{protocol}://{proxy}");
    }
    Some(proxy)
}

fn get_proxy_env(key: &str) -> Option<String> {
    std::env::var(key.to_lowercase())
        .ok()
        .filter(|value| !value.is_empty())
        .or_else(|| {
            std::env::var(key.to_uppercase())
                .ok()
                .filter(|value| !value.is_empty())
        })
}

fn should_proxy_hostname(hostname: &str, port: u16) -> bool {
    let no_proxy = get_proxy_env("no_proxy").unwrap_or_default().to_lowercase();
    if no_proxy.is_empty() {
        return true;
    }
    if no_proxy == "*" {
        return false;
    }

    no_proxy.split([',', ' ', '\t', '\n']).all(|entry| {
        let proxy = entry.trim();
        if proxy.is_empty() {
            return true;
        }

        let (mut proxy_hostname, proxy_port) = parse_no_proxy_entry(proxy);
        if proxy_port.is_some_and(|proxy_port| proxy_port != port) {
            return true;
        }

        if !proxy_hostname.starts_with(['.', '*']) {
            return hostname != proxy_hostname;
        }
        if let Some(stripped) = proxy_hostname.strip_prefix('*') {
            proxy_hostname = stripped;
        }
        !hostname.ends_with(proxy_hostname)
    })
}

fn parse_no_proxy_entry(proxy: &str) -> (&str, Option<u16>) {
    let Some((host, port)) = proxy.rsplit_once(':') else {
        return (proxy, None);
    };
    match port.parse::<u16>() {
        Ok(port) => (host, Some(port)),
        Err(_) => (proxy, None),
    }
}

fn default_proxy_port(protocol: &str) -> Option<u16> {
    DEFAULT_PROXY_PORTS
        .iter()
        .find_map(|(candidate, port)| (*candidate == protocol).then_some(*port))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use std::sync::{Mutex, OnceLock};

    const PROXY_ENV_KEYS: &[&str] = &[
        "HTTP_PROXY",
        "HTTPS_PROXY",
        "NO_PROXY",
        "ALL_PROXY",
        "http_proxy",
        "https_proxy",
        "no_proxy",
        "all_proxy",
    ];

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    fn reset_proxy_env() {
        for key in PROXY_ENV_KEYS {
            env::remove_var(key);
        }
    }

    #[test]
    fn respects_no_proxy_exclusions_like_pi_node_http_proxy() {
        let _guard = env_lock().lock().expect("env lock");
        reset_proxy_env();
        env::set_var("HTTPS_PROXY", "http://proxy.example:8080");
        env::set_var("NO_PROXY", "bedrock-runtime.us-east-1.amazonaws.com");

        assert_eq!(
            resolve_http_proxy_url_for_target("https://bedrock-runtime.us-east-1.amazonaws.com")
                .expect("proxy resolution"),
            None
        );
    }

    #[test]
    fn resolves_http_and_https_proxy_urls_like_pi_node_http_proxy() {
        let _guard = env_lock().lock().expect("env lock");
        reset_proxy_env();
        env::set_var("HTTPS_PROXY", "http://proxy.example:8080");

        assert_eq!(
            resolve_http_proxy_url_for_target("https://bedrock-runtime.us-east-1.amazonaws.com")
                .expect("proxy resolution"),
            Some("http://proxy.example:8080/".to_string())
        );
    }

    #[test]
    fn rejects_socks_and_pac_proxy_urls_like_pi_node_http_proxy() {
        let _guard = env_lock().lock().expect("env lock");
        reset_proxy_env();
        env::set_var("HTTPS_PROXY", "socks5://proxy.example:1080");

        let error =
            resolve_http_proxy_url_for_target("https://bedrock-runtime.us-east-1.amazonaws.com")
                .expect_err("unsupported proxy protocol should fail");
        assert!(error.starts_with(UNSUPPORTED_PROXY_PROTOCOL_MESSAGE));
    }

    #[test]
    fn follows_pi_proxy_env_priority_and_no_proxy_matching_rules() {
        let _guard = env_lock().lock().expect("env lock");
        reset_proxy_env();

        env::set_var("HTTPS_PROXY", "http://upper.example:8080");
        env::set_var("https_proxy", "http://lower.example:8080");
        assert_eq!(
            resolve_http_proxy_url_for_target("https://api.example.com").expect("proxy resolution"),
            Some("http://lower.example:8080/".to_string())
        );

        reset_proxy_env();
        env::set_var("ALL_PROXY", "proxy.example:8080");
        assert_eq!(
            resolve_http_proxy_url_for_target("https://api.example.com").expect("proxy resolution"),
            Some("https://proxy.example:8080/".to_string())
        );

        reset_proxy_env();
        env::set_var("HTTPS_PROXY", "http://proxy.example:8080");
        env::set_var("NO_PROXY", "*");
        assert_eq!(
            resolve_http_proxy_url_for_target("https://api.example.com").expect("proxy resolution"),
            None
        );

        reset_proxy_env();
        env::set_var("HTTPS_PROXY", "http://proxy.example:8080");
        env::set_var("NO_PROXY", "api.example.com:8443,*.internal");
        assert_eq!(
            resolve_http_proxy_url_for_target("https://api.example.com").expect("proxy resolution"),
            Some("http://proxy.example:8080/".to_string())
        );
        assert_eq!(
            resolve_http_proxy_url_for_target("https://service.internal")
                .expect("proxy resolution"),
            None
        );
    }
}
