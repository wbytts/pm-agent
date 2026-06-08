use std::sync::atomic::{AtomicBool, Ordering};

static KITTY_PROTOCOL_ACTIVE: AtomicBool = AtomicBool::new(false);

const ESC: &str = "\x1b";
const MOD_SHIFT: u8 = 1;
const MOD_ALT: u8 = 2;
const MOD_CTRL: u8 = 4;
const MOD_SUPER: u8 = 8;
const LOCK_MASK: u8 = 64 | 128;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedKeyId {
    pub key: String,
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub super_modifier: bool,
}

pub struct Key;

impl Key {
    pub fn escape() -> &'static str {
        "escape"
    }

    pub fn esc() -> &'static str {
        "esc"
    }

    pub fn enter() -> &'static str {
        "enter"
    }

    pub fn return_key() -> &'static str {
        "return"
    }

    pub fn tab() -> &'static str {
        "tab"
    }

    pub fn space() -> &'static str {
        "space"
    }

    pub fn backspace() -> &'static str {
        "backspace"
    }

    pub fn delete() -> &'static str {
        "delete"
    }

    pub fn insert() -> &'static str {
        "insert"
    }

    pub fn clear() -> &'static str {
        "clear"
    }

    pub fn home() -> &'static str {
        "home"
    }

    pub fn end() -> &'static str {
        "end"
    }

    pub fn page_up() -> &'static str {
        "pageUp"
    }

    pub fn page_down() -> &'static str {
        "pageDown"
    }

    pub fn up() -> &'static str {
        "up"
    }

    pub fn down() -> &'static str {
        "down"
    }

    pub fn left() -> &'static str {
        "left"
    }

    pub fn right() -> &'static str {
        "right"
    }

    pub fn f1() -> &'static str {
        "f1"
    }

    pub fn f2() -> &'static str {
        "f2"
    }

    pub fn f3() -> &'static str {
        "f3"
    }

    pub fn f4() -> &'static str {
        "f4"
    }

    pub fn f5() -> &'static str {
        "f5"
    }

    pub fn f6() -> &'static str {
        "f6"
    }

    pub fn f7() -> &'static str {
        "f7"
    }

    pub fn f8() -> &'static str {
        "f8"
    }

    pub fn f9() -> &'static str {
        "f9"
    }

    pub fn f10() -> &'static str {
        "f10"
    }

    pub fn f11() -> &'static str {
        "f11"
    }

    pub fn f12() -> &'static str {
        "f12"
    }

    pub fn backtick() -> &'static str {
        "`"
    }

    pub fn hyphen() -> &'static str {
        "-"
    }

    pub fn equals() -> &'static str {
        "="
    }

    pub fn left_bracket() -> &'static str {
        "["
    }

    pub fn right_bracket() -> &'static str {
        "]"
    }

    pub fn backslash() -> &'static str {
        "\\"
    }

    pub fn semicolon() -> &'static str {
        ";"
    }

    pub fn quote() -> &'static str {
        "'"
    }

    pub fn comma() -> &'static str {
        ","
    }

    pub fn period() -> &'static str {
        "."
    }

    pub fn slash() -> &'static str {
        "/"
    }

    pub fn exclamation() -> &'static str {
        "!"
    }

    pub fn at() -> &'static str {
        "@"
    }

    pub fn hash() -> &'static str {
        "#"
    }

    pub fn dollar() -> &'static str {
        "$"
    }

    pub fn percent() -> &'static str {
        "%"
    }

    pub fn caret() -> &'static str {
        "^"
    }

    pub fn ampersand() -> &'static str {
        "&"
    }

    pub fn asterisk() -> &'static str {
        "*"
    }

    pub fn left_paren() -> &'static str {
        "("
    }

    pub fn right_paren() -> &'static str {
        ")"
    }

    pub fn underscore() -> &'static str {
        "_"
    }

    pub fn plus() -> &'static str {
        "+"
    }

    pub fn pipe() -> &'static str {
        "|"
    }

    pub fn tilde() -> &'static str {
        "~"
    }

    pub fn left_brace() -> &'static str {
        "{"
    }

    pub fn right_brace() -> &'static str {
        "}"
    }

    pub fn colon() -> &'static str {
        ":"
    }

    pub fn less_than() -> &'static str {
        "<"
    }

    pub fn greater_than() -> &'static str {
        ">"
    }

    pub fn question() -> &'static str {
        "?"
    }

    pub fn ctrl(key: &str) -> String {
        format!("ctrl+{key}")
    }

    pub fn shift(key: &str) -> String {
        format!("shift+{key}")
    }

    pub fn alt(key: &str) -> String {
        format!("alt+{key}")
    }

    pub fn super_key(key: &str) -> String {
        format!("super+{key}")
    }

    pub fn ctrl_shift(key: &str) -> String {
        format!("ctrl+shift+{key}")
    }

    pub fn shift_ctrl(key: &str) -> String {
        format!("shift+ctrl+{key}")
    }

    pub fn ctrl_alt(key: &str) -> String {
        format!("ctrl+alt+{key}")
    }

    pub fn alt_ctrl(key: &str) -> String {
        format!("alt+ctrl+{key}")
    }

    pub fn shift_alt(key: &str) -> String {
        format!("shift+alt+{key}")
    }

    pub fn alt_shift(key: &str) -> String {
        format!("alt+shift+{key}")
    }

    pub fn ctrl_super(key: &str) -> String {
        format!("ctrl+super+{key}")
    }

    pub fn super_ctrl(key: &str) -> String {
        format!("super+ctrl+{key}")
    }

    pub fn shift_super(key: &str) -> String {
        format!("shift+super+{key}")
    }

    pub fn super_shift(key: &str) -> String {
        format!("super+shift+{key}")
    }

    pub fn alt_super(key: &str) -> String {
        format!("alt+super+{key}")
    }

    pub fn super_alt(key: &str) -> String {
        format!("super+alt+{key}")
    }

    pub fn ctrl_shift_alt(key: &str) -> String {
        format!("ctrl+shift+alt+{key}")
    }

    pub fn ctrl_shift_super(key: &str) -> String {
        format!("ctrl+shift+super+{key}")
    }
}

pub fn set_kitty_protocol_active(active: bool) {
    KITTY_PROTOCOL_ACTIVE.store(active, Ordering::Relaxed);
}

pub fn is_kitty_protocol_active() -> bool {
    KITTY_PROTOCOL_ACTIVE.load(Ordering::Relaxed)
}

pub fn is_key_release(data: &str) -> bool {
    if data.contains("\x1b[200~") {
        return false;
    }
    [":3u", ":3~", ":3A", ":3B", ":3C", ":3D", ":3H", ":3F"]
        .iter()
        .any(|marker| data.contains(marker))
}

pub fn is_key_repeat(data: &str) -> bool {
    if data.contains("\x1b[200~") {
        return false;
    }
    [":2u", ":2~", ":2A", ":2B", ":2C", ":2D", ":2H", ":2F"]
        .iter()
        .any(|marker| data.contains(marker))
}

pub fn parse_key(data: &str) -> Option<String> {
    if is_kitty_protocol_active() && (data == "\x1b\r" || data == "\n") {
        return Some("shift+enter".to_string());
    }
    if let Some(parsed) = parse_kitty_sequence(data) {
        return format_kitty_key(parsed);
    }
    if let Some(parsed) = parse_modify_other_keys_sequence(data) {
        return format_key_name_for_codepoint(parsed.codepoint, parsed.modifier);
    }
    if let Some(key) = parse_legacy_key(data) {
        return Some(key);
    }
    let candidates = [
        "escape",
        "enter",
        "tab",
        "space",
        "backspace",
        "delete",
        "insert",
        "home",
        "end",
        "pageUp",
        "pageDown",
        "up",
        "down",
        "left",
        "right",
        "f1",
        "f2",
        "f3",
        "f4",
        "f5",
        "f6",
        "f7",
        "f8",
        "f9",
        "f10",
        "f11",
        "f12",
    ];
    for key in candidates {
        if matches_key(data, key) {
            return Some(key.to_string());
        }
    }
    if data.chars().count() == 1 {
        let ch = data.chars().next()?;
        let code = ch as u32;
        if (1..=26).contains(&code) {
            return char::from_u32(code + 96).map(|letter| format!("ctrl+{letter}"));
        }
        if code >= 32 {
            return Some(data.to_string());
        }
    }
    None
}

pub fn decode_kitty_printable(data: &str) -> Option<String> {
    let body = data
        .strip_prefix("\x1b[")
        .and_then(|value| value.strip_suffix('u'))?;
    let (codepoint_part, modifier_part) = body.split_once(';').unwrap_or((body, "1"));
    let mut codepoints = codepoint_part.split(':');
    let codepoint = codepoints.next()?.parse::<u32>().ok()?;
    let shifted_key = codepoints.next().and_then(|value| {
        (!value.is_empty())
            .then_some(value)
            .and_then(|value| value.parse::<u32>().ok())
    });
    let modifier = modifier_part
        .split(':')
        .next()
        .and_then(|value| value.parse::<u8>().ok())
        .unwrap_or(1)
        .saturating_sub(1);

    let allowed_modifiers = MOD_SHIFT | LOCK_MASK;
    if modifier & !allowed_modifiers != 0 || modifier & (MOD_ALT | MOD_CTRL) != 0 {
        return None;
    }

    let effective_codepoint = if modifier & MOD_SHIFT != 0 {
        shifted_key.unwrap_or(codepoint)
    } else {
        codepoint
    };
    let effective_codepoint = normalize_codepoint(effective_codepoint as i32, 0);
    if effective_codepoint < 32 {
        return None;
    }
    char::from_u32(effective_codepoint as u32).map(|ch| ch.to_string())
}

pub fn decode_printable_key(data: &str) -> Option<String> {
    decode_kitty_printable(data).or_else(|| decode_modify_other_keys_printable(data))
}

pub fn matches_key(data: &str, key_id: &str) -> bool {
    let Some(parsed) = parse_key_id(key_id) else {
        return false;
    };
    let modifier = parsed.modifier();
    let key = parsed.key.as_str();

    match key {
        "escape" | "esc" => modifier == 0 && (data == ESC || matches_kitty(data, 27, 0)),
        "space" => match modifier {
            0 => data == " " || matches_kitty(data, 32, 0),
            MOD_CTRL => data == "\x00" || matches_kitty(data, 32, MOD_CTRL),
            MOD_ALT => data == "\x1b " || matches_kitty(data, 32, MOD_ALT),
            _ => matches_kitty(data, 32, modifier) || matches_modify_other_keys(data, 32, modifier),
        },
        "tab" => match modifier {
            0 => data == "\t" || matches_kitty(data, 9, 0),
            MOD_SHIFT => {
                data == "\x1b[Z"
                    || matches_kitty(data, 9, MOD_SHIFT)
                    || matches_modify_other_keys(data, 9, MOD_SHIFT)
            }
            _ => matches_kitty(data, 9, modifier) || matches_modify_other_keys(data, 9, modifier),
        },
        "enter" | "return" => match modifier {
            0 => {
                data == "\r"
                    || (!is_kitty_protocol_active() && data == "\n")
                    || data == "\x1bOM"
                    || matches_kitty(data, 13, 0)
                    || matches_kitty(data, 57414, 0)
            }
            MOD_SHIFT => {
                matches_kitty(data, 13, MOD_SHIFT)
                    || matches_kitty(data, 57414, MOD_SHIFT)
                    || matches_modify_other_keys(data, 13, MOD_SHIFT)
                    || (is_kitty_protocol_active() && (data == "\x1b\r" || data == "\n"))
            }
            MOD_ALT => {
                matches_kitty(data, 13, MOD_ALT)
                    || matches_kitty(data, 57414, MOD_ALT)
                    || matches_modify_other_keys(data, 13, MOD_ALT)
                    || (!is_kitty_protocol_active() && data == "\x1b\r")
            }
            _ => {
                matches_kitty(data, 13, modifier)
                    || matches_kitty(data, 57414, modifier)
                    || matches_modify_other_keys(data, 13, modifier)
            }
        },
        "backspace" => match modifier {
            0 => data == "\x7f" || data == "\x08" || matches_kitty(data, 127, 0),
            MOD_ALT => {
                data == "\x1b\x7f" || data == "\x1b\x08" || matches_kitty(data, 127, MOD_ALT)
            }
            MOD_CTRL => {
                matches_kitty(data, 127, MOD_CTRL) || matches_modify_other_keys(data, 127, MOD_CTRL)
            }
            _ => {
                matches_kitty(data, 127, modifier) || matches_modify_other_keys(data, 127, modifier)
            }
        },
        "up" | "down" | "right" | "left" => matches_direction(data, key, modifier),
        "home" | "end" | "insert" | "delete" | "pageup" | "pagedown" | "pageUp" | "pageDown" => {
            matches_functional(data, canonical_functional_key(key), modifier)
        }
        "clear" => matches_legacy(data, "clear", modifier),
        key if function_key_number(key).is_some() => matches_function_key(data, key, modifier),
        key if key.chars().count() == 1 => matches_printable(data, key, modifier),
        _ => false,
    }
}

pub fn parse_key_id(key_id: &str) -> Option<ParsedKeyId> {
    let parts = key_id.split('+').collect::<Vec<_>>();
    let key = parts.last()?.to_lowercase();
    if key.is_empty() {
        return None;
    }
    Some(ParsedKeyId {
        key,
        ctrl: parts.iter().any(|part| part.eq_ignore_ascii_case("ctrl")),
        shift: parts.iter().any(|part| part.eq_ignore_ascii_case("shift")),
        alt: parts.iter().any(|part| part.eq_ignore_ascii_case("alt")),
        super_modifier: parts.iter().any(|part| part.eq_ignore_ascii_case("super")),
    })
}

impl ParsedKeyId {
    fn modifier(&self) -> u8 {
        (u8::from(self.shift) * MOD_SHIFT)
            | (u8::from(self.alt) * MOD_ALT)
            | (u8::from(self.ctrl) * MOD_CTRL)
            | (u8::from(self.super_modifier) * MOD_SUPER)
    }
}

fn matches_printable(data: &str, key: &str, modifier: u8) -> bool {
    let Some(ch) = key.chars().next() else {
        return false;
    };
    if modifier == 0 {
        return data == key;
    }
    if modifier == MOD_CTRL {
        return raw_ctrl_char(ch).is_some_and(|ctrl| data == ctrl.to_string())
            || matches_kitty(data, ch as i32, modifier)
            || matches_modify_other_keys(data, ch as i32, modifier);
    }
    if modifier == MOD_ALT {
        return data == format!("\x1b{key}");
    }
    matches_kitty(data, ch as i32, modifier) || matches_modify_other_keys(data, ch as i32, modifier)
}

fn parse_legacy_key(data: &str) -> Option<String> {
    match data {
        "\x1b" => return Some("escape".to_string()),
        "\x1c" => return Some("ctrl+\\".to_string()),
        "\x1d" => return Some("ctrl+]".to_string()),
        "\x1f" => return Some("ctrl+-".to_string()),
        "\x1b\x1b" => return Some("ctrl+alt+[".to_string()),
        "\x1b\x1c" => return Some("ctrl+alt+\\".to_string()),
        "\x1b\x1d" => return Some("ctrl+alt+]".to_string()),
        "\x1b\x1f" => return Some("ctrl+alt+-".to_string()),
        "\t" => return Some("tab".to_string()),
        "\r" | "\x1bOM" => return Some("enter".to_string()),
        "\x00" => return Some("ctrl+space".to_string()),
        " " => return Some("space".to_string()),
        "\x7f" => return Some("backspace".to_string()),
        "\x08" => return Some("backspace".to_string()),
        "\x1b[Z" => return Some("shift+tab".to_string()),
        "\x1b\x7f" | "\x1b\x08" => return Some("alt+backspace".to_string()),
        "\x1b[A" => return Some("up".to_string()),
        "\x1b[B" => return Some("down".to_string()),
        "\x1b[C" => return Some("right".to_string()),
        "\x1b[D" => return Some("left".to_string()),
        "\x1b[H" | "\x1bOH" => return Some("home".to_string()),
        "\x1b[F" | "\x1bOF" => return Some("end".to_string()),
        "\x1b[[5~" => return Some("pageUp".to_string()),
        "\x1b[[6~" => return Some("pageDown".to_string()),
        "\x1b[3~" => return Some("delete".to_string()),
        "\x1b[5~" => return Some("pageUp".to_string()),
        "\x1b[6~" => return Some("pageDown".to_string()),
        "\x1b[2$" => return Some("shift+insert".to_string()),
        "\x1b[3$" => return Some("shift+delete".to_string()),
        "\x1b[5$" => return Some("shift+pageUp".to_string()),
        "\x1b[6$" => return Some("shift+pageDown".to_string()),
        "\x1b[7$" => return Some("shift+home".to_string()),
        "\x1b[8$" => return Some("shift+end".to_string()),
        "\x1b[2^" => return Some("ctrl+insert".to_string()),
        "\x1b[3^" => return Some("ctrl+delete".to_string()),
        "\x1b[5^" => return Some("ctrl+pageUp".to_string()),
        "\x1b[6^" => return Some("ctrl+pageDown".to_string()),
        "\x1b[7^" => return Some("ctrl+home".to_string()),
        "\x1b[8^" => return Some("ctrl+end".to_string()),
        _ => {}
    }

    if !is_kitty_protocol_active() {
        match data {
            "\n" => return Some("enter".to_string()),
            "\x1b\r" => return Some("alt+enter".to_string()),
            "\x1b " => return Some("alt+space".to_string()),
            "\x1bb" => return Some("alt+left".to_string()),
            "\x1bf" => return Some("alt+right".to_string()),
            "\x1bp" => return Some("alt+up".to_string()),
            "\x1bn" => return Some("alt+down".to_string()),
            "\x1bB" => return Some("alt+left".to_string()),
            "\x1bF" => return Some("alt+right".to_string()),
            _ => {}
        }
        let mut chars = data.chars();
        if chars.next() == Some('\x1b') {
            if let (Some(second), None) = (chars.next(), chars.next()) {
                let code = second as u32;
                if (1..=26).contains(&code) {
                    return char::from_u32(code + 96).map(|letter| format!("ctrl+alt+{letter}"));
                }
                if second.is_ascii_lowercase() || second.is_ascii_digit() {
                    return Some(format!("alt+{second}"));
                }
            }
        }
    }

    None
}

fn matches_direction(data: &str, key: &str, modifier: u8) -> bool {
    let (legacy, codepoint, suffix, shift_legacy, ctrl_legacy, alt_legacy) = match key {
        "up" => ("\x1b[A", -1, "A", "\x1b[a", "\x1bOa", "\x1bp"),
        "down" => ("\x1b[B", -2, "B", "\x1b[b", "\x1bOb", "\x1bn"),
        "right" => ("\x1b[C", -3, "C", "\x1b[c", "\x1bOc", "\x1bf"),
        "left" => ("\x1b[D", -4, "D", "\x1b[d", "\x1bOd", "\x1bb"),
        _ => return false,
    };
    if modifier == 0 {
        return data == legacy
            || data == legacy.replace("[", "O")
            || matches_kitty(data, codepoint, 0);
    }
    if modifier == MOD_SHIFT && data == shift_legacy {
        return true;
    }
    if modifier == MOD_CTRL && data == ctrl_legacy {
        return true;
    }
    if modifier == MOD_ALT && data == alt_legacy {
        return true;
    }
    data == format!("\x1b[1;{}{}", modifier + 1, suffix) || matches_kitty(data, codepoint, modifier)
}

fn matches_functional(data: &str, key: &str, modifier: u8) -> bool {
    let (number, codepoint, shift_legacy, ctrl_legacy) = match key {
        "insert" => (2, -11, "\x1b[2$", "\x1b[2^"),
        "delete" => (3, -10, "\x1b[3$", "\x1b[3^"),
        "pageUp" => (5, -12, "\x1b[5$", "\x1b[5^"),
        "pageDown" => (6, -13, "\x1b[6$", "\x1b[6^"),
        "home" => (1, -14, "\x1b[7$", "\x1b[7^"),
        "end" => (4, -15, "\x1b[8$", "\x1b[8^"),
        _ => return false,
    };
    if modifier == 0 {
        return data == format!("\x1b[{number}~")
            || (key == "home" && (data == "\x1b[H" || data == "\x1bOH" || data == "\x1b[7~"))
            || (key == "end" && (data == "\x1b[F" || data == "\x1bOF" || data == "\x1b[8~"))
            || matches_kitty(data, codepoint, 0);
    }
    if modifier == MOD_SHIFT && data == shift_legacy {
        return true;
    }
    if modifier == MOD_CTRL && data == ctrl_legacy {
        return true;
    }
    data == format!("\x1b[{number};{}~", modifier + 1) || matches_kitty(data, codepoint, modifier)
}

fn matches_function_key(data: &str, key: &str, modifier: u8) -> bool {
    let Some(number) = function_key_number(key) else {
        return false;
    };
    let legacy = match number {
        1 => &["\x1bOP", "\x1b[11~", "\x1b[[A"][..],
        2 => &["\x1bOQ", "\x1b[12~", "\x1b[[B"][..],
        3 => &["\x1bOR", "\x1b[13~", "\x1b[[C"][..],
        4 => &["\x1bOS", "\x1b[14~", "\x1b[[D"][..],
        5 => &["\x1b[15~", "\x1b[[E"][..],
        6 => &["\x1b[17~"][..],
        7 => &["\x1b[18~"][..],
        8 => &["\x1b[19~"][..],
        9 => &["\x1b[20~"][..],
        10 => &["\x1b[21~"][..],
        11 => &["\x1b[23~"][..],
        12 => &["\x1b[24~"][..],
        _ => &[],
    };
    modifier == 0 && legacy.contains(&data)
}

fn matches_legacy(data: &str, key: &str, modifier: u8) -> bool {
    match (key, modifier) {
        ("clear", 0) => data == "\x1b[E" || data == "\x1bOE",
        ("clear", MOD_SHIFT) => data == "\x1b[e",
        ("clear", MOD_CTRL) => data == "\x1bOe",
        _ => false,
    }
}

fn matches_kitty(data: &str, expected_codepoint: i32, expected_modifier: u8) -> bool {
    let Some(parsed) = parse_kitty_sequence(data) else {
        return false;
    };
    let actual_mod = parsed.modifier & !LOCK_MASK;
    let expected_mod = expected_modifier & !LOCK_MASK;
    if actual_mod != expected_mod {
        return false;
    }
    let normalized_codepoint = normalize_codepoint(parsed.codepoint, parsed.modifier);
    let normalized_expected = normalize_codepoint(expected_codepoint, expected_modifier);
    if normalized_codepoint == normalized_expected {
        return true;
    }
    if parsed.base_layout_key == Some(expected_codepoint) {
        return !is_latin_letter_codepoint(normalized_codepoint)
            && !is_known_symbol_codepoint(normalized_codepoint);
    }
    false
}

#[derive(Debug, Clone, Copy)]
struct KittySequence {
    codepoint: i32,
    base_layout_key: Option<i32>,
    modifier: u8,
}

fn parse_kitty_sequence(data: &str) -> Option<KittySequence> {
    if let Some(body) = data
        .strip_prefix("\x1b[")
        .and_then(|value| value.strip_suffix('u'))
    {
        let (codepoint_part, modifier_part) = body.split_once(';').unwrap_or((body, "1"));
        let mut codepoints = codepoint_part.split(':');
        let codepoint = codepoints
            .next()
            .and_then(|value| value.parse::<i32>().ok())?;
        let _shifted_key = codepoints.next();
        let base_layout_key = codepoints
            .next()
            .filter(|value| !value.is_empty())
            .and_then(|value| value.parse::<i32>().ok());
        let modifier = modifier_part
            .split(':')
            .next()
            .and_then(|value| value.parse::<u8>().ok())
            .unwrap_or(1)
            .saturating_sub(1);
        return Some(KittySequence {
            codepoint,
            base_layout_key,
            modifier,
        });
    }
    if let Some(body) = data.strip_prefix("\x1b[1;") {
        let suffix = body.chars().last()?;
        let codepoint = match suffix {
            'A' => -1,
            'B' => -2,
            'C' => -3,
            'D' => -4,
            'H' => -14,
            'F' => -15,
            _ => return None,
        };
        let modifier = body[..body.len() - 1]
            .split(':')
            .next()
            .and_then(|value| value.parse::<u8>().ok())?
            .saturating_sub(1);
        return Some(KittySequence {
            codepoint,
            base_layout_key: None,
            modifier,
        });
    }
    if let Some(body) = data
        .strip_prefix("\x1b[")
        .and_then(|value| value.strip_suffix('~'))
    {
        let (number, modifier) = body.split_once(';').unwrap_or((body, "1"));
        let codepoint = match number.parse::<i32>().ok()? {
            2 => -11,
            3 => -10,
            5 => -12,
            6 => -13,
            7 => -14,
            8 => -15,
            _ => return None,
        };
        return Some(KittySequence {
            codepoint,
            base_layout_key: None,
            modifier: modifier
                .split(':')
                .next()
                .and_then(|value| value.parse::<u8>().ok())
                .unwrap_or(1)
                .saturating_sub(1),
        });
    }
    None
}

#[derive(Debug, Clone, Copy)]
struct ModifyOtherKeysSequence {
    codepoint: i32,
    modifier: u8,
}

fn parse_modify_other_keys_sequence(data: &str) -> Option<ModifyOtherKeysSequence> {
    let body = data
        .strip_prefix("\x1b[27;")
        .and_then(|value| value.strip_suffix('~'))?;
    let (modifier, codepoint) = body.split_once(';')?;
    Some(ModifyOtherKeysSequence {
        codepoint: codepoint.parse::<i32>().ok()?,
        modifier: modifier.parse::<u8>().ok()?.saturating_sub(1),
    })
}

fn matches_modify_other_keys(data: &str, expected_codepoint: i32, expected_modifier: u8) -> bool {
    let Some(parsed) = parse_modify_other_keys_sequence(data) else {
        return false;
    };
    parsed.codepoint == expected_codepoint && parsed.modifier == expected_modifier
}

fn format_kitty_key(parsed: KittySequence) -> Option<String> {
    format_key_name_for_codepoint(parsed.codepoint, parsed.modifier).or_else(|| {
        parsed
            .base_layout_key
            .and_then(|codepoint| format_key_name_for_codepoint(codepoint, parsed.modifier))
    })
}

fn format_key_name_for_codepoint(codepoint: i32, modifier: u8) -> Option<String> {
    let normalized = normalize_codepoint(codepoint, modifier);
    let effective_codepoint = if is_latin_letter_codepoint(normalized)
        || (48..=57).contains(&normalized)
        || is_known_symbol_codepoint(normalized)
    {
        normalized
    } else {
        normalized
    };

    let key_name = key_name_for_codepoint(effective_codepoint)?;
    Some(format_key_name_with_modifiers(key_name, modifier))
}

fn key_name_for_codepoint(codepoint: i32) -> Option<String> {
    let key = match codepoint {
        27 => "escape",
        9 => "tab",
        13 | 57414 => "enter",
        32 => "space",
        127 => "backspace",
        -10 => "delete",
        -11 => "insert",
        -12 => "pageUp",
        -13 => "pageDown",
        -14 => "home",
        -15 => "end",
        -1 => "up",
        -2 => "down",
        -3 => "right",
        -4 => "left",
        48..=57 | 97..=122 => return char::from_u32(codepoint as u32).map(|ch| ch.to_string()),
        _ if is_known_symbol_codepoint(codepoint) => {
            return char::from_u32(codepoint as u32).map(|ch| ch.to_string());
        }
        _ => return None,
    };
    Some(key.to_string())
}

fn format_key_name_with_modifiers(key_name: String, modifier: u8) -> String {
    let modifier = modifier & !LOCK_MASK;
    let mut parts = Vec::new();
    if modifier & MOD_SHIFT != 0 {
        parts.push("shift");
    }
    if modifier & MOD_CTRL != 0 {
        parts.push("ctrl");
    }
    if modifier & MOD_ALT != 0 {
        parts.push("alt");
    }
    if modifier & MOD_SUPER != 0 {
        parts.push("super");
    }
    if parts.is_empty() {
        key_name
    } else {
        format!("{}+{key_name}", parts.join("+"))
    }
}

fn is_latin_letter_codepoint(codepoint: i32) -> bool {
    (97..=122).contains(&codepoint)
}

fn is_known_symbol_codepoint(codepoint: i32) -> bool {
    matches!(
        codepoint,
        33 | 34
            | 35
            | 36
            | 37
            | 38
            | 39
            | 40
            | 41
            | 42
            | 43
            | 44
            | 45
            | 46
            | 47
            | 58
            | 59
            | 60
            | 61
            | 62
            | 63
            | 64
            | 91
            | 92
            | 93
            | 94
            | 95
            | 96
            | 123
            | 124
            | 125
            | 126
    )
}

fn decode_modify_other_keys_printable(data: &str) -> Option<String> {
    let Some(body) = data
        .strip_prefix("\x1b[27;")
        .and_then(|value| value.strip_suffix('~'))
    else {
        return None;
    };
    let Some((modifier, codepoint)) = body.split_once(';') else {
        return None;
    };
    let modifier = modifier.parse::<u8>().ok()?.saturating_sub(1) & !LOCK_MASK;
    if modifier & !MOD_SHIFT != 0 {
        return None;
    }
    let codepoint = codepoint.parse::<u32>().ok()?;
    if codepoint < 32 {
        return None;
    }
    char::from_u32(codepoint).map(|ch| ch.to_string())
}

fn normalize_codepoint(codepoint: i32, modifier: u8) -> i32 {
    if modifier & MOD_SHIFT != 0 && (65..=90).contains(&codepoint) {
        return codepoint + 32;
    }
    match codepoint {
        57399..=57408 => codepoint - 57351,
        57409 => 46,
        57410 => 47,
        57411 => 42,
        57412 => 45,
        57413 => 43,
        57415 => 61,
        57416 => 44,
        57417 => -4,
        57418 => -3,
        57419 => -1,
        57420 => -2,
        57421 => -12,
        57422 => -13,
        57423 => -14,
        57424 => -15,
        57425 => -11,
        57426 => -10,
        _ => codepoint,
    }
}

fn raw_ctrl_char(key: char) -> Option<char> {
    let lower = key.to_ascii_lowercase();
    if lower.is_ascii_lowercase() || matches!(lower, '[' | '\\' | ']' | '_') {
        return char::from_u32((lower as u32) & 0x1f);
    }
    if lower == '-' {
        return Some(char::from_u32(31)?);
    }
    None
}

fn function_key_number(key: &str) -> Option<u8> {
    let number = key.strip_prefix('f')?.parse::<u8>().ok()?;
    (1..=12).contains(&number).then_some(number)
}

fn canonical_functional_key(key: &str) -> &str {
    match key {
        "pageup" => "pageUp",
        "pagedown" => "pageDown",
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_helper_exposes_pi_special_and_symbol_key_ids() {
        assert_eq!(Key::escape(), "escape");
        assert_eq!(Key::page_up(), "pageUp");
        assert_eq!(Key::left_brace(), "{");
        assert_eq!(Key::question(), "?");
    }

    #[test]
    fn key_helper_builds_pi_modifier_key_ids_in_declared_order() {
        assert_eq!(Key::ctrl("c"), "ctrl+c");
        assert_eq!(Key::shift("enter"), "shift+enter");
        assert_eq!(Key::ctrl_shift("p"), "ctrl+shift+p");
        assert_eq!(Key::shift_ctrl("p"), "shift+ctrl+p");
        assert_eq!(Key::alt_super("k"), "alt+super+k");
        assert_eq!(Key::ctrl_shift_alt("x"), "ctrl+shift+alt+x");
        assert_eq!(Key::ctrl_shift_super("k"), "ctrl+shift+super+k");
    }

    #[test]
    fn matches_common_legacy_keys() {
        assert!(matches_key("\x1b[A", "up"));
        assert!(matches_key("\r", "enter"));
        assert!(matches_key("\x7f", "backspace"));
        assert!(matches_key("\x03", "ctrl+c"));
    }

    #[test]
    fn parses_raw_ctrl_alt_and_shift_legacy_keys_like_pi() {
        set_kitty_protocol_active(false);
        assert_eq!(parse_key("\x03"), Some("ctrl+c".to_string()));
        assert_eq!(parse_key("\x1b\x03"), Some("ctrl+alt+c".to_string()));
        assert_eq!(parse_key("\x1ba"), Some("alt+a".to_string()));
        assert_eq!(parse_key("\x1b5"), Some("alt+5".to_string()));
        assert_eq!(parse_key("\x1b[Z"), Some("shift+tab".to_string()));
        assert_eq!(parse_key("\x1b\r"), Some("alt+enter".to_string()));
        assert_eq!(parse_key("\x1b "), Some("alt+space".to_string()));
    }

    #[test]
    fn parses_extended_legacy_sequence_key_ids_like_pi() {
        set_kitty_protocol_active(false);

        for (data, expected) in [
            ("\x1b[[5~", "pageUp"),
            ("\x1b[[6~", "pageDown"),
            ("\x1b[2$", "shift+insert"),
            ("\x1b[3$", "shift+delete"),
            ("\x1b[7$", "shift+home"),
            ("\x1b[8$", "shift+end"),
            ("\x1b[2^", "ctrl+insert"),
            ("\x1b[3^", "ctrl+delete"),
            ("\x1b[7^", "ctrl+home"),
            ("\x1b[8^", "ctrl+end"),
            ("\x1bb", "alt+left"),
            ("\x1bf", "alt+right"),
            ("\x1bp", "alt+up"),
            ("\x1bn", "alt+down"),
        ] {
            assert_eq!(parse_key(data), Some(expected.to_string()), "{data:?}");
        }
    }

    #[test]
    fn matches_extended_legacy_sequence_key_ids_like_pi() {
        set_kitty_protocol_active(false);

        for (data, key_id) in [
            ("\x1b[2$", "shift+insert"),
            ("\x1b[3$", "shift+delete"),
            ("\x1b[5$", "shift+pageUp"),
            ("\x1b[6$", "shift+pageDown"),
            ("\x1b[7$", "shift+home"),
            ("\x1b[8$", "shift+end"),
            ("\x1b[2^", "ctrl+insert"),
            ("\x1b[3^", "ctrl+delete"),
            ("\x1b[5^", "ctrl+pageUp"),
            ("\x1b[6^", "ctrl+pageDown"),
            ("\x1b[7^", "ctrl+home"),
            ("\x1b[8^", "ctrl+end"),
            ("\x1b[a", "shift+up"),
            ("\x1b[b", "shift+down"),
            ("\x1b[c", "shift+right"),
            ("\x1b[d", "shift+left"),
            ("\x1bOa", "ctrl+up"),
            ("\x1bOb", "ctrl+down"),
            ("\x1bOc", "ctrl+right"),
            ("\x1bOd", "ctrl+left"),
            ("\x1bp", "alt+up"),
            ("\x1bn", "alt+down"),
            ("\x1bb", "alt+left"),
            ("\x1bf", "alt+right"),
        ] {
            assert!(matches_key(data, key_id), "{data:?} should match {key_id}");
        }
    }

    #[test]
    fn matches_csi_u_and_modify_other_keys() {
        assert!(matches_key("\x1b[99;5u", "ctrl+c"));
        assert!(matches_key("\x1b[27;5;99~", "ctrl+c"));
        assert!(matches_key("\x1b[1;6A", "ctrl+shift+up"));
    }

    #[test]
    fn parses_modify_other_keys_sequences_like_pi() {
        assert_eq!(parse_key("\x1b[27;5;99~"), Some("ctrl+c".to_string()));
        assert_eq!(parse_key("\x1b[27;6;65~"), Some("shift+ctrl+a".to_string()));
        assert_eq!(parse_key("\x1b[27;3;13~"), Some("alt+enter".to_string()));
    }

    #[test]
    fn matches_kitty_base_layout_key_for_non_latin_layout_like_pi() {
        assert!(matches_key("\x1b[1089::99;5u", "ctrl+c"));
        assert_eq!(parse_key("\x1b[1089::99;5u"), Some("ctrl+c".to_string()));

        assert!(!matches_key("\x1b[107::118;5u", "ctrl+v"));
        assert_eq!(parse_key("\x1b[107::118;5u"), Some("ctrl+k".to_string()));
    }

    #[test]
    fn detects_kitty_event_types() {
        assert!(is_key_repeat("\x1b[65;1:2u"));
        assert!(is_key_release("\x1b[65;1:3u"));
        assert!(!is_key_release("\x1b[200~:3u\x1b[201~"));
    }

    #[test]
    fn decodes_kitty_printable_input() {
        assert_eq!(decode_kitty_printable("\x1b[97u"), Some("a".to_string()));
        assert_eq!(
            decode_kitty_printable("\x1b[97:65;2u"),
            Some("A".to_string())
        );
        assert_eq!(decode_kitty_printable("\x1b[57399u"), Some("0".to_string()));
        assert_eq!(decode_kitty_printable("\x1b[57413u"), Some("+".to_string()));
        assert_eq!(decode_kitty_printable("\x1b[99;5u"), None);
    }

    #[test]
    fn decodes_printable_key_like_pi_public_entrypoint() {
        assert_eq!(decode_printable_key("\x1b[97u"), Some("a".to_string()));
        assert_eq!(decode_printable_key("\x1b[97:65;2u"), Some("A".to_string()));
        assert_eq!(decode_printable_key("\x1b[27;2;65~"), Some("A".to_string()));
        assert_eq!(decode_printable_key("a"), None);
        assert_eq!(decode_printable_key("\x1b[27;5;99~"), None);
    }
}
