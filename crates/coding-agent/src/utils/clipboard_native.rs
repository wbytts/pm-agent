#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipboardNativePlatform {
    Macos,
    Windows,
    Linux,
    Other,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ClipboardNativeEnvironment {
    pub termux: bool,
    pub display: bool,
    pub wayland_display: bool,
}

impl From<crate::utils::clipboard::ClipboardEnvironment> for ClipboardNativeEnvironment {
    fn from(environment: crate::utils::clipboard::ClipboardEnvironment) -> Self {
        Self {
            termux: environment.termux,
            display: environment.x11_display,
            wayland_display: environment.wayland_display,
        }
    }
}

pub fn should_try_clipboard_native(
    platform: ClipboardNativePlatform,
    environment: &ClipboardNativeEnvironment,
) -> bool {
    if environment.termux {
        return false;
    }

    platform != ClipboardNativePlatform::Linux || environment.display || environment.wayland_display
}

pub fn load_clipboard_native_from_roots<'a, R, T>(
    roots: impl IntoIterator<Item = &'a R>,
    mut load: impl FnMut(&R) -> Option<T>,
) -> Option<T>
where
    R: 'a + ?Sized,
{
    for root in roots {
        if let Some(clipboard) = load(root) {
            return Some(clipboard);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_clipboard_availability_matches_pi_clipboard_native() {
        assert!(!should_try_clipboard_native(
            ClipboardNativePlatform::Linux,
            &ClipboardNativeEnvironment {
                termux: true,
                display: true,
                wayland_display: false,
            },
        ));
        assert!(!should_try_clipboard_native(
            ClipboardNativePlatform::Linux,
            &ClipboardNativeEnvironment::default(),
        ));
        assert!(should_try_clipboard_native(
            ClipboardNativePlatform::Linux,
            &ClipboardNativeEnvironment {
                termux: false,
                display: false,
                wayland_display: true,
            },
        ));
        assert!(should_try_clipboard_native(
            ClipboardNativePlatform::Macos,
            &ClipboardNativeEnvironment::default(),
        ));
    }

    #[test]
    fn native_clipboard_loader_falls_back_between_resolution_roots_like_pi() {
        let mut attempted = Vec::new();
        let loaded = load_clipboard_native_from_roots(["bundled", "exec-dir"], |root| {
            attempted.push(root.to_string());
            (root == "exec-dir").then_some("native")
        });

        assert_eq!(loaded, Some("native"));
        assert_eq!(attempted, vec!["bundled", "exec-dir"]);

        let missing = load_clipboard_native_from_roots(["bundled"], |_| None::<&str>);
        assert_eq!(missing, None);
    }

    #[test]
    fn native_environment_maps_from_clipboard_environment() {
        let native = ClipboardNativeEnvironment::from(crate::utils::ClipboardEnvironment {
            termux: true,
            wayland_display: true,
            x11_display: true,
            wayland_session: false,
        });

        assert_eq!(
            native,
            ClipboardNativeEnvironment {
                termux: true,
                display: true,
                wayland_display: true,
            }
        );
    }
}
