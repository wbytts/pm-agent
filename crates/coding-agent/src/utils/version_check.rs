use std::cmp::Ordering;
use std::time::Duration;

use serde::Deserialize;

use super::pi_user_agent::get_pi_user_agent;

const LATEST_VERSION_URL: &str = "https://pi.dev/api/latest-version";
const DEFAULT_VERSION_CHECK_TIMEOUT_MS: u64 = 10_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LatestPiRelease {
    pub version: String,
    pub package_name: Option<String>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionCheckOptions {
    pub timeout_ms: u64,
}

impl Default for VersionCheckOptions {
    fn default() -> Self {
        Self {
            timeout_ms: DEFAULT_VERSION_CHECK_TIMEOUT_MS,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedVersion {
    major: u64,
    minor: u64,
    patch: u64,
    prerelease: Option<String>,
}

#[derive(Debug, Deserialize)]
struct LatestPiReleaseResponse {
    #[serde(rename = "packageName")]
    package_name: Option<String>,
    version: Option<String>,
    note: Option<String>,
}

pub fn compare_package_versions(left_version: &str, right_version: &str) -> Option<Ordering> {
    let left = parse_package_version(left_version)?;
    let right = parse_package_version(right_version)?;

    Some(
        left.major
            .cmp(&right.major)
            .then(left.minor.cmp(&right.minor))
            .then(left.patch.cmp(&right.patch))
            .then_with(|| {
                compare_prerelease(left.prerelease.as_deref(), right.prerelease.as_deref())
            }),
    )
}

pub fn is_newer_package_version(candidate_version: &str, current_version: &str) -> bool {
    match compare_package_versions(candidate_version, current_version) {
        Some(ordering) => ordering == Ordering::Greater,
        None => candidate_version.trim() != current_version.trim(),
    }
}

pub fn get_latest_pi_release(
    current_version: &str,
    options: Option<VersionCheckOptions>,
) -> Result<Option<LatestPiRelease>, reqwest::Error> {
    if std::env::var_os("PI_SKIP_VERSION_CHECK").is_some()
        || std::env::var_os("PI_OFFLINE").is_some()
    {
        return Ok(None);
    }

    let options = options.unwrap_or_default();
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_millis(options.timeout_ms))
        .build()?;
    let response = client
        .get(LATEST_VERSION_URL)
        .header("User-Agent", get_pi_user_agent(current_version))
        .header("accept", "application/json")
        .send()?;

    if !response.status().is_success() {
        return Ok(None);
    }

    let data = response.json::<LatestPiReleaseResponse>()?;
    Ok(normalize_latest_release_response(data))
}

pub fn get_latest_pi_version(
    current_version: &str,
    options: Option<VersionCheckOptions>,
) -> Result<Option<String>, reqwest::Error> {
    Ok(get_latest_pi_release(current_version, options)?.map(|release| release.version))
}

pub fn check_for_new_pi_version(current_version: &str) -> Option<LatestPiRelease> {
    get_latest_pi_release(current_version, None)
        .ok()
        .flatten()
        .filter(|release| is_newer_package_version(&release.version, current_version))
}

fn parse_package_version(version: &str) -> Option<ParsedVersion> {
    let version = version.trim();
    let version = version.strip_prefix('v').unwrap_or(version);
    let (version, _) = version.split_once('+').unwrap_or((version, ""));
    let (core, prerelease) = match version.split_once('-') {
        Some((core, prerelease)) => {
            if prerelease.is_empty()
                || !prerelease
                    .chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || ch == '.' || ch == '-')
            {
                return None;
            }
            (core, Some(prerelease.to_string()))
        }
        None => (version, None),
    };
    let mut parts = core.split('.');

    let major = parse_version_number(parts.next()?)?;
    let minor = parse_version_number(parts.next()?)?;
    let patch = parse_version_number(parts.next()?)?;
    if parts.next().is_some() {
        return None;
    }

    Some(ParsedVersion {
        major,
        minor,
        patch,
        prerelease,
    })
}

fn parse_version_number(value: &str) -> Option<u64> {
    if value.is_empty() || !value.chars().all(|ch| ch.is_ascii_digit()) {
        return None;
    }
    value.parse().ok()
}

fn compare_prerelease(left: Option<&str>, right: Option<&str>) -> Ordering {
    match (left, right) {
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Greater,
        (Some(_), None) => Ordering::Less,
        (Some(left), Some(right)) => left.cmp(right),
    }
}

fn normalize_latest_release_response(data: LatestPiReleaseResponse) -> Option<LatestPiRelease> {
    let version = data.version?.trim().to_string();
    if version.is_empty() {
        return None;
    }

    Some(LatestPiRelease {
        version,
        package_name: data
            .package_name
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty()),
        note: data
            .note
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_package_versions_with_prerelease() {
        assert_eq!(
            compare_package_versions("v1.2.3", "1.2.2"),
            Some(Ordering::Greater)
        );
        assert_eq!(
            compare_package_versions("1.2.3-alpha", "1.2.3"),
            Some(Ordering::Less)
        );
        assert_eq!(
            compare_package_versions("1.2.3+build", "1.2.3"),
            Some(Ordering::Equal)
        );
        assert_eq!(compare_package_versions("bad", "1.2.3"), None);
    }

    #[test]
    fn rejects_invalid_package_version_shapes_like_pi_regex() {
        assert_eq!(compare_package_versions("1.2.3-", "1.2.3"), None);
        assert_eq!(compare_package_versions("vv1.2.3", "1.2.3"), None);
        assert!(is_newer_package_version("1.2.3-", "1.2.3"));
        assert!(is_newer_package_version("vv1.2.3", "1.2.3"));
    }

    #[test]
    fn detects_newer_version_with_string_fallback() {
        assert!(is_newer_package_version("1.2.4", "1.2.3"));
        assert!(!is_newer_package_version("1.2.3-alpha", "1.2.3"));
        assert!(is_newer_package_version("next", "current"));
        assert!(!is_newer_package_version("same", "same"));
    }

    #[test]
    fn normalizes_latest_release_response() {
        let release = normalize_latest_release_response(LatestPiReleaseResponse {
            version: Some(" 1.2.3 ".to_string()),
            package_name: Some(" @scope/pi ".to_string()),
            note: Some(" ".to_string()),
        })
        .expect("release");

        assert_eq!(release.version, "1.2.3");
        assert_eq!(release.package_name.as_deref(), Some("@scope/pi"));
        assert_eq!(release.note, None);
    }
}
