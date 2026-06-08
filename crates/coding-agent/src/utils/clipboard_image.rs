pub fn is_wayland_session<K, V>(env: impl IntoIterator<Item = (K, V)>) -> bool
where
    K: AsRef<str>,
    V: AsRef<str>,
{
    env_contains_wayland(env)
}

fn env_contains_wayland<K, V>(env: impl IntoIterator<Item = (K, V)>) -> bool
where
    K: AsRef<str>,
    V: AsRef<str>,
{
    env.into_iter().any(|(key, value)| {
        let key = key.as_ref();
        let value = value.as_ref();
        (key == "WAYLAND_DISPLAY" && !value.is_empty())
            || (key == "XDG_SESSION_TYPE" && value == "wayland")
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipboardImageReadBackend {
    WlPaste,
    Xclip,
    PowerShell,
    Native,
}

pub fn clipboard_image_read_plan<K, V>(
    platform: &str,
    env: impl IntoIterator<Item = (K, V)>,
    is_wsl: bool,
) -> Vec<ClipboardImageReadBackend>
where
    K: AsRef<str>,
    V: AsRef<str>,
{
    let env = env
        .into_iter()
        .map(|(key, value)| (key.as_ref().to_string(), value.as_ref().to_string()))
        .collect::<Vec<_>>();
    if env
        .iter()
        .any(|(key, value)| key == "TERMUX_VERSION" && !value.is_empty())
    {
        return Vec::new();
    }

    if platform != "linux" {
        return vec![ClipboardImageReadBackend::Native];
    }

    let wayland = env_contains_wayland(
        env.iter()
            .map(|(key, value)| (key.as_str(), value.as_str())),
    );
    let mut plan = Vec::new();
    if wayland || is_wsl {
        plan.push(ClipboardImageReadBackend::WlPaste);
        plan.push(ClipboardImageReadBackend::Xclip);
    }
    if is_wsl {
        plan.push(ClipboardImageReadBackend::PowerShell);
    }
    if !wayland {
        plan.push(ClipboardImageReadBackend::Native);
    }
    plan
}

pub fn extension_for_image_mime_type(mime_type: &str) -> Option<&'static str> {
    match base_mime_type(mime_type).as_str() {
        "image/png" => Some("png"),
        "image/jpeg" => Some("jpg"),
        "image/webp" => Some("webp"),
        "image/gif" => Some("gif"),
        _ => None,
    }
}

pub fn select_preferred_image_mime_type<'a>(
    mime_types: impl IntoIterator<Item = &'a str>,
) -> Option<String> {
    let normalized = mime_types
        .into_iter()
        .map(str::trim)
        .filter(|mime_type| !mime_type.is_empty())
        .map(|raw| (raw.to_string(), base_mime_type(raw)))
        .collect::<Vec<_>>();

    for preferred in ["image/png", "image/jpeg", "image/webp", "image/gif"] {
        if let Some((raw, _)) = normalized.iter().find(|(_, base)| base == preferred) {
            return Some(raw.clone());
        }
    }

    normalized
        .into_iter()
        .find(|(_, base)| base.starts_with("image/"))
        .map(|(raw, _)| raw)
}

fn base_mime_type(mime_type: &str) -> String {
    mime_type
        .split(';')
        .next()
        .unwrap_or(mime_type)
        .trim()
        .to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn detects_wayland_session_like_pi_clipboard_image() {
        assert!(is_wayland_session(HashMap::from([(
            "WAYLAND_DISPLAY",
            "wayland-1"
        )])));
        assert!(is_wayland_session(HashMap::from([(
            "XDG_SESSION_TYPE",
            "wayland"
        )])));
        assert!(!is_wayland_session(HashMap::from([(
            "XDG_SESSION_TYPE",
            "x11"
        )])));
    }

    #[test]
    fn maps_image_mime_type_extensions_like_pi_clipboard_image() {
        assert_eq!(
            extension_for_image_mime_type(" image/png; charset=binary "),
            Some("png")
        );
        assert_eq!(extension_for_image_mime_type("IMAGE/JPEG"), Some("jpg"));
        assert_eq!(extension_for_image_mime_type("image/webp"), Some("webp"));
        assert_eq!(extension_for_image_mime_type("image/gif"), Some("gif"));
        assert_eq!(extension_for_image_mime_type("image/bmp"), None);
    }

    #[test]
    fn plans_clipboard_image_read_backends_like_pi_clipboard_image() {
        assert_eq!(
            clipboard_image_read_plan(
                "linux",
                HashMap::from([("WAYLAND_DISPLAY", "wayland-1")]),
                false,
            ),
            vec![
                ClipboardImageReadBackend::WlPaste,
                ClipboardImageReadBackend::Xclip,
            ]
        );
        assert_eq!(
            clipboard_image_read_plan("linux", HashMap::from([("WSLENV", "1")]), true),
            vec![
                ClipboardImageReadBackend::WlPaste,
                ClipboardImageReadBackend::Xclip,
                ClipboardImageReadBackend::PowerShell,
                ClipboardImageReadBackend::Native,
            ]
        );
        assert_eq!(
            clipboard_image_read_plan("linux", HashMap::<&str, &str>::new(), false),
            vec![ClipboardImageReadBackend::Native]
        );
        assert_eq!(
            clipboard_image_read_plan("darwin", HashMap::<&str, &str>::new(), false),
            vec![ClipboardImageReadBackend::Native]
        );
        assert!(clipboard_image_read_plan(
            "linux",
            HashMap::from([("TERMUX_VERSION", "1")]),
            false
        )
        .is_empty());
    }

    #[test]
    fn selects_preferred_image_mime_type_like_pi_clipboard_image() {
        assert_eq!(
            select_preferred_image_mime_type([" text/plain ", "image/webp", "image/png"]),
            Some("image/png".to_string())
        );
        assert_eq!(
            select_preferred_image_mime_type(["image/bmp; format=dib", "text/plain"]),
            Some("image/bmp; format=dib".to_string())
        );
        assert_eq!(select_preferred_image_mime_type(["text/plain", ""]), None);
    }
}
