use serde_yaml::Value;

#[derive(Debug, Clone, PartialEq)]
pub struct ParsedFrontmatter {
    pub frontmatter: Value,
    pub body: String,
}

pub fn parse_frontmatter(content: &str) -> Result<ParsedFrontmatter, serde_yaml::Error> {
    let (yaml, body) = extract_frontmatter(content);
    let frontmatter = match yaml {
        Some(yaml) if !yaml.trim().is_empty() => {
            normalize_frontmatter_value(&yaml, serde_yaml::from_str(&yaml)?)
        }
        _ => Value::Mapping(Default::default()),
    };

    Ok(ParsedFrontmatter { frontmatter, body })
}

pub fn strip_frontmatter(content: &str) -> Result<String, serde_yaml::Error> {
    parse_frontmatter(content).map(|parsed| parsed.body)
}

fn extract_frontmatter(content: &str) -> (Option<String>, String) {
    let normalized = normalize_newlines(content);
    if !normalized.starts_with("---") {
        return (None, normalized);
    }

    let Some(end_index) = normalized[3..].find("\n---").map(|index| index + 3) else {
        return (None, normalized);
    };

    let yaml = normalized[4..end_index].to_string();
    let body = normalized[end_index + 4..].trim().to_string();
    (Some(yaml), body)
}

fn normalize_newlines(content: &str) -> String {
    content.replace("\r\n", "\n").replace('\r', "\n")
}

fn normalize_frontmatter_value(yaml: &str, mut value: Value) -> Value {
    if value.is_null() {
        return Value::Mapping(Default::default());
    }
    let literal_keys = literal_block_keys(yaml);
    if literal_keys.is_empty() {
        return value;
    }
    let Some(mapping) = value.as_mapping_mut() else {
        return value;
    };
    for key in literal_keys {
        let key_value = Value::String(key);
        let Some(Value::String(text)) = mapping.get_mut(&key_value) else {
            continue;
        };
        if !text.ends_with('\n') {
            text.push('\n');
        }
    }
    value
}

fn literal_block_keys(yaml: &str) -> Vec<String> {
    yaml.lines()
        .filter_map(|line| {
            let trimmed = line.trim_start();
            if trimmed.starts_with('#') || line.len() != trimmed.len() {
                return None;
            }
            let (key, marker) = trimmed.split_once(':')?;
            let marker = marker.trim_start();
            (marker == "|" || marker.starts_with("| ")).then(|| key.trim().to_string())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_yaml_frontmatter_and_body() {
        let parsed =
            parse_frontmatter("---\ntitle: Test\ncount: 2\n---\n\nBody").expect("frontmatter");
        assert_eq!(
            parsed.frontmatter["title"],
            Value::String("Test".to_string())
        );
        assert_eq!(parsed.frontmatter["count"], Value::Number(2.into()));
        assert_eq!(parsed.body, "Body");
    }

    #[test]
    fn returns_empty_frontmatter_when_missing() {
        let parsed = parse_frontmatter("Body").expect("frontmatter");
        assert!(parsed
            .frontmatter
            .as_mapping()
            .is_some_and(|map| map.is_empty()));
        assert_eq!(parsed.body, "Body");
    }

    #[test]
    fn strips_frontmatter() {
        assert_eq!(
            strip_frontmatter("---\na: 1\n---\nContent").expect("frontmatter"),
            "Content"
        );
    }

    #[test]
    fn parses_multiline_and_crlf_frontmatter_like_pi() {
        let parsed = parse_frontmatter(
            "---\r\ndescription: |\r\n  Line one\r\n  Line two\r\n---\r\nBody\r\n",
        )
        .expect("frontmatter");

        assert_eq!(
            parsed.frontmatter["description"],
            Value::String("Line one\nLine two\n".to_string())
        );
        assert_eq!(parsed.body, "Body");
    }

    #[test]
    fn returns_empty_frontmatter_for_comments_and_missing_terminator_like_pi() {
        let comments = parse_frontmatter("---\n# just a comment\n---\nBody").expect("frontmatter");
        assert!(comments
            .frontmatter
            .as_mapping()
            .is_some_and(|map| map.is_empty()));
        assert_eq!(comments.body, "Body");

        let missing =
            parse_frontmatter("---\nname: test\nBody without terminator").expect("frontmatter");
        assert!(missing
            .frontmatter
            .as_mapping()
            .is_some_and(|map| map.is_empty()));
        assert_eq!(missing.body, "---\nname: test\nBody without terminator");
    }
}
