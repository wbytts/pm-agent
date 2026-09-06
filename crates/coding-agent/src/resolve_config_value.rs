use std::collections::BTreeMap;
use std::process::{Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

static COMMAND_RESULT_CACHE: OnceLock<Mutex<BTreeMap<String, Option<String>>>> = OnceLock::new();
const COMMAND_TIMEOUT: Duration = Duration::from_secs(10);

/// 解析配置值，支持字面量、环境变量名和 `!command` shell 命令。
pub fn resolve_config_value(config: &str) -> Option<String> {
    if config.starts_with('!') {
        return execute_command(config);
    }
    std::env::var(config)
        .ok()
        .filter(|value| !value.is_empty())
        .or_else(|| Some(config.to_string()))
}

pub fn resolve_config_value_uncached(config: &str) -> Option<String> {
    if config.starts_with('!') {
        return execute_command_uncached(config);
    }
    std::env::var(config)
        .ok()
        .filter(|value| !value.is_empty())
        .or_else(|| Some(config.to_string()))
}

pub fn resolve_config_value_or_throw(config: &str, description: &str) -> Result<String, String> {
    if let Some(value) = resolve_config_value_uncached(config) {
        return Ok(value);
    }
    if let Some(command) = config.strip_prefix('!') {
        return Err(format!(
            "Failed to resolve {description} from shell command: {command}"
        ));
    }
    Err(format!("Failed to resolve {description}"))
}

pub fn resolve_headers(
    headers: Option<&BTreeMap<String, String>>,
) -> Option<BTreeMap<String, String>> {
    let headers = headers?;
    let resolved = headers
        .iter()
        .filter_map(|(key, value)| resolve_config_value(value).map(|value| (key.clone(), value)))
        .collect::<BTreeMap<_, _>>();
    (!resolved.is_empty()).then_some(resolved)
}

pub fn resolve_headers_or_throw(
    headers: Option<&BTreeMap<String, String>>,
    description: &str,
) -> Result<Option<BTreeMap<String, String>>, String> {
    let Some(headers) = headers else {
        return Ok(None);
    };
    let mut resolved = BTreeMap::new();
    for (key, value) in headers {
        resolved.insert(
            key.clone(),
            resolve_config_value_or_throw(value, &format!("{description} header \"{key}\""))?,
        );
    }
    Ok((!resolved.is_empty()).then_some(resolved))
}

pub fn clear_config_value_cache() {
    if let Some(cache) = COMMAND_RESULT_CACHE.get() {
        cache.lock().expect("config cache should lock").clear();
    }
}

fn execute_command(command_config: &str) -> Option<String> {
    let cache = COMMAND_RESULT_CACHE.get_or_init(|| Mutex::new(BTreeMap::new()));
    if let Some(value) = cache
        .lock()
        .expect("config cache should lock")
        .get(command_config)
        .cloned()
    {
        return value;
    }
    let value = execute_command_uncached(command_config);
    cache
        .lock()
        .expect("config cache should lock")
        .insert(command_config.to_string(), value.clone());
    value
}

fn execute_command_uncached(command_config: &str) -> Option<String> {
    let command = command_config.strip_prefix('!')?;
    let mut child = shell_command(command)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .stdin(Stdio::null())
        .spawn()
        .ok()?;

    let started = Instant::now();
    while started.elapsed() < COMMAND_TIMEOUT {
        match child.try_wait() {
            Ok(Some(status)) => {
                if !status.success() {
                    return None;
                }
                let output = child.wait_with_output().ok()?;
                let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
                return (!value.is_empty()).then_some(value);
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(_) => return None,
        }
    }
    let _ = child.kill();
    None
}

#[cfg(windows)]
fn shell_command(command: &str) -> Command {
    let mut cmd = Command::new("cmd");
    cmd.args(["/C", command]);
    cmd
}

#[cfg(not(windows))]
fn shell_command(command: &str) -> Command {
    let mut cmd = Command::new("sh");
    cmd.args(["-c", command]);
    cmd
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn resolves_literals_and_environment_names() {
        std::env::set_var("PM_AGENT_TEST_CONFIG_VALUE", "from_env");
        assert_eq!(
            resolve_config_value("PM_AGENT_TEST_CONFIG_VALUE").as_deref(),
            Some("from_env")
        );
        assert_eq!(resolve_config_value("literal").as_deref(), Some("literal"));
    }

    #[test]
    fn resolves_command_values() {
        clear_config_value_cache();
        assert_eq!(
            resolve_config_value("!printf resolved").as_deref(),
            Some("resolved")
        );
    }

    #[test]
    fn failed_command_results_are_cached_like_pi() {
        clear_config_value_cache();
        let counter = temp_file("failed-command-cache");
        fs::write(&counter, "0").expect("counter should be written");
        let command = format!(
            "!sh -c 'count=$(cat \"{}\"); echo $((count + 1)) > \"{}\"; exit 1'",
            sh_path(&counter),
            sh_path(&counter)
        );

        assert_eq!(resolve_config_value(&command), None);
        assert_eq!(resolve_config_value(&command), None);

        assert_eq!(
            fs::read_to_string(&counter).expect("counter should be readable"),
            "1\n"
        );
    }

    #[test]
    fn clearing_cache_allows_command_to_run_again_like_pi() {
        clear_config_value_cache();
        let counter = temp_file("clear-command-cache");
        fs::write(&counter, "0").expect("counter should be written");
        let command = format!(
            "!sh -c 'count=$(cat \"{}\"); echo $((count + 1)) > \"{}\"; echo value'",
            sh_path(&counter),
            sh_path(&counter)
        );

        assert_eq!(resolve_config_value(&command).as_deref(), Some("value"));
        clear_config_value_cache();
        assert_eq!(resolve_config_value(&command).as_deref(), Some("value"));

        assert_eq!(
            fs::read_to_string(&counter).expect("counter should be readable"),
            "2\n"
        );
    }

    #[test]
    fn environment_values_are_not_cached_like_pi() {
        std::env::set_var("PM_AGENT_TEST_UNCACHED_CONFIG_VALUE", "first");
        assert_eq!(
            resolve_config_value("PM_AGENT_TEST_UNCACHED_CONFIG_VALUE").as_deref(),
            Some("first")
        );
        std::env::set_var("PM_AGENT_TEST_UNCACHED_CONFIG_VALUE", "second");
        assert_eq!(
            resolve_config_value("PM_AGENT_TEST_UNCACHED_CONFIG_VALUE").as_deref(),
            Some("second")
        );
        std::env::remove_var("PM_AGENT_TEST_UNCACHED_CONFIG_VALUE");
    }

    #[test]
    fn empty_environment_values_fall_back_to_literal_like_pi() {
        std::env::set_var("PM_AGENT_TEST_EMPTY_HEADER", "");

        assert_eq!(
            resolve_config_value("PM_AGENT_TEST_EMPTY_HEADER").as_deref(),
            Some("PM_AGENT_TEST_EMPTY_HEADER")
        );
        std::env::remove_var("PM_AGENT_TEST_EMPTY_HEADER");
    }

    fn temp_file(label: &str) -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be valid")
            .as_nanos();
        std::env::temp_dir().join(format!("pm-agent-resolve-config-{label}-{nanos}"))
    }

    fn sh_path(path: &std::path::Path) -> String {
        path.to_string_lossy()
            .replace('\\', "/")
            .replace('"', "\\\"")
    }
}
