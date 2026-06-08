#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnsiCode {
    pub code: String,
    pub length: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SliceWithWidth {
    pub text: String,
    pub width: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VisualTruncateResult {
    pub visual_lines: Vec<String>,
    pub skipped_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractSegmentsResult {
    pub before: String,
    pub before_width: usize,
    pub after: String,
    pub after_width: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TextSegment {
    text: String,
    width: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ActiveHyperlink {
    params: String,
    url: String,
    terminator: &'static str,
}

const RESET: &str = "\x1b[0m";

pub fn visible_width(text: &str) -> usize {
    if text.is_empty() {
        return 0;
    }

    if is_printable_ascii(text) {
        return text.len();
    }

    let mut width = 0;
    for segment in segments_without_ansi(text) {
        width += segment.width;
    }
    width
}

pub fn normalize_terminal_output(text: &str) -> String {
    if !text.contains('\u{0e33}') && !text.contains('\u{0eb3}') {
        return text.to_string();
    }

    text.chars()
        .flat_map(|ch| match ch {
            '\u{0e33}' => ['\u{0e4d}', '\u{0e32}'].into_iter().collect::<Vec<_>>(),
            '\u{0eb3}' => ['\u{0ecd}', '\u{0eb2}'].into_iter().collect::<Vec<_>>(),
            _ => [ch].into_iter().collect::<Vec<_>>(),
        })
        .collect()
}

pub fn extract_ansi_code(text: &str, pos: usize) -> Option<AnsiCode> {
    if pos >= text.len() || !text.is_char_boundary(pos) || text.as_bytes()[pos] != 0x1b {
        return None;
    }

    let bytes = text.as_bytes();
    let next = *bytes.get(pos + 1)?;

    match next {
        b'[' => {
            let mut end = pos + 2;
            while end < bytes.len() {
                if matches!(bytes[end], b'm' | b'G' | b'K' | b'H' | b'J') {
                    let length = end + 1 - pos;
                    return Some(AnsiCode {
                        code: text[pos..end + 1].to_string(),
                        length,
                    });
                }
                end += 1;
            }
            None
        }
        b']' | b'_' => {
            let mut end = pos + 2;
            while end < bytes.len() {
                if bytes[end] == 0x07 {
                    let length = end + 1 - pos;
                    return Some(AnsiCode {
                        code: text[pos..end + 1].to_string(),
                        length,
                    });
                }
                if bytes[end] == 0x1b && bytes.get(end + 1) == Some(&b'\\') {
                    let length = end + 2 - pos;
                    return Some(AnsiCode {
                        code: text[pos..end + 2].to_string(),
                        length,
                    });
                }
                end += 1;
            }
            None
        }
        _ => None,
    }
}

pub fn wrap_text_with_ansi(text: &str, width: usize) -> Vec<String> {
    if text.is_empty() {
        return vec![String::new()];
    }

    if width == 0 {
        return text.split('\n').map(|_| String::new()).collect();
    }

    let mut result = Vec::new();
    let mut active_hyperlink: Option<ActiveHyperlink> = None;
    let mut active_sgr = ActiveSgr::default();
    for line in text.split('\n') {
        let prepend_active_prefix = !result.is_empty();
        result.extend(wrap_single_line_with_state(
            line,
            width,
            &mut active_hyperlink,
            &mut active_sgr,
            prepend_active_prefix,
        ));
    }

    if result.is_empty() {
        vec![String::new()]
    } else {
        result
    }
}

pub fn truncate_to_visual_lines(
    text: &str,
    max_visual_lines: usize,
    width: usize,
    padding_x: usize,
) -> VisualTruncateResult {
    if text.is_empty() || text.trim().is_empty() || max_visual_lines == 0 {
        return VisualTruncateResult {
            visual_lines: Vec::new(),
            skipped_count: 0,
        };
    }

    let normalized_text = text.replace('\t', "   ");
    let content_width = width.saturating_sub(padding_x * 2).max(1);
    let left_margin = " ".repeat(padding_x);
    let right_margin = " ".repeat(padding_x);
    let all_visual_lines = wrap_text_with_ansi(&normalized_text, content_width)
        .into_iter()
        .map(|line| {
            let line = format!("{left_margin}{line}{right_margin}");
            let padding = " ".repeat(width.saturating_sub(visible_width(&line)));
            format!("{line}{padding}")
        })
        .collect::<Vec<_>>();

    if all_visual_lines.len() <= max_visual_lines {
        return VisualTruncateResult {
            visual_lines: all_visual_lines,
            skipped_count: 0,
        };
    }

    let skipped_count = all_visual_lines.len() - max_visual_lines;
    VisualTruncateResult {
        visual_lines: all_visual_lines
            .into_iter()
            .skip(skipped_count)
            .collect::<Vec<_>>(),
        skipped_count,
    }
}

pub fn is_whitespace_char(ch: char) -> bool {
    ch.is_whitespace()
}

pub fn is_punctuation_char(ch: char) -> bool {
    matches!(
        ch,
        '(' | ')'
            | '{'
            | '}'
            | '['
            | ']'
            | '<'
            | '>'
            | '.'
            | ','
            | ';'
            | ':'
            | '\''
            | '"'
            | '!'
            | '?'
            | '+'
            | '-'
            | '='
            | '*'
            | '/'
            | '\\'
            | '|'
            | '&'
            | '%'
            | '^'
            | '$'
            | '#'
            | '@'
            | '~'
            | '`'
    )
}

pub fn apply_background_to_line<F>(line: &str, width: usize, bg_fn: F) -> String
where
    F: FnOnce(&str) -> String,
{
    let visible_len = visible_width(line);
    let padding = " ".repeat(width.saturating_sub(visible_len));
    bg_fn(&(line.to_string() + &padding))
}

pub fn truncate_to_width(text: &str, max_width: usize, ellipsis: &str, pad: bool) -> String {
    if max_width == 0 {
        return String::new();
    }

    if text.is_empty() {
        return if pad {
            " ".repeat(max_width)
        } else {
            String::new()
        };
    }

    let text_width = visible_width(text);
    if text_width <= max_width {
        return if pad {
            text.to_string() + &" ".repeat(max_width - text_width)
        } else {
            text.to_string()
        };
    }

    let ellipsis_width = visible_width(ellipsis);
    if ellipsis_width >= max_width {
        let clipped = truncate_fragment_to_width(ellipsis, max_width);
        if clipped.width == 0 {
            return if pad {
                " ".repeat(max_width)
            } else {
                String::new()
            };
        }
        return finalize_truncated_result("", 0, &clipped.text, clipped.width, max_width, pad);
    }

    let target_width = max_width - ellipsis_width;
    let clipped = truncate_fragment_to_width(text, target_width);
    finalize_truncated_result(
        &clipped.text,
        clipped.width,
        ellipsis,
        ellipsis_width,
        max_width,
        pad,
    )
}

pub fn slice_by_column(line: &str, start_col: usize, length: usize, strict: bool) -> String {
    slice_with_width(line, start_col, length, strict).text
}

pub fn slice_with_width(
    line: &str,
    start_col: usize,
    length: usize,
    strict: bool,
) -> SliceWithWidth {
    if length == 0 {
        return SliceWithWidth {
            text: String::new(),
            width: 0,
        };
    }

    let end_col = start_col + length;
    let mut result = String::new();
    let mut result_width = 0;
    let mut current_col = 0;
    let mut pending_ansi = String::new();
    let mut i = 0;

    while i < line.len() {
        if let Some(ansi) = extract_ansi_code(line, i) {
            if current_col >= start_col && current_col < end_col {
                result.push_str(&ansi.code);
            } else if current_col < start_col {
                pending_ansi.push_str(&ansi.code);
            }
            i += ansi.length;
            continue;
        }

        let (segment, next_i) = next_text_segment(line, i);
        let in_range = current_col >= start_col && current_col < end_col;
        let fits = !strict || current_col + segment.width <= end_col;
        if in_range && fits {
            if !pending_ansi.is_empty() {
                result.push_str(&pending_ansi);
                pending_ansi.clear();
            }
            result.push_str(&segment.text);
            result_width += segment.width;
        }
        current_col += segment.width;
        i = next_i;

        if current_col >= end_col {
            break;
        }
    }

    SliceWithWidth {
        text: result,
        width: result_width,
    }
}

pub fn extract_segments(
    line: &str,
    before_end: usize,
    after_start: usize,
    after_len: usize,
    strict_after: bool,
) -> ExtractSegmentsResult {
    let after_end = after_start + after_len;
    let mut before = String::new();
    let mut before_width = 0;
    let mut after = String::new();
    let mut after_width = 0;
    let mut current_col = 0;
    let mut i = 0;
    let mut pending_ansi_before = String::new();
    let mut active_ansi = String::new();
    let mut after_started = false;

    while i < line.len() {
        if let Some(ansi) = extract_ansi_code(line, i) {
            active_ansi.push_str(&ansi.code);
            if current_col < before_end {
                pending_ansi_before.push_str(&ansi.code);
            } else if current_col >= after_start && current_col < after_end && after_started {
                after.push_str(&ansi.code);
            }
            i += ansi.length;
            continue;
        }

        let (segment, next_i) = next_text_segment(line, i);
        if current_col < before_end {
            if !pending_ansi_before.is_empty() {
                before.push_str(&pending_ansi_before);
                pending_ansi_before.clear();
            }
            before.push_str(&segment.text);
            before_width += segment.width;
        } else if current_col >= after_start && current_col < after_end {
            let fits = !strict_after || current_col + segment.width <= after_end;
            if fits {
                if !after_started {
                    after.push_str(&active_ansi);
                    after_started = true;
                }
                after.push_str(&segment.text);
                after_width += segment.width;
            }
        }

        current_col += segment.width;
        i = next_i;
        if after_len == 0 {
            if current_col >= before_end {
                break;
            }
        } else if current_col >= after_end {
            break;
        }
    }

    ExtractSegmentsResult {
        before,
        before_width,
        after,
        after_width,
    }
}

fn wrap_single_line_with_state(
    line: &str,
    width: usize,
    active_hyperlink: &mut Option<ActiveHyperlink>,
    active_sgr: &mut ActiveSgr,
    prepend_active_prefix: bool,
) -> Vec<String> {
    if line.is_empty() {
        return vec![String::new()];
    }

    let line = if prepend_active_prefix {
        format!("{}{}", active_prefix(active_hyperlink, active_sgr), line)
    } else {
        line.to_string()
    };

    if visible_width(&line) <= width {
        update_wrap_state_from_text(&line, active_hyperlink, active_sgr);
        return vec![line];
    }

    let mut wrapped = Vec::new();
    let mut current_line = String::new();
    let mut current_width = 0;
    let tokens = split_tokens_with_ansi(&line);

    for token in tokens {
        let token_width = visible_width(&token);
        let is_whitespace = token.trim().is_empty();

        if token_width > width && !is_whitespace {
            if !current_line.is_empty() {
                let mut line_to_wrap = current_line.trim_end().to_string();
                line_to_wrap.push_str(&line_break_suffix(&active_hyperlink, &active_sgr));
                wrapped.push(line_to_wrap);
                current_line.clear();
                current_width = 0;
            }

            let broken = break_long_token(&token, width, active_hyperlink, active_sgr);
            if let Some((last, prefix)) = broken.split_last() {
                wrapped.extend(prefix.iter().cloned());
                current_width = visible_width(last);
                current_line = last.clone();
            }
            continue;
        }

        if current_width > 0 && current_width + token_width > width {
            let mut line_to_wrap = current_line.trim_end().to_string();
            line_to_wrap.push_str(&line_break_suffix(&active_hyperlink, &active_sgr));
            wrapped.push(line_to_wrap);

            if is_whitespace {
                current_line = active_prefix(active_hyperlink, active_sgr);
                current_width = 0;
            } else {
                current_line = active_prefix(active_hyperlink, active_sgr);
                current_line.push_str(&token);
                current_width = token_width;
            }
        } else {
            current_line.push_str(&token);
            current_width += token_width;
        }

        update_wrap_state_from_text(&token, active_hyperlink, active_sgr);
    }

    if !current_line.is_empty() {
        wrapped.push(current_line);
    }

    if wrapped.is_empty() {
        vec![String::new()]
    } else {
        wrapped
            .into_iter()
            .map(|line| line.trim_end().to_string())
            .collect()
    }
}

fn split_tokens_with_ansi(line: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut current_is_whitespace: Option<bool> = None;
    let mut i = 0;

    while i < line.len() {
        if let Some(ansi) = extract_ansi_code(line, i) {
            if current_is_whitespace == Some(true) && visible_width(&current) > 0 {
                tokens.push(current);
                current = String::new();
                current_is_whitespace = None;
            }
            current.push_str(&ansi.code);
            i += ansi.length;
            continue;
        }

        let (segment, next_i) = next_text_segment(line, i);
        let is_whitespace = segment.text.trim().is_empty();
        if let Some(previous_is_whitespace) = current_is_whitespace {
            if previous_is_whitespace != is_whitespace && visible_width(&current) > 0 {
                tokens.push(current);
                current = String::new();
            }
        }
        current_is_whitespace = Some(is_whitespace);
        current.push_str(&segment.text);
        i = next_i;
    }

    if !current.is_empty() {
        tokens.push(current);
    }

    tokens
}

fn break_long_token(
    token: &str,
    width: usize,
    active_hyperlink: &mut Option<ActiveHyperlink>,
    active_sgr: &mut ActiveSgr,
) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current = active_prefix(active_hyperlink, active_sgr);
    let mut current_width = 0;
    let mut i = 0;

    while i < token.len() {
        if let Some(ansi) = extract_ansi_code(token, i) {
            apply_wrap_ansi(&ansi.code, active_hyperlink, active_sgr);
            current.push_str(&ansi.code);
            i += ansi.length;
            continue;
        }

        let (segment, next_i) = next_text_segment(token, i);
        if current_width + segment.width > width {
            current.push_str(&line_break_suffix(active_hyperlink, active_sgr));
            lines.push(current);
            current = active_prefix(active_hyperlink, active_sgr);
            current_width = 0;
        }

        current.push_str(&segment.text);
        current_width += segment.width;
        i = next_i;
    }

    if !current.is_empty() {
        lines.push(current);
    }

    if lines.is_empty() {
        vec![String::new()]
    } else {
        lines
    }
}

fn update_wrap_state_from_text(
    text: &str,
    active_hyperlink: &mut Option<ActiveHyperlink>,
    active_sgr: &mut ActiveSgr,
) {
    let mut i = 0;
    while i < text.len() {
        if let Some(ansi) = extract_ansi_code(text, i) {
            apply_wrap_ansi(&ansi.code, active_hyperlink, active_sgr);
            i += ansi.length;
        } else {
            let (_, next_i) = next_text_segment(text, i);
            i = next_i;
        }
    }
}

fn apply_wrap_ansi(
    ansi_code: &str,
    active_hyperlink: &mut Option<ActiveHyperlink>,
    active_sgr: &mut ActiveSgr,
) {
    if let Some(parsed) = parse_osc8_hyperlink(ansi_code) {
        *active_hyperlink = parsed;
    } else if is_sgr_code(ansi_code) {
        active_sgr.apply(ansi_code);
    }
}

fn active_prefix(active_hyperlink: &Option<ActiveHyperlink>, active_sgr: &ActiveSgr) -> String {
    let mut prefix = active_hyperlink
        .as_ref()
        .map(format_osc8_hyperlink)
        .unwrap_or_default();
    prefix.push_str(&active_sgr.sequence());
    prefix
}

fn line_break_suffix(active_hyperlink: &Option<ActiveHyperlink>, active_sgr: &ActiveSgr) -> String {
    let mut suffix = String::new();
    if let Some(hyperlink) = active_hyperlink {
        suffix.push_str(&format_osc8_close(hyperlink));
    }
    suffix.push_str(active_sgr.line_break_suffix());
    suffix
}

fn is_sgr_code(ansi_code: &str) -> bool {
    ansi_code.starts_with("\x1b[") && ansi_code.ends_with('m')
}

#[derive(Default)]
struct ActiveSgr {
    foreground: Option<String>,
    background: Option<String>,
    underline: bool,
    other: Vec<String>,
}

impl ActiveSgr {
    fn apply(&mut self, sgr_code: &str) {
        let Some(params) = sgr_code
            .strip_prefix("\x1b[")
            .and_then(|code| code.strip_suffix('m'))
        else {
            return;
        };

        let params = if params.is_empty() { "0" } else { params };
        let values = params
            .split(';')
            .filter_map(|param| param.parse::<u16>().ok())
            .collect::<Vec<_>>();

        if sgr_values_reset_all(&values) {
            self.clear();
            return;
        }

        let mut index = 0;
        while index < values.len() {
            let value = values[index];
            match value {
                4 => self.underline = true,
                24 => self.underline = false,
                30..=37 | 90..=97 => self.foreground = Some(sgr_code.to_string()),
                38 => {
                    self.foreground = Some(sgr_code.to_string());
                    index += extended_sgr_color_param_len(&values[index..]);
                }
                39 => self.foreground = None,
                40..=47 | 100..=107 => self.background = Some(sgr_code.to_string()),
                48 => {
                    self.background = Some(sgr_code.to_string());
                    index += extended_sgr_color_param_len(&values[index..]);
                }
                49 => self.background = None,
                _ => self.other.push(sgr_code.to_string()),
            }
            index += 1;
        }
    }

    fn sequence(&self) -> String {
        let mut sequence = String::new();
        for code in &self.other {
            sequence.push_str(code);
        }
        if let Some(foreground) = &self.foreground {
            sequence.push_str(foreground);
        }
        if let Some(background) = &self.background {
            sequence.push_str(background);
        }
        if self.underline {
            sequence.push_str("\x1b[4m");
        }
        sequence
    }

    fn line_break_suffix(&self) -> &'static str {
        if self.underline {
            "\x1b[24m"
        } else {
            ""
        }
    }

    fn clear(&mut self) {
        self.foreground = None;
        self.background = None;
        self.underline = false;
        self.other.clear();
    }
}

fn sgr_values_reset_all(values: &[u16]) -> bool {
    let mut index = 0;
    while index < values.len() {
        match values[index] {
            0 => return true,
            38 | 48 => {
                index += extended_sgr_color_param_len(&values[index..]) + 1;
                continue;
            }
            _ => index += 1,
        }
    }
    false
}

fn extended_sgr_color_param_len(values: &[u16]) -> usize {
    match values {
        [38 | 48, 2, _, _, _, ..] => 4,
        [38 | 48, 5, _, ..] => 2,
        _ => 0,
    }
}

fn parse_osc8_hyperlink(ansi_code: &str) -> Option<Option<ActiveHyperlink>> {
    if !ansi_code.starts_with("\x1b]8;") {
        return None;
    }

    let terminator = if ansi_code.ends_with('\x07') {
        "\x07"
    } else if ansi_code.ends_with("\x1b\\") {
        "\x1b\\"
    } else {
        return None;
    };
    let terminator_len = terminator.len();
    let body = &ansi_code[4..ansi_code.len().saturating_sub(terminator_len)];
    let separator = body.find(';')?;
    let params = &body[..separator];
    let url = &body[separator + 1..];
    if url.is_empty() {
        return Some(None);
    }
    Some(Some(ActiveHyperlink {
        params: params.to_string(),
        url: url.to_string(),
        terminator,
    }))
}

fn format_osc8_hyperlink(hyperlink: &ActiveHyperlink) -> String {
    format!(
        "\x1b]8;{};{}{}",
        hyperlink.params, hyperlink.url, hyperlink.terminator
    )
}

fn format_osc8_close(hyperlink: &ActiveHyperlink) -> String {
    format!("\x1b]8;;{}", hyperlink.terminator)
}

fn truncate_fragment_to_width(text: &str, max_width: usize) -> SliceWithWidth {
    if max_width == 0 || text.is_empty() {
        return SliceWithWidth {
            text: String::new(),
            width: 0,
        };
    }

    if is_printable_ascii(text) {
        let clipped: String = text.chars().take(max_width).collect();
        let width = clipped.len();
        return SliceWithWidth {
            text: clipped,
            width,
        };
    }

    let mut result = String::new();
    let mut width = 0;
    let mut pending_ansi = String::new();
    let mut i = 0;

    while i < text.len() {
        if let Some(ansi) = extract_ansi_code(text, i) {
            pending_ansi.push_str(&ansi.code);
            i += ansi.length;
            continue;
        }

        let (segment, next_i) = next_text_segment(text, i);
        if width + segment.width > max_width {
            break;
        }

        if !pending_ansi.is_empty() {
            result.push_str(&pending_ansi);
            pending_ansi.clear();
        }
        result.push_str(&segment.text);
        width += segment.width;
        i = next_i;
    }

    SliceWithWidth {
        text: result,
        width,
    }
}

fn finalize_truncated_result(
    prefix: &str,
    prefix_width: usize,
    ellipsis: &str,
    ellipsis_width: usize,
    max_width: usize,
    pad: bool,
) -> String {
    let visible = prefix_width + ellipsis_width;
    let mut result = if ellipsis.is_empty() {
        format!("{prefix}{RESET}")
    } else {
        format!("{prefix}{RESET}{ellipsis}{RESET}")
    };

    if pad {
        result.push_str(&" ".repeat(max_width.saturating_sub(visible)));
    }

    result
}

fn segments_without_ansi(text: &str) -> Vec<TextSegment> {
    let mut segments = Vec::new();
    let mut i = 0;

    while i < text.len() {
        if let Some(ansi) = extract_ansi_code(text, i) {
            i += ansi.length;
            continue;
        }
        let (segment, next_i) = next_text_segment(text, i);
        segments.push(segment);
        i = next_i;
    }

    segments
}

fn next_text_segment(text: &str, start: usize) -> (TextSegment, usize) {
    let first = text[start..].chars().next().expect("valid segment start");

    if first == '\t' {
        return (
            TextSegment {
                text: first.to_string(),
                width: 3,
            },
            start + first.len_utf8(),
        );
    }

    if is_regional_indicator(first) {
        let mut end = start + first.len_utf8();
        if let Some(next) = text[end..].chars().next() {
            if is_regional_indicator(next) {
                end += next.len_utf8();
            }
        }
        return (
            TextSegment {
                text: text[start..end].to_string(),
                width: 2,
            },
            end,
        );
    }

    if could_be_emoji(first) {
        let mut end = start + first.len_utf8();
        consume_emoji_suffix(text, &mut end);
        return (
            TextSegment {
                text: text[start..end].to_string(),
                width: 2,
            },
            end,
        );
    }

    let mut end = start + first.len_utf8();
    while let Some(next) = text[end..].chars().next() {
        if is_combining_suffix(next) {
            end += next.len_utf8();
        } else {
            break;
        }
    }

    (
        TextSegment {
            text: text[start..end].to_string(),
            width: char_width(first),
        },
        end,
    )
}

fn consume_emoji_suffix(text: &str, end: &mut usize) {
    loop {
        let Some(next) = text[*end..].chars().next() else {
            break;
        };

        if next == '\u{200d}' {
            *end += next.len_utf8();
            if let Some(joined) = text[*end..].chars().next() {
                *end += joined.len_utf8();
                continue;
            }
        }

        if is_variation_selector(next) || is_skin_tone_modifier(next) || is_combining_suffix(next) {
            *end += next.len_utf8();
            continue;
        }

        break;
    }
}

fn char_width(ch: char) -> usize {
    let cp = ch as u32;

    if ch == '\t' {
        return 3;
    }
    if is_mark_or_ignorable(ch) {
        return 0;
    }
    if cp == 0x0e33 || cp == 0x0eb3 {
        return 1;
    }
    if is_wide_codepoint(cp) {
        return 2;
    }
    1
}

fn is_printable_ascii(text: &str) -> bool {
    text.bytes().all(|byte| (0x20..=0x7e).contains(&byte))
}

fn is_mark_or_ignorable(ch: char) -> bool {
    let cp = ch as u32;
    cp <= 0x1f
        || cp == 0x7f
        || (0x0300..=0x036f).contains(&cp)
        || (0x0483..=0x0489).contains(&cp)
        || (0x0591..=0x05bd).contains(&cp)
        || cp == 0x05bf
        || (0x05c1..=0x05c2).contains(&cp)
        || (0x05c4..=0x05c5).contains(&cp)
        || cp == 0x05c7
        || (0x0610..=0x061a).contains(&cp)
        || (0x064b..=0x065f).contains(&cp)
        || cp == 0x0670
        || (0x06d6..=0x06dc).contains(&cp)
        || (0x06df..=0x06e4).contains(&cp)
        || (0x06e7..=0x06e8).contains(&cp)
        || (0x06ea..=0x06ed).contains(&cp)
        || (0x0711..=0x0711).contains(&cp)
        || (0x0730..=0x074a).contains(&cp)
        || (0x07a6..=0x07b0).contains(&cp)
        || (0x07eb..=0x07f3).contains(&cp)
        || (0x0816..=0x0819).contains(&cp)
        || (0x081b..=0x0823).contains(&cp)
        || (0x0825..=0x0827).contains(&cp)
        || (0x0829..=0x082d).contains(&cp)
        || (0x0859..=0x085b).contains(&cp)
        || (0x08d3..=0x08e1).contains(&cp)
        || (0x08e3..=0x0903).contains(&cp)
        || (0x093a..=0x093c).contains(&cp)
        || (0x0941..=0x0948).contains(&cp)
        || (0x094d..=0x094d).contains(&cp)
        || (0x0951..=0x0957).contains(&cp)
        || (0x0962..=0x0963).contains(&cp)
        || (0x0981..=0x0981).contains(&cp)
        || (0x09bc..=0x09bc).contains(&cp)
        || (0x09c1..=0x09c4).contains(&cp)
        || (0x09cd..=0x09cd).contains(&cp)
        || (0x09e2..=0x09e3).contains(&cp)
        || (0x0e31..=0x0e31).contains(&cp)
        || (0x0e34..=0x0e3a).contains(&cp)
        || (0x0e47..=0x0e4e).contains(&cp)
        || (0x0eb1..=0x0eb1).contains(&cp)
        || (0x0eb4..=0x0ebc).contains(&cp)
        || (0x0ec8..=0x0ecd).contains(&cp)
        || (0x200b..=0x200f).contains(&cp)
        || (0x202a..=0x202e).contains(&cp)
        || (0x2060..=0x206f).contains(&cp)
        || is_variation_selector(ch)
}

fn is_combining_suffix(ch: char) -> bool {
    let cp = ch as u32;
    (0x0300..=0x036f).contains(&cp)
        || (0x0483..=0x0489).contains(&cp)
        || (0x0591..=0x05bd).contains(&cp)
        || cp == 0x05bf
        || (0x05c1..=0x05c2).contains(&cp)
        || (0x05c4..=0x05c5).contains(&cp)
        || cp == 0x05c7
        || (0x0610..=0x061a).contains(&cp)
        || (0x064b..=0x065f).contains(&cp)
        || cp == 0x0670
        || (0x06d6..=0x06dc).contains(&cp)
        || (0x06df..=0x06e4).contains(&cp)
        || (0x06e7..=0x06e8).contains(&cp)
        || (0x06ea..=0x06ed).contains(&cp)
        || (0x0711..=0x0711).contains(&cp)
        || (0x0730..=0x074a).contains(&cp)
        || (0x07a6..=0x07b0).contains(&cp)
        || (0x07eb..=0x07f3).contains(&cp)
        || (0x0816..=0x0819).contains(&cp)
        || (0x081b..=0x0823).contains(&cp)
        || (0x0825..=0x0827).contains(&cp)
        || (0x0829..=0x082d).contains(&cp)
        || (0x0859..=0x085b).contains(&cp)
        || (0x08d3..=0x08e1).contains(&cp)
        || (0x08e3..=0x0903).contains(&cp)
        || (0x093a..=0x093c).contains(&cp)
        || (0x0941..=0x0948).contains(&cp)
        || (0x094d..=0x094d).contains(&cp)
        || (0x0951..=0x0957).contains(&cp)
        || (0x0962..=0x0963).contains(&cp)
        || (0x0981..=0x0981).contains(&cp)
        || (0x09bc..=0x09bc).contains(&cp)
        || (0x09c1..=0x09c4).contains(&cp)
        || (0x09cd..=0x09cd).contains(&cp)
        || (0x09e2..=0x09e3).contains(&cp)
        || (0x0e31..=0x0e31).contains(&cp)
        || (0x0e34..=0x0e3a).contains(&cp)
        || (0x0e47..=0x0e4e).contains(&cp)
        || (0x0eb1..=0x0eb1).contains(&cp)
        || (0x0eb4..=0x0ebc).contains(&cp)
        || (0x0ec8..=0x0ecd).contains(&cp)
        || is_variation_selector(ch)
}

fn is_wide_codepoint(cp: u32) -> bool {
    (0x1100..=0x115f).contains(&cp)
        || cp == 0x2329
        || cp == 0x232a
        || (0x2e80..=0xa4cf).contains(&cp)
        || (0xac00..=0xd7a3).contains(&cp)
        || (0xf900..=0xfaff).contains(&cp)
        || (0xfe10..=0xfe19).contains(&cp)
        || (0xfe30..=0xfe6f).contains(&cp)
        || (0xff00..=0xff60).contains(&cp)
        || (0xffe0..=0xffe6).contains(&cp)
        || (0x20000..=0x3fffd).contains(&cp)
}

fn is_variation_selector(ch: char) -> bool {
    let cp = ch as u32;
    (0xfe00..=0xfe0f).contains(&cp) || (0xe0100..=0xe01ef).contains(&cp)
}

fn is_skin_tone_modifier(ch: char) -> bool {
    let cp = ch as u32;
    (0x1f3fb..=0x1f3ff).contains(&cp)
}

fn is_regional_indicator(ch: char) -> bool {
    let cp = ch as u32;
    (0x1f1e6..=0x1f1ff).contains(&cp)
}

fn could_be_emoji(ch: char) -> bool {
    let cp = ch as u32;
    (0x1f000..=0x1fbff).contains(&cp)
        || (0x2300..=0x23ff).contains(&cp)
        || (0x2600..=0x27bf).contains(&cp)
        || (0x2b50..=0x2b55).contains(&cp)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn visible_width_counts_tabs_and_skips_ansi() {
        assert_eq!(visible_width("\t\x1b[31m界\x1b[0m"), 5);
    }

    #[test]
    fn visible_width_handles_thai_and_lao_am() {
        assert_eq!(visible_width("ำ"), 1);
        assert_eq!(visible_width("ຳ"), 1);
        assert_eq!(visible_width("กำ"), 2);
        assert_eq!(visible_width("ກຳ"), 2);
        assert_eq!(normalize_terminal_output("ำ"), "ํา");
        assert_eq!(normalize_terminal_output("ຳ"), "ໍາ");
        assert_eq!(visible_width(&normalize_terminal_output("ำabc")), 4);
    }

    #[test]
    fn regional_indicators_and_common_emoji_are_stable_width() {
        assert_eq!(visible_width("🇨"), 2);
        assert_eq!(visible_width("      - 🇨"), 10);
        for cp in 0x1f1e6..=0x1f1ff {
            let ch = char::from_u32(cp).expect("regional indicator");
            assert_eq!(visible_width(&ch.to_string()), 2);
        }
        for sample in ["🇯🇵", "🇺🇸", "👍", "👍🏻", "✅", "⚡", "⚡️", "👨", "👨‍💻", "🏳️‍🌈"]
        {
            assert_eq!(visible_width(sample), 2, "{sample}");
        }
    }

    #[test]
    fn wraps_partial_flag_before_overflow() {
        let wrapped = wrap_text_with_ansi("      - 🇨", 9);
        assert_eq!(wrapped.len(), 2);
        assert_eq!(visible_width(&wrapped[0]), 7);
        assert_eq!(visible_width(&wrapped[1]), 2);
    }

    #[test]
    fn wrap_text_reopens_osc8_hyperlinks_across_lines_like_pi() {
        let wrapped = wrap_text_with_ansi("\x1b]8;;https://example.com\x07abcdef", 3);

        assert_eq!(
            wrapped,
            vec![
                "\x1b]8;;https://example.com\x07abc\x1b]8;;\x07".to_string(),
                "\x1b]8;;https://example.com\x07def".to_string(),
            ]
        );
    }

    #[test]
    fn wrap_text_preserves_osc8_terminator_style_like_pi() {
        let url = format!("https://example.com/oauth/{}", "a".repeat(32));
        let wrapped = wrap_text_with_ansi(&format!("\x1b]8;;{url}\x07{url}\x1b]8;;\x07"), 20);

        assert!(wrapped.len() > 1);
        for line in &wrapped {
            assert!(
                line.contains(&format!("\x1b]8;;{url}\x07")),
                "line should reopen the hyperlink with BEL: {line:?}"
            );
            assert!(
                !line.contains(&format!("\x1b]8;;{url}\x1b\\")),
                "line should not rewrite BEL hyperlinks to ST: {line:?}"
            );
        }
        for line in wrapped.iter().take(wrapped.len().saturating_sub(1)) {
            assert!(
                line.ends_with("\x1b]8;;\x07"),
                "wrapped hyperlink line should close with BEL: {line:?}"
            );
        }
    }

    #[test]
    fn wrap_text_preserves_color_across_wrapped_lines_like_pi() {
        let wrapped =
            wrap_text_with_ansi(&format!("\x1b[31m{}\x1b[0m", "hello world this is red"), 10);

        assert!(wrapped.len() > 1);
        for line in wrapped.iter().skip(1) {
            assert!(
                line.starts_with("\x1b[31m"),
                "continuation line should restart red style: {line:?}"
            );
        }
        for line in wrapped.iter().take(wrapped.len().saturating_sub(1)) {
            assert!(
                !line.ends_with("\x1b[0m"),
                "middle wrapped line should not reset all styling: {line:?}"
            );
        }
    }

    #[test]
    fn wrap_text_preserves_background_when_nested_style_wraps_like_pi() {
        let wrapped = wrap_text_with_ansi(
            "\x1b[41mprefix \x1b[4mUNDERLINED_CONTENT_THAT_WRAPS\x1b[24m suffix\x1b[0m",
            20,
        );

        assert!(wrapped.len() > 1);
        for line in &wrapped {
            let has_background = line.contains("[41m")
                || line.contains(";41m")
                || line.contains("[41;")
                || line.contains(";41;");
            assert!(
                has_background,
                "wrapped line should preserve background style: {line:?}"
            );
        }
        for line in wrapped.iter().take(wrapped.len().saturating_sub(1)) {
            assert!(
                !line.ends_with("\x1b[0m"),
                "middle wrapped line should not reset all styling: {line:?}"
            );
        }
    }

    #[test]
    fn wrap_text_closes_underline_before_line_break_like_pi() {
        let wrapped = wrap_text_with_ansi(
            "\x1b[4mhttps://example.com/very/long/path/that/will/wrap\x1b[24m",
            20,
        );

        assert!(wrapped.len() > 1);
        for line in wrapped.iter().take(wrapped.len().saturating_sub(1)) {
            assert!(
                line.ends_with("\x1b[24m"),
                "underlined wrapped line should close underline before padding: {line:?}"
            );
            assert!(
                !line.ends_with("\x1b[0m"),
                "underlined wrapped line should not use full reset: {line:?}"
            );
        }
        for line in wrapped.iter().skip(1) {
            assert!(
                line.starts_with("\x1b[4m"),
                "continuation line should restart underline: {line:?}"
            );
        }
    }

    #[test]
    fn wrap_text_does_not_apply_underline_before_styled_token_like_pi() {
        let wrapped = wrap_text_with_ansi(
            "read this thread \x1b[4mhttps://example.com/very/long/path/that/will/wrap\x1b[24m",
            40,
        );

        assert_eq!(wrapped[0], "read this thread");
        assert!(
            wrapped[1].starts_with("\x1b[4m"),
            "styled token should start on the continuation line: {:?}",
            wrapped[1]
        );
        assert!(wrapped[1].contains("https://"));
    }

    #[test]
    fn wrap_text_carries_ansi_state_across_literal_newlines_like_pi() {
        let wrapped = wrap_text_with_ansi("\x1b[31mfirst\nsecond\x1b[0m", 80);

        assert_eq!(wrapped, vec!["\x1b[31mfirst", "\x1b[31msecond\x1b[0m"]);
    }

    #[test]
    fn wrap_text_preserves_truecolor_sgr_with_zero_components_like_pi() {
        let truecolor = "\x1b[38;2;255;0;0m";
        let wrapped = wrap_text_with_ansi(
            &format!("{truecolor}hello world this should wrap\x1b[0m"),
            10,
        );

        assert!(wrapped.len() > 1);
        for line in wrapped.iter().skip(1) {
            assert!(
                line.starts_with(truecolor),
                "continuation line should preserve truecolor SGR: {line:?}"
            );
        }
    }

    #[test]
    fn truncate_to_visual_lines_returns_empty_for_empty_or_blank_text() {
        assert_eq!(
            truncate_to_visual_lines("", 3, 10, 0),
            VisualTruncateResult {
                visual_lines: Vec::new(),
                skipped_count: 0,
            }
        );
        assert_eq!(
            truncate_to_visual_lines("   \n\t", 3, 10, 1),
            VisualTruncateResult {
                visual_lines: Vec::new(),
                skipped_count: 0,
            }
        );
    }

    #[test]
    fn truncate_to_visual_lines_keeps_last_wrapped_lines_like_pi_text_render() {
        let result = truncate_to_visual_lines("alpha beta gamma delta", 2, 8, 0);

        assert_eq!(
            result,
            VisualTruncateResult {
                visual_lines: vec!["gamma   ".to_string(), "delta   ".to_string()],
                skipped_count: 2,
            }
        );
        assert!(result
            .visual_lines
            .iter()
            .all(|line| visible_width(line) == 8));
    }

    #[test]
    fn truncate_to_visual_lines_applies_horizontal_padding_before_wrapping() {
        let result = truncate_to_visual_lines("abcdef", 2, 6, 1);

        assert_eq!(
            result,
            VisualTruncateResult {
                visual_lines: vec![" abcd ".to_string(), " ef   ".to_string()],
                skipped_count: 0,
            }
        );
    }

    #[test]
    fn truncate_to_width_keeps_bounds_and_resets() {
        let truncated = truncate_to_width(&"🙂界".repeat(1000), 40, "…", false);
        assert!(visible_width(&truncated) <= 40);
        assert!(truncated.ends_with("…\x1b[0m"));

        let styled = truncate_to_width(
            &format!("\x1b[31m{}\x1b[0m", "hello ".repeat(100)),
            20,
            "…",
            false,
        );
        assert!(styled.contains("\x1b[31m"));
        assert!(styled.ends_with("\x1b[0m…\x1b[0m"));
    }

    #[test]
    fn truncate_to_width_clips_wide_ellipsis() {
        assert_eq!(truncate_to_width("abcdef", 1, "🙂", false), "");
        assert_eq!(
            truncate_to_width("abcdef", 2, "🙂", false),
            "\x1b[0m🙂\x1b[0m"
        );
        assert_eq!(truncate_to_width("a", 2, "🙂", false), "a");
        assert_eq!(truncate_to_width("界", 2, "🙂", false), "界");
    }

    #[test]
    fn truncate_to_width_pads_and_keeps_contiguous_prefix() {
        let padded = truncate_to_width("🙂界🙂界🙂界", 8, "…", true);
        assert_eq!(visible_width(&padded), 8);

        let truncated = truncate_to_width("🙂\t界 \x1b_abc\x07", 7, "…", true);
        assert_eq!(truncated, "🙂\t\x1b[0m…\x1b[0m ");
    }

    #[test]
    fn slice_columns_preserves_pending_ansi() {
        let sliced = slice_with_width("\x1b[31mabc界", 2, 3, true);
        assert_eq!(sliced.text, "\x1b[31mc界");
        assert_eq!(sliced.width, 3);
    }

    #[test]
    fn extract_segments_preserves_before_and_inherited_after_style_like_pi() {
        let segments = extract_segments("\x1b[31mabcde", 2, 4, 1, true);

        assert_eq!(segments.before, "\x1b[31mab");
        assert_eq!(segments.before_width, 2);
        assert_eq!(segments.after, "\x1b[31me");
        assert_eq!(segments.after_width, 1);
    }
}
