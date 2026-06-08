use serde::de::DeserializeOwned;
use serde_json::Value;

const VALID_ESCAPES: &[char] = &['"', '\\', '/', 'b', 'f', 'n', 'r', 't', 'u'];

pub fn repair_json(json: &str) -> String {
    let mut repaired = String::new();
    let mut in_string = false;
    let chars = json.chars().collect::<Vec<_>>();
    let mut index = 0;

    while index < chars.len() {
        let ch = chars[index];
        if !in_string {
            repaired.push(ch);
            if ch == '"' {
                in_string = true;
            }
            index += 1;
            continue;
        }

        if ch == '"' {
            repaired.push(ch);
            in_string = false;
            index += 1;
            continue;
        }

        if ch == '\\' {
            let Some(next) = chars.get(index + 1).copied() else {
                repaired.push_str("\\\\");
                index += 1;
                continue;
            };

            if next == 'u' {
                let unicode_digits = chars
                    .get(index + 2..index + 6)
                    .unwrap_or_default()
                    .iter()
                    .collect::<String>();
                if unicode_digits.len() == 4
                    && unicode_digits
                        .chars()
                        .all(|digit| digit.is_ascii_hexdigit())
                {
                    repaired.push_str("\\u");
                    repaired.push_str(&unicode_digits);
                    index += 6;
                    continue;
                }
            }

            if VALID_ESCAPES.contains(&next) {
                repaired.push('\\');
                repaired.push(next);
                index += 2;
                continue;
            }

            repaired.push_str("\\\\");
            index += 1;
            continue;
        }

        if is_control_character(ch) {
            repaired.push_str(&escape_control_character(ch));
        } else {
            repaired.push(ch);
        }
        index += 1;
    }

    repaired
}

pub fn parse_json_with_repair<T: DeserializeOwned>(json: &str) -> serde_json::Result<T> {
    match serde_json::from_str(json) {
        Ok(value) => Ok(value),
        Err(original_error) => {
            let repaired = repair_json(json);
            if repaired != json {
                serde_json::from_str(&repaired)
            } else {
                Err(original_error)
            }
        }
    }
}

pub fn parse_streaming_json(partial_json: Option<&str>) -> Value {
    let Some(partial_json) = partial_json else {
        return Value::Object(Default::default());
    };
    if partial_json.trim().is_empty() {
        return Value::Object(Default::default());
    }
    if let Ok(value) = parse_json_with_repair::<Value>(partial_json) {
        return value;
    }
    let repaired = repair_json(partial_json);
    parse_partial_json(&repaired).unwrap_or_else(|| Value::Object(Default::default()))
}

fn parse_partial_json(json: &str) -> Option<Value> {
    let mut candidate = json.trim().to_string();
    if candidate.is_empty() {
        return None;
    }
    if let Ok(value) = serde_json::from_str(&candidate) {
        return Some(value);
    }

    close_open_string(&mut candidate);
    trim_incomplete_suffix(&mut candidate);
    let (objects, arrays) = count_unclosed_containers(&candidate);
    candidate.push_str(&"]".repeat(arrays));
    candidate.push_str(&"}".repeat(objects));
    serde_json::from_str(&candidate).ok()
}

fn close_open_string(candidate: &mut String) {
    let mut in_string = false;
    let mut escaped = false;
    for ch in candidate.chars() {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        if ch == '"' {
            in_string = !in_string;
        }
    }
    if in_string {
        candidate.push('"');
    }
}

fn trim_incomplete_suffix(candidate: &mut String) {
    while matches!(candidate.chars().last(), Some(',' | ':' | '[' | '{')) {
        candidate.pop();
    }
}

fn count_unclosed_containers(candidate: &str) -> (usize, usize) {
    let mut objects = 0usize;
    let mut arrays = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    for ch in candidate.chars() {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        if ch == '"' {
            in_string = !in_string;
            continue;
        }
        if in_string {
            continue;
        }
        match ch {
            '{' => objects += 1,
            '}' => objects = objects.saturating_sub(1),
            '[' => arrays += 1,
            ']' => arrays = arrays.saturating_sub(1),
            _ => {}
        }
    }
    (objects, arrays)
}

fn is_control_character(ch: char) -> bool {
    (ch as u32) <= 0x1f
}

fn escape_control_character(ch: char) -> String {
    match ch {
        '\u{0008}' => "\\b".to_string(),
        '\u{000c}' => "\\f".to_string(),
        '\n' => "\\n".to_string(),
        '\r' => "\\r".to_string(),
        '\t' => "\\t".to_string(),
        _ => format!("\\u{:04x}", ch as u32),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn repairs_control_characters_and_invalid_escapes() {
        assert_eq!(repair_json("{\"a\":\"x\ny\"}"), "{\"a\":\"x\\ny\"}");
        assert_eq!(
            repair_json("{\"path\":\"c:\\qtmp\"}"),
            "{\"path\":\"c:\\\\qtmp\"}"
        );
        let value: Value = parse_json_with_repair("{\"a\":\"x\ny\"}").expect("parse repaired json");
        assert_eq!(value, json!({"a": "x\ny"}));
    }

    #[test]
    fn parses_streaming_json_best_effort() {
        assert_eq!(parse_streaming_json(None), json!({}));
        assert_eq!(parse_streaming_json(Some("{\"a\":1")), json!({"a": 1}));
        assert_eq!(
            parse_streaming_json(Some("{\"a\":\"unterminated")),
            json!({"a": "unterminated"})
        );
    }
}
