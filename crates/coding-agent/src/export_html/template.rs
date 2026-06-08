use crate::export_html::theme::ExportTheme;

const TEMPLATE_HTML: &str = include_str!("assets/template.html");
const TEMPLATE_CSS: &str = include_str!("assets/template.css");
const TEMPLATE_JS: &str = include_str!("assets/template.js");

pub fn render_template(session_json: &str, theme: &ExportTheme) -> String {
    let css = TEMPLATE_CSS
        .replace("{{THEME_VARS}}", &theme.css_vars())
        .replace("{{THEME_NAME}}", &theme.name);

    TEMPLATE_HTML
        .replace("{{CSS}}", &css)
        .replace("{{JS}}", TEMPLATE_JS)
        .replace("{{SESSION_DATA}}", &escape_script_json(session_json))
}

fn escape_script_json(json: &str) -> String {
    json.replace('&', "\\u0026")
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embeds_json_without_breaking_script_tag() {
        let html = render_template(r#"{"text":"</script><div>&"}"#, &ExportTheme::resolve(None));

        assert!(html.contains("\\u003c/script\\u003e"));
        assert!(!html.contains("</script><div>"));
    }
}
