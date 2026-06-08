use std::collections::HashMap;

use crate::utils::html::decode_html_entity_at;

pub type HighlightFormatter = fn(&str) -> String;
pub type HighlightTheme = HashMap<String, HighlightFormatter>;

const SPAN_CLOSE: &str = "</span>";
const HIGHLIGHT_CLASS_PREFIX: &str = "hljs-";

pub fn render_highlighted_html(html: &str, theme: &HighlightTheme) -> String {
    let mut output = String::new();
    let mut text_buffer = String::new();
    let mut scopes: Vec<Option<String>> = Vec::new();
    let mut index = 0usize;

    while index < html.len() {
        if is_span_open_tag_start(html, index) {
            if let Some(relative_tag_end) = html[index + 5..].find('>') {
                flush_text(&mut output, &mut text_buffer, &scopes, theme);
                let tag_end = index + 5 + relative_tag_end;
                let tag = &html[index..=tag_end];
                scopes.push(get_scope_from_span_tag(tag));
                index = tag_end + 1;
                continue;
            }
        }

        if html[index..].starts_with(SPAN_CLOSE) {
            flush_text(&mut output, &mut text_buffer, &scopes, theme);
            scopes.pop();
            index += SPAN_CLOSE.len();
            continue;
        }

        if html[index..].starts_with('&') {
            if let Some(decoded) = decode_html_entity_at(html, index) {
                text_buffer.push_str(&decoded.text);
                index += decoded.length;
                continue;
            }
        }

        let Some(ch) = html[index..].chars().next() else {
            break;
        };
        text_buffer.push(ch);
        index += ch.len_utf8();
    }

    flush_text(&mut output, &mut text_buffer, &scopes, theme);
    output
}

fn flush_text(
    output: &mut String,
    text_buffer: &mut String,
    scopes: &[Option<String>],
    theme: &HighlightTheme,
) {
    if text_buffer.is_empty() {
        return;
    }
    if let Some(formatter) = get_active_formatter(scopes, theme) {
        output.push_str(&formatter(text_buffer));
    } else {
        output.push_str(text_buffer);
    }
    text_buffer.clear();
}

fn get_scope_from_span_tag(tag: &str) -> Option<String> {
    let class_value = class_attribute_value(tag)?;
    for class_name in class_value.split_whitespace() {
        if let Some(scope) = class_name.strip_prefix(HIGHLIGHT_CLASS_PREFIX) {
            return Some(scope.to_string());
        }
    }
    None
}

fn class_attribute_value(tag: &str) -> Option<String> {
    let class_index = tag.find("class")?;
    let after_class = &tag[class_index + "class".len()..];
    let equals_index = after_class.find('=')?;
    let after_equals = after_class[equals_index + 1..].trim_start();
    let quote = after_equals.chars().next()?;
    if quote != '"' && quote != '\'' {
        return None;
    }
    let value_start = quote.len_utf8();
    let value_end = after_equals[value_start..].find(quote)?;
    Some(after_equals[value_start..value_start + value_end].to_string())
}

fn get_active_formatter(
    scopes: &[Option<String>],
    theme: &HighlightTheme,
) -> Option<HighlightFormatter> {
    for scope in scopes.iter().rev().flatten() {
        if let Some(formatter) = get_scope_formatter(scope, theme) {
            return Some(formatter);
        }
    }
    theme.get("default").copied()
}

fn get_scope_formatter(scope: &str, theme: &HighlightTheme) -> Option<HighlightFormatter> {
    if let Some(formatter) = theme.get(scope) {
        return Some(*formatter);
    }
    if let Some(dot_index) = scope.find('.') {
        if let Some(formatter) = theme.get(&scope[..dot_index]) {
            return Some(*formatter);
        }
    }
    if let Some(dash_index) = scope.find('-') {
        if let Some(formatter) = theme.get(&scope[..dash_index]) {
            return Some(*formatter);
        }
    }
    None
}

fn is_span_open_tag_start(html: &str, index: usize) -> bool {
    if !html[index..].starts_with("<span") {
        return false;
    }
    let next_index = index + "<span".len();
    matches!(
        html[next_index..].chars().next(),
        Some('>' | ' ' | '\t' | '\n' | '\r')
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wrap_keyword(text: &str) -> String {
        format!("<kw>{text}</kw>")
    }

    fn wrap_title(text: &str) -> String {
        format!("<title>{text}</title>")
    }

    fn wrap_default(text: &str) -> String {
        format!("<default>{text}</default>")
    }

    fn theme(entries: &[(&str, HighlightFormatter)]) -> HighlightTheme {
        entries
            .iter()
            .map(|(key, formatter)| ((*key).to_string(), *formatter))
            .collect()
    }

    #[test]
    fn renders_span_scopes_with_theme_like_pi_syntax_highlight() {
        let theme = theme(&[("keyword", wrap_keyword)]);

        assert_eq!(
            render_highlighted_html(
                r#"let <span class="hljs-keyword">async</span> value"#,
                &theme
            ),
            "let <kw>async</kw> value"
        );
    }

    #[test]
    fn uses_nearest_nested_scope_and_scope_prefixes_like_pi_syntax_highlight() {
        let theme = theme(&[("title", wrap_title), ("keyword", wrap_keyword)]);

        assert_eq!(
            render_highlighted_html(
                r#"<span class="hljs-keyword">fn <span class='hljs-title.function_'>run</span></span>"#,
                &theme
            ),
            "<kw>fn </kw><title>run</title>"
        );
    }

    #[test]
    fn decodes_html_entities_and_uses_default_formatter_like_pi_syntax_highlight() {
        let theme = theme(&[("default", wrap_default)]);

        assert_eq!(
            render_highlighted_html("&lt;tag attr=&quot;v&quot;&gt;&amp;#x41;", &theme),
            "<default><tag attr=\"v\">&#x41;</default>"
        );
    }

    #[test]
    fn preserves_non_span_tags_and_matches_dash_prefix_like_pi_syntax_highlight() {
        let theme = theme(&[("title", wrap_title)]);

        assert_eq!(
            render_highlighted_html(
                r#"<em><span class="hljs-title-function">run</span></em>"#,
                &theme
            ),
            "<em><title>run</title></em>"
        );
    }
}
