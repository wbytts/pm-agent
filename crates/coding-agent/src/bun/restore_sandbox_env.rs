use std::collections::BTreeMap;
use std::fs;

pub fn restore_sandbox_env(is_bun_runtime: bool) {
    if !is_bun_runtime || std::env::vars_os().next().is_some() {
        return;
    }

    let Ok(data) = fs::read_to_string("/proc/self/environ") else {
        return;
    };

    for (key, value) in restored_sandbox_env_vars(true, true, &data).unwrap_or_default() {
        std::env::set_var(key, value);
    }
}

pub fn restored_sandbox_env_vars(
    is_bun_runtime: bool,
    env_is_empty: bool,
    proc_environ: &str,
) -> Option<BTreeMap<String, String>> {
    if !is_bun_runtime || !env_is_empty {
        return None;
    }
    Some(parse_proc_environ(proc_environ))
}

pub fn parse_proc_environ(data: &str) -> BTreeMap<String, String> {
    data.split('\0')
        .filter_map(|entry| {
            let index = entry.find('=')?;
            (index > 0).then(|| (entry[..index].to_string(), entry[index + 1..].to_string()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn parses_proc_environ_entries_like_pi_restore_sandbox_env() {
        assert_eq!(
            parse_proc_environ("FOO=bar\0BAZ=qux\0NO_VALUE\0EMPTY=\0"),
            BTreeMap::from([
                ("BAZ".to_string(), "qux".to_string()),
                ("EMPTY".to_string(), String::new()),
                ("FOO".to_string(), "bar".to_string()),
            ])
        );
    }

    #[test]
    fn plans_restore_only_for_bun_with_empty_env_like_pi_restore_sandbox_env() {
        assert_eq!(
            restored_sandbox_env_vars(true, true, "FOO=bar\0"),
            Some(BTreeMap::from([("FOO".to_string(), "bar".to_string())]))
        );
        assert_eq!(restored_sandbox_env_vars(false, true, "FOO=bar\0"), None);
        assert_eq!(restored_sandbox_env_vars(true, false, "FOO=bar\0"), None);
    }
}
