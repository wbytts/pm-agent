#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedHtmlEntity {
    pub text: String,
    pub length: usize,
}

pub fn decode_html_entity(entity: &str) -> Option<String> {
    match entity {
        "amp" => Some("&".to_string()),
        "lt" => Some("<".to_string()),
        "gt" => Some(">".to_string()),
        "quot" => Some("\"".to_string()),
        "apos" => Some("'".to_string()),
        _ if entity.starts_with("#x") || entity.starts_with("#X") => {
            decode_code_point(u32::from_str_radix(&entity[2..], 16).ok()?)
        }
        _ if entity.starts_with('#') => decode_code_point(entity[1..].parse::<u32>().ok()?),
        _ => None,
    }
}

pub fn decode_html_entity_at(html: &str, index: usize) -> Option<DecodedHtmlEntity> {
    if index >= html.len() || !html.is_char_boundary(index) || html.as_bytes()[index] != b'&' {
        return None;
    }

    let tail = &html[index + 1..];
    let semicolon_offset = tail.find(';')?;
    if semicolon_offset + 1 > 16 {
        return None;
    }

    let entity = &tail[..semicolon_offset];
    let decoded = decode_html_entity(entity)?;
    Some(DecodedHtmlEntity {
        text: decoded,
        length: semicolon_offset + 2,
    })
}

fn decode_code_point(code_point: u32) -> Option<String> {
    char::from_u32(code_point).map(|ch| ch.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_named_and_numeric_entities() {
        assert_eq!(decode_html_entity("amp").as_deref(), Some("&"));
        assert_eq!(decode_html_entity("#x1f642").as_deref(), Some("🙂"));
        assert_eq!(decode_html_entity("#169").as_deref(), Some("©"));
    }

    #[test]
    fn decodes_entity_at_index() {
        let decoded = decode_html_entity_at("a&amp;b", 1).expect("entity");
        assert_eq!(decoded.text, "&");
        assert_eq!(decoded.length, 5);
        assert!(decode_html_entity_at("a&amp;b", 0).is_none());
    }
}
