use serde_yaml::Value;

#[derive(Debug, Clone, PartialEq)]
pub struct ParsedFrontmatter {
    pub frontmatter: Value,
    pub body: String,
}

pub fn parse_frontmatter(content: &str) -> Result<ParsedFrontmatter, serde_yaml::Error> {
    let (yaml, body) = extract_frontmatter(content);
    let frontmatter = match yaml {
        Some(yaml) if !yaml.trim().is_empty() => serde_yaml::from_str(&yaml)?,
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
}
