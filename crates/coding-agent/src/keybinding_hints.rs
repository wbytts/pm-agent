use tui::KeybindingsManager;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct KeyTextFormatOptions {
    pub capitalize: bool,
}

pub fn format_key_text(key: &str, options: KeyTextFormatOptions) -> String {
    key.split('/')
        .map(|candidate| {
            candidate
                .split('+')
                .map(|part| format_key_part(part, options))
                .collect::<Vec<_>>()
                .join("+")
        })
        .collect::<Vec<_>>()
        .join("/")
}

pub fn key_text(manager: &KeybindingsManager, keybinding: &str) -> String {
    format_keys(&manager.keys(keybinding), KeyTextFormatOptions::default())
}

pub fn key_display_text(manager: &KeybindingsManager, keybinding: &str) -> String {
    format_keys(
        &manager.keys(keybinding),
        KeyTextFormatOptions { capitalize: true },
    )
}

pub fn raw_key_hint(key: &str, description: &str) -> String {
    format!("{} {description}", format_key_text(key, Default::default()))
}

pub fn key_hint(manager: &KeybindingsManager, keybinding: &str, description: &str) -> String {
    format!("{} {description}", key_text(manager, keybinding))
}

fn format_keys(keys: &[String], options: KeyTextFormatOptions) -> String {
    if keys.is_empty() {
        String::new()
    } else {
        format_key_text(&keys.join("/"), options)
    }
}

fn format_key_part(part: &str, options: KeyTextFormatOptions) -> String {
    let display_part = if cfg!(target_os = "macos") && part.eq_ignore_ascii_case("alt") {
        "option"
    } else {
        part
    };

    if options.capitalize {
        capitalize_first(display_part)
    } else {
        display_part.to_string()
    }
}

fn capitalize_first(value: &str) -> String {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return String::new();
    };
    first.to_uppercase().collect::<String>() + chars.as_str()
}

#[cfg(test)]
mod tests {
    use super::{format_key_text, raw_key_hint, KeyTextFormatOptions};

    #[test]
    fn format_key_text_preserves_combinations_and_alternates() {
        assert_eq!(
            format_key_text("ctrl+x/shift+tab", Default::default()),
            "ctrl+x/shift+tab"
        );
    }

    #[test]
    fn format_key_text_can_capitalize_key_parts() {
        assert_eq!(
            format_key_text(
                "ctrl+x/shift+tab",
                KeyTextFormatOptions { capitalize: true },
            ),
            "Ctrl+X/Shift+Tab"
        );
    }

    #[test]
    fn raw_key_hint_formats_key_and_description() {
        assert_eq!(raw_key_hint("ctrl+x", "cancel"), "ctrl+x cancel");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn format_key_text_uses_option_for_alt_on_macos() {
        assert_eq!(
            format_key_text("alt+enter", Default::default()),
            "option+enter"
        );
    }
}
