/// 去掉 JSON 行注释和尾逗号，保持字符串字面量内部内容不变。
pub(super) fn strip_json_comments(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    let mut in_string = false;
    let mut escaped = false;
    while let Some(ch) = chars.next() {
        if in_string {
            output.push(ch);
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }
        if ch == '"' {
            in_string = true;
            output.push(ch);
            continue;
        }
        if ch == '/' && chars.peek() == Some(&'/') {
            for next in chars.by_ref() {
                if next == '\n' {
                    output.push('\n');
                    break;
                }
            }
            continue;
        }
        output.push(ch);
    }
    remove_trailing_commas(&output)
}

fn remove_trailing_commas(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let chars = input.chars().collect::<Vec<_>>();
    let mut index = 0;
    let mut in_string = false;
    let mut escaped = false;
    while index < chars.len() {
        let ch = chars[index];
        if in_string {
            output.push(ch);
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            index += 1;
            continue;
        }
        if ch == '"' {
            in_string = true;
            output.push(ch);
            index += 1;
            continue;
        }
        if ch == ',' {
            let mut lookahead = index + 1;
            while chars
                .get(lookahead)
                .is_some_and(|next| next.is_whitespace())
            {
                lookahead += 1;
            }
            if matches!(chars.get(lookahead), Some('}' | ']')) {
                index += 1;
                continue;
            }
        }
        output.push(ch);
        index += 1;
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_comments_and_trailing_commas() {
        let json = strip_json_comments(
            r#"{"a": "http://x", // comment
          "b": [1,],
        }"#,
        );
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("json should parse");
        assert_eq!(parsed["a"], "http://x");
    }
}
