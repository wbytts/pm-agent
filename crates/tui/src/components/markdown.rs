use super::Component;
use crate::{
    apply_background_to_line, get_capabilities, hyperlink, is_image_line, visible_width,
    wrap_text_with_ansi,
};
use std::sync::Arc;

pub type MarkdownStyleFn = Arc<dyn Fn(&str) -> String + Send + Sync>;
pub type HighlightCodeFn = Arc<dyn Fn(&str, Option<&str>) -> Vec<String> + Send + Sync>;

#[derive(Clone, Default)]
pub struct DefaultTextStyle {
    pub color: Option<MarkdownStyleFn>,
    pub bg_color: Option<MarkdownStyleFn>,
    pub bold: bool,
    pub italic: bool,
    pub strikethrough: bool,
    pub underline: bool,
}

#[derive(Clone)]
pub struct MarkdownTheme {
    pub heading: MarkdownStyleFn,
    pub link: MarkdownStyleFn,
    pub link_url: MarkdownStyleFn,
    pub code: MarkdownStyleFn,
    pub code_block: MarkdownStyleFn,
    pub code_block_border: MarkdownStyleFn,
    pub quote: MarkdownStyleFn,
    pub quote_border: MarkdownStyleFn,
    pub hr: MarkdownStyleFn,
    pub list_bullet: MarkdownStyleFn,
    pub bold: MarkdownStyleFn,
    pub italic: MarkdownStyleFn,
    pub strikethrough: MarkdownStyleFn,
    pub underline: MarkdownStyleFn,
    pub highlight_code: Option<HighlightCodeFn>,
    pub code_block_indent: String,
}

impl Default for MarkdownTheme {
    fn default() -> Self {
        let identity: MarkdownStyleFn = Arc::new(str::to_string);
        Self {
            heading: identity.clone(),
            link: identity.clone(),
            link_url: identity.clone(),
            code: identity.clone(),
            code_block: identity.clone(),
            code_block_border: identity.clone(),
            quote: identity.clone(),
            quote_border: identity.clone(),
            hr: identity.clone(),
            list_bullet: identity.clone(),
            bold: identity.clone(),
            italic: identity.clone(),
            strikethrough: identity.clone(),
            underline: identity,
            highlight_code: None,
            code_block_indent: "  ".to_string(),
        }
    }
}

pub struct Markdown {
    text: String,
    padding_x: usize,
    padding_y: usize,
    theme: MarkdownTheme,
    default_text_style: Option<DefaultTextStyle>,
    cached_text: Option<String>,
    cached_width: Option<usize>,
    cached_lines: Option<Vec<String>>,
}

impl Markdown {
    pub fn new(
        text: impl Into<String>,
        padding_x: usize,
        padding_y: usize,
        theme: MarkdownTheme,
        default_text_style: Option<DefaultTextStyle>,
    ) -> Self {
        Self {
            text: text.into(),
            padding_x,
            padding_y,
            theme,
            default_text_style,
            cached_text: None,
            cached_width: None,
            cached_lines: None,
        }
    }

    pub fn set_text(&mut self, text: impl Into<String>) {
        self.text = text.into();
        self.invalidate();
    }

    fn render_blocks(&self, width: usize) -> Vec<String> {
        let normalized = self.text.replace('\t', "   ");
        let mut lines = Vec::new();
        let source = normalized.lines().collect::<Vec<_>>();
        let mut index = 0;

        while index < source.len() {
            let line = source[index];
            let trimmed = line.trim();
            if trimmed.is_empty() {
                lines.push(String::new());
                index += 1;
                continue;
            }

            if let Some((lang, fence)) = parse_fence_start(trimmed) {
                let mut code = Vec::new();
                index += 1;
                while index < source.len() && !source[index].trim_start().starts_with(fence) {
                    code.push(source[index]);
                    index += 1;
                }
                if index < source.len() {
                    index += 1;
                }
                self.render_code_block(&mut lines, &code.join("\n"), lang.as_deref());
                self.push_spacing_if_needed(&mut lines, index, &source);
                continue;
            }

            if let Some(level) = heading_level(trimmed) {
                let text = trimmed[level + 1..].trim();
                let rendered = self.render_inline(text, InlineMode::Heading(level));
                let styled = if level >= 3 {
                    let prefix = (self.theme.heading)(&format!("{} ", "#".repeat(level)));
                    format!("{prefix}{rendered}")
                } else {
                    rendered
                };
                lines.push(styled);
                self.push_spacing_if_needed(&mut lines, index + 1, &source);
                index += 1;
                continue;
            }

            if is_hr(trimmed) {
                lines.push((self.theme.hr)(&"─".repeat(width.min(80))));
                self.push_spacing_if_needed(&mut lines, index + 1, &source);
                index += 1;
                continue;
            }

            if trimmed.starts_with('>') {
                let mut quote_lines = Vec::new();
                while index < source.len() && source[index].trim_start().starts_with('>') {
                    let quote = source[index]
                        .trim_start()
                        .trim_start_matches('>')
                        .trim_start();
                    quote_lines.push(quote);
                    index += 1;
                }
                self.render_quote(&mut lines, &quote_lines, width);
                self.push_spacing_if_needed(&mut lines, index, &source);
                continue;
            }

            if is_table_start(&source, index) {
                let mut table_lines = Vec::new();
                while index < source.len()
                    && source[index].contains('|')
                    && !source[index].trim().is_empty()
                {
                    table_lines.push(source[index]);
                    index += 1;
                }
                self.render_table(&mut lines, &table_lines, width);
                self.push_spacing_if_needed(&mut lines, index, &source);
                continue;
            }

            if parse_list_item(line).is_some() {
                let mut items = Vec::new();
                while index < source.len() {
                    if let Some(item) = parse_list_item(source[index]) {
                        items.push(item);
                        index += 1;
                    } else {
                        break;
                    }
                }
                self.render_list(&mut lines, &items, width);
                continue;
            }

            let mut paragraph = vec![trimmed.to_string()];
            index += 1;
            while index < source.len()
                && !source[index].trim().is_empty()
                && parse_fence_start(source[index].trim()).is_none()
                && heading_level(source[index].trim()).is_none()
                && !is_hr(source[index].trim())
                && !source[index].trim_start().starts_with('>')
                && parse_list_item(source[index]).is_none()
                && !is_table_start(&source, index)
            {
                paragraph.push(source[index].trim().to_string());
                index += 1;
            }
            lines.push(self.render_inline(&paragraph.join(" "), InlineMode::Default));
            self.push_spacing_if_needed(&mut lines, index, &source);
        }

        lines
    }

    fn render_code_block(&self, lines: &mut Vec<String>, code: &str, lang: Option<&str>) {
        lines.push((self.theme.code_block_border)(&format!(
            "```{}",
            lang.unwrap_or_default()
        )));
        if let Some(highlight_code) = &self.theme.highlight_code {
            for line in highlight_code(code, lang) {
                lines.push(format!("{}{}", self.theme.code_block_indent, line));
            }
        } else {
            for line in code.split('\n') {
                lines.push(format!(
                    "{}{}",
                    self.theme.code_block_indent,
                    (self.theme.code_block)(line)
                ));
            }
        }
        lines.push((self.theme.code_block_border)("```"));
    }

    fn render_quote(&self, lines: &mut Vec<String>, quote_lines: &[&str], width: usize) {
        let content_width = width.saturating_sub(2).max(1);
        for line in quote_lines {
            let styled = (self.theme.quote)(&(self.theme.italic)(
                &self.render_inline(line, InlineMode::Plain),
            ));
            for wrapped in wrap_text_with_ansi(&styled, content_width) {
                lines.push(format!("{}{}", (self.theme.quote_border)("│ "), wrapped));
            }
        }
    }

    fn render_list(&self, lines: &mut Vec<String>, items: &[ListItem], width: usize) {
        for item in items {
            let marker = format!(
                "{}{}",
                item.bullet,
                item.task_marker.as_deref().unwrap_or("")
            );
            let first_prefix = format!(
                "{}{}",
                " ".repeat(item.indent),
                (self.theme.list_bullet)(&marker)
            );
            let continuation_prefix = format!(
                "{}{}",
                " ".repeat(item.indent),
                " ".repeat(visible_width(&marker))
            );
            let item_width = width.saturating_sub(visible_width(&first_prefix)).max(1);
            let rendered = self.render_inline(&item.text, InlineMode::Default);
            let mut first = true;
            for wrapped in wrap_text_with_ansi(&rendered, item_width) {
                let prefix = if first {
                    &first_prefix
                } else {
                    &continuation_prefix
                };
                lines.push(format!("{prefix}{wrapped}"));
                first = false;
            }
        }
    }

    fn render_table(&self, lines: &mut Vec<String>, table_lines: &[&str], available_width: usize) {
        if table_lines.len() < 2 {
            return;
        }
        let header = split_table_row(table_lines[0]);
        let rows = table_lines
            .iter()
            .skip(2)
            .map(|line| split_table_row(line))
            .collect::<Vec<_>>();
        let cols = header.len();
        if cols == 0 {
            return;
        }

        let overhead = 3 * cols + 1;
        if available_width <= overhead {
            lines.extend(wrap_text_with_ansi(
                &table_lines.join("\n"),
                available_width,
            ));
            return;
        }
        let available_cells = available_width - overhead;
        let mut widths = vec![1usize; cols];
        for (idx, cell) in header.iter().enumerate() {
            widths[idx] = widths[idx].max(visible_width(
                &self.render_inline(cell, InlineMode::Default),
            ));
        }
        for row in &rows {
            for (idx, cell) in row.iter().enumerate().take(cols) {
                widths[idx] = widths[idx].max(visible_width(
                    &self.render_inline(cell, InlineMode::Default),
                ));
            }
        }
        shrink_widths(&mut widths, available_cells);

        lines.push(format!(
            "┌─{}─┐",
            widths
                .iter()
                .map(|w| "─".repeat(*w))
                .collect::<Vec<_>>()
                .join("─┬─")
        ));
        self.render_table_row(lines, &header, &widths, true);
        let separator = format!(
            "├─{}─┤",
            widths
                .iter()
                .map(|w| "─".repeat(*w))
                .collect::<Vec<_>>()
                .join("─┼─")
        );
        lines.push(separator.clone());
        for (idx, row) in rows.iter().enumerate() {
            self.render_table_row(lines, row, &widths, false);
            if idx + 1 < rows.len() {
                lines.push(separator.clone());
            }
        }
        lines.push(format!(
            "└─{}─┘",
            widths
                .iter()
                .map(|w| "─".repeat(*w))
                .collect::<Vec<_>>()
                .join("─┴─")
        ));
    }

    fn render_table_row(
        &self,
        lines: &mut Vec<String>,
        cells: &[String],
        widths: &[usize],
        header: bool,
    ) {
        let wrapped_cells = widths
            .iter()
            .enumerate()
            .map(|(idx, width)| {
                let rendered = self.render_inline(
                    cells.get(idx).map(String::as_str).unwrap_or(""),
                    InlineMode::Default,
                );
                wrap_text_with_ansi(&rendered, *width)
            })
            .collect::<Vec<_>>();
        let row_count = wrapped_cells.iter().map(Vec::len).max().unwrap_or(1);
        for row_idx in 0..row_count {
            let parts = wrapped_cells
                .iter()
                .enumerate()
                .map(|(idx, lines)| {
                    let text = lines.get(row_idx).cloned().unwrap_or_default();
                    let padded = format!(
                        "{}{}",
                        text,
                        " ".repeat(widths[idx].saturating_sub(visible_width(&text)))
                    );
                    if header {
                        (self.theme.bold)(&padded)
                    } else {
                        padded
                    }
                })
                .collect::<Vec<_>>();
            lines.push(format!("│ {} │", parts.join(" │ ")));
        }
    }

    fn render_inline(&self, text: &str, mode: InlineMode) -> String {
        let styled_text = render_inline_markers(text, self, mode);
        match mode {
            InlineMode::Plain | InlineMode::Heading(_) => styled_text,
            InlineMode::Default => self.apply_default_style(&styled_text),
        }
    }

    fn apply_default_style(&self, text: &str) -> String {
        let Some(style) = &self.default_text_style else {
            return text.to_string();
        };
        let mut styled = text.to_string();
        if let Some(color) = &style.color {
            styled = color(&styled);
        }
        if style.bold {
            styled = (self.theme.bold)(&styled);
        }
        if style.italic {
            styled = (self.theme.italic)(&styled);
        }
        if style.strikethrough {
            styled = (self.theme.strikethrough)(&styled);
        }
        if style.underline {
            styled = (self.theme.underline)(&styled);
        }
        styled
    }

    fn push_spacing_if_needed(&self, lines: &mut Vec<String>, next_index: usize, source: &[&str]) {
        if next_index < source.len() && !source[next_index].trim().is_empty() {
            lines.push(String::new());
        }
    }
}

impl Component for Markdown {
    fn render(&mut self, width: usize) -> Vec<String> {
        if self.cached_text.as_deref() == Some(&self.text) && self.cached_width == Some(width) {
            if let Some(lines) = &self.cached_lines {
                return lines.clone();
            }
        }
        if self.text.trim().is_empty() {
            self.cached_text = Some(self.text.clone());
            self.cached_width = Some(width);
            self.cached_lines = Some(Vec::new());
            return Vec::new();
        }

        let content_width = width.saturating_sub(self.padding_x * 2).max(1);
        let rendered = self.render_blocks(content_width);
        let mut wrapped_lines = Vec::new();
        for line in rendered {
            if is_image_line(&line) {
                wrapped_lines.push(line);
            } else {
                wrapped_lines.extend(wrap_text_with_ansi(&line, content_width));
            }
        }

        let bg_fn = self
            .default_text_style
            .as_ref()
            .and_then(|style| style.bg_color.clone());
        let left = " ".repeat(self.padding_x);
        let right = " ".repeat(self.padding_x);
        let mut content_lines = Vec::new();
        for line in wrapped_lines {
            if is_image_line(&line) {
                content_lines.push(line);
                continue;
            }
            let line_with_margins = format!("{left}{line}{right}");
            if let Some(bg_fn) = &bg_fn {
                content_lines.push(apply_background_to_line(
                    &line_with_margins,
                    width,
                    |line| bg_fn(line),
                ));
            } else {
                content_lines.push(format!(
                    "{}{}",
                    line_with_margins,
                    " ".repeat(width.saturating_sub(visible_width(&line_with_margins)))
                ));
            }
        }

        let empty_line = " ".repeat(width);
        let mut result = Vec::new();
        for _ in 0..self.padding_y {
            result.push(if let Some(bg_fn) = &bg_fn {
                apply_background_to_line(&empty_line, width, |line| bg_fn(line))
            } else {
                empty_line.clone()
            });
        }
        result.extend(content_lines);
        for _ in 0..self.padding_y {
            result.push(if let Some(bg_fn) = &bg_fn {
                apply_background_to_line(&empty_line, width, |line| bg_fn(line))
            } else {
                empty_line.clone()
            });
        }
        if result.is_empty() {
            result.push(String::new());
        }

        self.cached_text = Some(self.text.clone());
        self.cached_width = Some(width);
        self.cached_lines = Some(result.clone());
        result
    }

    fn invalidate(&mut self) {
        self.cached_text = None;
        self.cached_width = None;
        self.cached_lines = None;
    }
}

#[derive(Debug, Clone, Copy)]
enum InlineMode {
    Default,
    Plain,
    Heading(usize),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ListItem {
    indent: usize,
    bullet: String,
    task_marker: Option<String>,
    text: String,
}

fn render_inline_markers(text: &str, markdown: &Markdown, mode: InlineMode) -> String {
    let chars = text.chars().collect::<Vec<_>>();
    let mut result = String::new();
    let mut index = 0;
    while index < chars.len() {
        if chars[index] == '`' {
            if let Some(end) = find_marker(&chars, index + 1, "`") {
                result.push_str(&(markdown.theme.code)(
                    &chars[index + 1..end].iter().collect::<String>(),
                ));
                index = end + 1;
                continue;
            }
        }
        if starts_with(&chars, index, "**") {
            if let Some(end) = find_marker(&chars, index + 2, "**") {
                let inner = render_inline_markers(
                    &chars[index + 2..end].iter().collect::<String>(),
                    markdown,
                    mode,
                );
                result.push_str(&(markdown.theme.bold)(&inner));
                index = end + 2;
                continue;
            }
        }
        if starts_with(&chars, index, "~~")
            && index + 2 < chars.len()
            && !chars[index + 2].is_whitespace()
        {
            if let Some(end) = find_marker(&chars, index + 2, "~~") {
                let inner = render_inline_markers(
                    &chars[index + 2..end].iter().collect::<String>(),
                    markdown,
                    mode,
                );
                result.push_str(&(markdown.theme.strikethrough)(&inner));
                index = end + 2;
                continue;
            }
        }
        if chars[index] == '*' {
            if let Some(end) = find_marker(&chars, index + 1, "*") {
                let inner = render_inline_markers(
                    &chars[index + 1..end].iter().collect::<String>(),
                    markdown,
                    mode,
                );
                result.push_str(&(markdown.theme.italic)(&inner));
                index = end + 1;
                continue;
            }
        }
        if chars[index] == '[' {
            if let Some(close) = chars[index + 1..].iter().position(|ch| *ch == ']') {
                let close = index + 1 + close;
                if chars.get(close + 1) == Some(&'(') {
                    if let Some(end) = chars[close + 2..].iter().position(|ch| *ch == ')') {
                        let end = close + 2 + end;
                        let label = chars[index + 1..close].iter().collect::<String>();
                        let href = chars[close + 2..end].iter().collect::<String>();
                        let link_text = render_inline_markers(&label, markdown, mode);
                        let styled_link =
                            (markdown.theme.link)(&(markdown.theme.underline)(&link_text));
                        if get_capabilities().hyperlinks {
                            result.push_str(&hyperlink(&styled_link, &href));
                        } else if label == href
                            || href.strip_prefix("mailto:") == Some(label.as_str())
                        {
                            result.push_str(&styled_link);
                        } else {
                            result.push_str(&format!(
                                "{}{}",
                                styled_link,
                                (markdown.theme.link_url)(&format!(" ({href})"))
                            ));
                        }
                        index = end + 1;
                        continue;
                    }
                }
            }
        }
        result.push(chars[index]);
        index += 1;
    }

    match mode {
        InlineMode::Heading(1) => {
            (markdown.theme.heading)(&(markdown.theme.bold)(&(markdown.theme.underline)(&result)))
        }
        InlineMode::Heading(_) => (markdown.theme.heading)(&(markdown.theme.bold)(&result)),
        _ => result,
    }
}

fn parse_fence_start(line: &str) -> Option<(Option<String>, &str)> {
    if line.starts_with("```") {
        Some((non_empty(line.trim_start_matches('`').trim()), "```"))
    } else if line.starts_with("~~~") {
        Some((non_empty(line.trim_start_matches('~').trim()), "~~~"))
    } else {
        None
    }
}

fn non_empty(value: &str) -> Option<String> {
    (!value.is_empty()).then(|| value.to_string())
}

fn heading_level(line: &str) -> Option<usize> {
    let level = line.chars().take_while(|ch| *ch == '#').count();
    (1..=6)
        .contains(&level)
        .then_some(level)
        .filter(|level| line.chars().nth(*level) == Some(' '))
}

fn is_hr(line: &str) -> bool {
    let compact = line.split_whitespace().collect::<String>();
    compact.len() >= 3 && compact.chars().all(|ch| matches!(ch, '-' | '*' | '_'))
}

fn parse_list_item(line: &str) -> Option<ListItem> {
    let indent = line.chars().take_while(|ch| *ch == ' ').count();
    let trimmed = line.trim_start();
    let (bullet, rest) = if let Some(rest) = trimmed.strip_prefix("- ") {
        ("- ".to_string(), rest)
    } else if let Some(rest) = trimmed.strip_prefix("* ") {
        ("- ".to_string(), rest)
    } else {
        let dot = trimmed.find(". ")?;
        if trimmed[..dot].chars().all(|ch| ch.is_ascii_digit()) {
            (format!("{}. ", &trimmed[..dot]), &trimmed[dot + 2..])
        } else {
            return None;
        }
    };
    let (task_marker, text) = if let Some(rest) = rest.strip_prefix("[ ] ") {
        (Some("[ ] ".to_string()), rest)
    } else if let Some(rest) = rest
        .strip_prefix("[x] ")
        .or_else(|| rest.strip_prefix("[X] "))
    {
        (Some("[x] ".to_string()), rest)
    } else {
        (None, rest)
    };
    Some(ListItem {
        indent,
        bullet,
        task_marker,
        text: text.to_string(),
    })
}

fn is_table_start(lines: &[&str], index: usize) -> bool {
    index + 1 < lines.len()
        && lines[index].contains('|')
        && lines[index + 1].contains('|')
        && split_table_row(lines[index + 1])
            .iter()
            .all(|cell| cell.chars().all(|ch| matches!(ch, '-' | ':' | ' ')))
}

fn split_table_row(line: &str) -> Vec<String> {
    line.trim()
        .trim_matches('|')
        .split('|')
        .map(|cell| cell.trim().to_string())
        .collect()
}

fn shrink_widths(widths: &mut [usize], available: usize) {
    let total = widths.iter().sum::<usize>();
    if total <= available {
        return;
    }
    let mut over = total - available;
    while over > 0 && widths.iter().any(|width| *width > 1) {
        for width in widths.iter_mut() {
            if over == 0 {
                break;
            }
            if *width > 1 {
                *width -= 1;
                over -= 1;
            }
        }
    }
}

fn starts_with(chars: &[char], index: usize, marker: &str) -> bool {
    marker
        .chars()
        .enumerate()
        .all(|(offset, ch)| chars.get(index + offset) == Some(&ch))
}

fn find_marker(chars: &[char], mut index: usize, marker: &str) -> Option<usize> {
    while index < chars.len() {
        if starts_with(chars, index, marker) {
            return Some(index);
        }
        index += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{reset_capabilities_cache, set_capabilities, TerminalCapabilities};

    fn themed() -> MarkdownTheme {
        MarkdownTheme {
            heading: Arc::new(|text| format!("<h>{text}</h>")),
            link: Arc::new(|text| format!("<a>{text}</a>")),
            link_url: Arc::new(|text| format!("<url>{text}</url>")),
            code: Arc::new(|text| format!("<c>{text}</c>")),
            code_block: Arc::new(|text| format!("<cb>{text}</cb>")),
            code_block_border: Arc::new(|text| format!("<b>{text}</b>")),
            quote: Arc::new(|text| format!("<q>{text}</q>")),
            quote_border: Arc::new(|text| format!("<qb>{text}</qb>")),
            hr: Arc::new(|text| format!("<hr>{text}</hr>")),
            list_bullet: Arc::new(|text| format!("<li>{text}</li>")),
            bold: Arc::new(|text| format!("<strong>{text}</strong>")),
            italic: Arc::new(|text| format!("<em>{text}</em>")),
            strikethrough: Arc::new(|text| format!("<del>{text}</del>")),
            underline: Arc::new(|text| format!("<u>{text}</u>")),
            highlight_code: None,
            code_block_indent: "  ".to_string(),
        }
    }

    #[test]
    fn markdown_renders_headings_inline_styles_and_links() {
        let _guard = crate::terminal_image::CAPABILITIES_TEST_LOCK
            .get_or_init(|| std::sync::Mutex::new(()))
            .lock()
            .expect("lock capabilities test");
        set_capabilities(TerminalCapabilities {
            images: None,
            true_color: true,
            hyperlinks: false,
        });
        let mut markdown = Markdown::new(
            "# Title\n\nhello **bold** *it* `code` ~~gone~~ [site](https://example.com)",
            0,
            0,
            themed(),
            None,
        );

        let lines = markdown.render(120);

        assert!(lines[0].contains("<h><strong><u>Title</u></strong></h>"));
        assert!(lines
            .iter()
            .any(|line| line.contains("<strong>bold</strong>")));
        assert!(lines.iter().any(|line| line.contains("<em>it</em>")));
        assert!(lines.iter().any(|line| line.contains("<c>code</c>")));
        assert!(lines.iter().any(|line| line.contains("<del>gone</del>")));
        assert!(lines
            .iter()
            .any(|line| line.contains("<url> (https://example.com)</url>")));
        reset_capabilities_cache();
    }

    #[test]
    fn markdown_renders_lists_quotes_code_and_tables() {
        let mut markdown = Markdown::new(
            "- [x] done\n- todo\n\n> quoted\n\n```rust\nfn main() {}\n```\n\n| A | B |\n| - | - |\n| 1 | 2 |",
            0,
            0,
            themed(),
            None,
        );

        let lines = markdown.render(80);

        assert!(lines
            .iter()
            .any(|line| line.contains("<li>- [x] </li>done")));
        assert!(lines
            .iter()
            .any(|line| line.contains("<qb>│ </qb><q><em>quoted</em></q>")));
        assert!(lines.iter().any(|line| line.contains("<b>```rust</b>")));
        assert!(lines
            .iter()
            .any(|line| line.contains("<cb>fn main() {}</cb>")));
        assert!(lines.iter().any(|line| line.starts_with("┌─")));
        assert!(lines.iter().any(|line| line.contains("<strong>A</strong>")));
    }

    #[test]
    fn markdown_applies_padding_background_cache_and_invalidate() {
        let style = DefaultTextStyle {
            color: None,
            bg_color: Some(Arc::new(|text| format!("[{text}]"))),
            bold: false,
            italic: false,
            strikethrough: false,
            underline: false,
        };
        let mut markdown = Markdown::new("hello", 1, 1, MarkdownTheme::default(), Some(style));

        let first = markdown.render(10);
        let cached = markdown.render(10);
        assert_eq!(first, cached);
        assert_eq!(first.len(), 3);
        assert!(first[1].starts_with("[ hello"));

        markdown.set_text("world");
        let second = markdown.render(10);
        assert_ne!(first, second);
        assert!(second[1].contains("world"));
    }
}
