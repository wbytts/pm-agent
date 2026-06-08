use std::str::FromStr;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ModifierKey {
    Shift,
    Command,
    Control,
    Option,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParseModifierKeyError {
    value: String,
}

impl FromStr for ModifierKey {
    type Err = ParseModifierKeyError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "shift" => Ok(Self::Shift),
            "command" => Ok(Self::Command),
            "control" => Ok(Self::Control),
            "option" => Ok(Self::Option),
            _ => Err(ParseModifierKeyError {
                value: value.to_string(),
            }),
        }
    }
}

/// Rust 版本不加载 pi Node 包里的平台原生模块；没有可用 helper 时保持和源实现一致，返回 false。
pub fn is_native_modifier_pressed(_key: ModifierKey) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::{is_native_modifier_pressed, ModifierKey};
    use std::str::FromStr;

    #[test]
    fn modifier_key_parses_pi_native_modifier_names() {
        assert_eq!(ModifierKey::from_str("shift"), Ok(ModifierKey::Shift));
        assert_eq!(ModifierKey::from_str("command"), Ok(ModifierKey::Command));
        assert_eq!(ModifierKey::from_str("control"), Ok(ModifierKey::Control));
        assert_eq!(ModifierKey::from_str("option"), Ok(ModifierKey::Option));
        assert!(ModifierKey::from_str("alt").is_err());
    }

    #[test]
    fn native_modifier_query_is_false_without_platform_helper() {
        assert!(!is_native_modifier_pressed(ModifierKey::Shift));
        assert!(!is_native_modifier_pressed(ModifierKey::Command));
        assert!(!is_native_modifier_pressed(ModifierKey::Control));
        assert!(!is_native_modifier_pressed(ModifierKey::Option));
    }
}
