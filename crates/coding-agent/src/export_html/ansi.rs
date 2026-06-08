#[derive(Debug, Clone, PartialEq, Eq)]
struct TextStyle {
    fg: Option<String>,
    bg: Option<String>,
    bold: bool,
    dim: bool,
    italic: bool,
    underline: bool,
}

const ANSI_COLORS: [&str; 16] = [
    "#000000", "#800000", "#008000", "#808000", "#000080", "#800080", "#008080", "#c0c0c0",
    "#808080", "#ff0000", "#00ff00", "#ffff00", "#0000ff", "#ff00ff", "#00ffff", "#ffffff",
];

pub fn ansi_to_html(text: &str) -> String {
    let mut style = TextStyle::empty();
    let mut output = String::new();
    let mut index = 0;
    let mut open_span = false;

    while let Some(relative_start) = text[index..].find("\x1b[") {
        let start = index + relative_start;
        output.push_str(&escape_html(&text[index..start]));

        let Some(relative_end) = text[start..].find('m') else {
            output.push_str(&escape_html(&text[start..]));
            return close_span(output, open_span);
        };

        let end = start + relative_end;
        let params = parse_sgr_params(&text[start + 2..end]);
        if open_span {
            output.push_str("</span>");
            open_span = false;
        }

        apply_sgr_codes(&params, &mut style);
        if style.has_style() {
            output.push_str(&format!("<span style=\"{}\">", style.to_inline_css()));
            open_span = true;
        }
        index = end + 1;
    }

    output.push_str(&escape_html(&text[index..]));
    close_span(output, open_span)
}

pub fn ansi_lines_to_html(lines: &[String]) -> String {
    lines
        .iter()
        .map(|line| {
            let html = ansi_to_html(line);
            if html.is_empty() {
                "<div class=\"ansi-line\">&nbsp;</div>".to_string()
            } else {
                format!("<div class=\"ansi-line\">{html}</div>")
            }
        })
        .collect::<Vec<_>>()
        .join("")
}

impl TextStyle {
    fn empty() -> Self {
        Self {
            fg: None,
            bg: None,
            bold: false,
            dim: false,
            italic: false,
            underline: false,
        }
    }

    fn reset(&mut self) {
        *self = Self::empty();
    }

    fn has_style(&self) -> bool {
        self.fg.is_some()
            || self.bg.is_some()
            || self.bold
            || self.dim
            || self.italic
            || self.underline
    }

    fn to_inline_css(&self) -> String {
        let mut parts = Vec::new();
        if let Some(fg) = &self.fg {
            parts.push(format!("color:{fg}"));
        }
        if let Some(bg) = &self.bg {
            parts.push(format!("background-color:{bg}"));
        }
        if self.bold {
            parts.push("font-weight:bold".to_string());
        }
        if self.dim {
            parts.push("opacity:0.6".to_string());
        }
        if self.italic {
            parts.push("font-style:italic".to_string());
        }
        if self.underline {
            parts.push("text-decoration:underline".to_string());
        }
        parts.join(";")
    }
}

fn parse_sgr_params(params: &str) -> Vec<u16> {
    if params.is_empty() {
        return vec![0];
    }
    params
        .split(';')
        .map(|value| value.parse::<u16>().unwrap_or(0))
        .collect()
}

fn apply_sgr_codes(params: &[u16], style: &mut TextStyle) {
    let mut index = 0;
    while index < params.len() {
        let code = params[index];
        match code {
            0 => style.reset(),
            1 => style.bold = true,
            2 => style.dim = true,
            3 => style.italic = true,
            4 => style.underline = true,
            22 => {
                style.bold = false;
                style.dim = false;
            }
            23 => style.italic = false,
            24 => style.underline = false,
            30..=37 => style.fg = Some(ANSI_COLORS[(code - 30) as usize].to_string()),
            39 => style.fg = None,
            40..=47 => style.bg = Some(ANSI_COLORS[(code - 40) as usize].to_string()),
            49 => style.bg = None,
            90..=97 => style.fg = Some(ANSI_COLORS[(code - 90 + 8) as usize].to_string()),
            100..=107 => style.bg = Some(ANSI_COLORS[(code - 100 + 8) as usize].to_string()),
            38 | 48 => {
                if let Some((color, consumed)) = parse_extended_color(&params[index + 1..]) {
                    if code == 38 {
                        style.fg = Some(color);
                    } else {
                        style.bg = Some(color);
                    }
                    index += consumed;
                }
            }
            _ => {}
        }
        index += 1;
    }
}

fn parse_extended_color(params: &[u16]) -> Option<(String, usize)> {
    match params {
        [5, color, ..] => Some((color_256_to_hex(*color), 2)),
        [2, r, g, b, ..] => Some((format!("rgb({r},{g},{b})"), 4)),
        _ => None,
    }
}

fn color_256_to_hex(index: u16) -> String {
    if index < 16 {
        return ANSI_COLORS[index as usize].to_string();
    }

    if index < 232 {
        let cube_index = index - 16;
        let r = cube_index / 36;
        let g = (cube_index % 36) / 6;
        let b = cube_index % 6;
        return format!(
            "#{:02x}{:02x}{:02x}",
            color_cube_component(r),
            color_cube_component(g),
            color_cube_component(b)
        );
    }

    let gray = 8 + (index.saturating_sub(232) * 10).min(247);
    format!("#{gray:02x}{gray:02x}{gray:02x}")
}

fn color_cube_component(value: u16) -> u16 {
    if value == 0 {
        0
    } else {
        55 + value * 40
    }
}

fn escape_html(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#039;")
}

fn close_span(mut output: String, open_span: bool) -> String {
    if open_span {
        output.push_str("</span>");
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_standard_styles_and_resets() {
        assert_eq!(
            ansi_to_html("\x1b[31;1mred\x1b[0m plain"),
            "<span style=\"color:#800000;font-weight:bold\">red</span> plain"
        );
    }

    #[test]
    fn converts_256_color_and_true_color() {
        assert_eq!(
            ansi_to_html("\x1b[38;5;196mhot\x1b[48;2;1;2;3mbg"),
            "<span style=\"color:#ff0000\">hot</span><span style=\"color:#ff0000;background-color:rgb(1,2,3)\">bg</span>"
        );
    }

    #[test]
    fn escapes_html_content() {
        assert_eq!(
            ansi_to_html("<tag & \"quote\">"),
            "&lt;tag &amp; &quot;quote&quot;&gt;"
        );
    }

    #[test]
    fn wraps_lines() {
        let lines = vec!["one".to_string(), String::new()];
        assert_eq!(
            ansi_lines_to_html(&lines),
            "<div class=\"ansi-line\">one</div><div class=\"ansi-line\">&nbsp;</div>"
        );
    }
}
