use crate::components::CURSOR_MARKER;
use crate::{
    is_image_line, normalize_terminal_output, slice_by_column, slice_with_width, visible_width,
};
use std::collections::BTreeSet;

const SEGMENT_RESET: &str = "\x1b[0m";
const LINE_RESET: &str = "\x1b[0m\x1b]8;;\x07";
const KITTY_SEQUENCE_PREFIX: &str = "\x1b_G";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CursorPosition {
    pub row: usize,
    pub col: usize,
}

pub fn extract_cursor_position(lines: &mut [String], height: usize) -> Option<CursorPosition> {
    let viewport_top = lines.len().saturating_sub(height);
    for row in (viewport_top..lines.len()).rev() {
        let line = &lines[row];
        let Some(marker_index) = line.find(CURSOR_MARKER) else {
            continue;
        };

        let col = visible_width(&line[..marker_index]);
        lines[row].replace_range(marker_index..marker_index + CURSOR_MARKER.len(), "");
        return Some(CursorPosition { row, col });
    }

    None
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SizeValue {
    Cells(usize),
    Percent(f64),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlayAnchor {
    Center,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
    TopCenter,
    BottomCenter,
    LeftCenter,
    RightCenter,
}

impl Default for OverlayAnchor {
    fn default() -> Self {
        Self::Center
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlayMargin {
    All(isize),
    Each {
        top: isize,
        right: isize,
        bottom: isize,
        left: isize,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct OverlayOptions {
    pub width: Option<SizeValue>,
    pub min_width: Option<usize>,
    pub max_height: Option<SizeValue>,
    pub anchor: Option<OverlayAnchor>,
    pub offset_x: Option<isize>,
    pub offset_y: Option<isize>,
    pub row: Option<SizeValue>,
    pub col: Option<SizeValue>,
    pub margin: Option<OverlayMargin>,
    pub non_capturing: bool,
}

impl Default for OverlayOptions {
    fn default() -> Self {
        Self {
            width: None,
            min_width: None,
            max_height: None,
            anchor: None,
            offset_x: None,
            offset_y: None,
            row: None,
            col: None,
            margin: None,
            non_capturing: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OverlayLayout {
    pub width: usize,
    pub row: usize,
    pub col: usize,
    pub max_height: Option<usize>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RenderedOverlay {
    pub lines: Vec<String>,
    pub options: OverlayOptions,
    pub focus_order: usize,
}

pub fn resolve_overlay_layout(
    options: Option<&OverlayOptions>,
    overlay_height: usize,
    term_width: usize,
    term_height: usize,
) -> OverlayLayout {
    let options = options.cloned().unwrap_or_default();
    let (margin_top, margin_right, margin_bottom, margin_left) = resolve_margin(options.margin);

    let avail_width = term_width
        .saturating_sub(margin_left)
        .saturating_sub(margin_right)
        .max(1);
    let avail_height = term_height
        .saturating_sub(margin_top)
        .saturating_sub(margin_bottom)
        .max(1);

    let mut width = parse_size_value(options.width, term_width).unwrap_or(avail_width.min(80));
    if let Some(min_width) = options.min_width {
        width = width.max(min_width);
    }
    width = width.clamp(1, avail_width);

    let max_height = parse_size_value(options.max_height, term_height)
        .map(|height| height.clamp(1, avail_height));
    let effective_height = max_height
        .map(|height| overlay_height.min(height))
        .unwrap_or(overlay_height);

    let anchor = options.anchor.unwrap_or_default();
    let mut row = match options.row {
        Some(SizeValue::Cells(row)) => row as isize,
        Some(SizeValue::Percent(percent)) => {
            let max_row = avail_height.saturating_sub(effective_height);
            margin_top as isize + ((max_row as f64) * (percent / 100.0)).floor() as isize
        }
        None => resolve_anchor_row(anchor, effective_height, avail_height, margin_top) as isize,
    };
    let mut col = match options.col {
        Some(SizeValue::Cells(col)) => col as isize,
        Some(SizeValue::Percent(percent)) => {
            let max_col = avail_width.saturating_sub(width);
            margin_left as isize + ((max_col as f64) * (percent / 100.0)).floor() as isize
        }
        None => resolve_anchor_col(anchor, width, avail_width, margin_left) as isize,
    };

    row += options.offset_y.unwrap_or(0);
    col += options.offset_x.unwrap_or(0);

    let min_row = margin_top as isize;
    let max_row = term_height
        .saturating_sub(margin_bottom)
        .saturating_sub(effective_height) as isize;
    let min_col = margin_left as isize;
    let max_col = term_width
        .saturating_sub(margin_right)
        .saturating_sub(width) as isize;

    OverlayLayout {
        width,
        row: row.clamp(min_row, max_row).max(0) as usize,
        col: col.clamp(min_col, max_col).max(0) as usize,
        max_height,
    }
}

fn parse_size_value(value: Option<SizeValue>, reference_size: usize) -> Option<usize> {
    match value {
        Some(SizeValue::Cells(cells)) => Some(cells),
        Some(SizeValue::Percent(percent)) => {
            Some(((reference_size as f64) * (percent / 100.0)).floor() as usize)
        }
        None => None,
    }
}

fn resolve_margin(margin: Option<OverlayMargin>) -> (usize, usize, usize, usize) {
    let clamp = |value: isize| value.max(0) as usize;
    match margin {
        Some(OverlayMargin::All(value)) => {
            let value = clamp(value);
            (value, value, value, value)
        }
        Some(OverlayMargin::Each {
            top,
            right,
            bottom,
            left,
        }) => (clamp(top), clamp(right), clamp(bottom), clamp(left)),
        None => (0, 0, 0, 0),
    }
}

fn resolve_anchor_row(
    anchor: OverlayAnchor,
    height: usize,
    avail_height: usize,
    margin_top: usize,
) -> usize {
    match anchor {
        OverlayAnchor::TopLeft | OverlayAnchor::TopCenter | OverlayAnchor::TopRight => margin_top,
        OverlayAnchor::BottomLeft | OverlayAnchor::BottomCenter | OverlayAnchor::BottomRight => {
            margin_top + avail_height.saturating_sub(height)
        }
        OverlayAnchor::LeftCenter | OverlayAnchor::Center | OverlayAnchor::RightCenter => {
            margin_top + avail_height.saturating_sub(height) / 2
        }
    }
}

fn resolve_anchor_col(
    anchor: OverlayAnchor,
    width: usize,
    avail_width: usize,
    margin_left: usize,
) -> usize {
    match anchor {
        OverlayAnchor::TopLeft | OverlayAnchor::LeftCenter | OverlayAnchor::BottomLeft => {
            margin_left
        }
        OverlayAnchor::TopRight | OverlayAnchor::RightCenter | OverlayAnchor::BottomRight => {
            margin_left + avail_width.saturating_sub(width)
        }
        OverlayAnchor::TopCenter | OverlayAnchor::Center | OverlayAnchor::BottomCenter => {
            margin_left + avail_width.saturating_sub(width) / 2
        }
    }
}

pub fn composite_line_at(
    base_line: &str,
    overlay_line: &str,
    start_col: usize,
    overlay_width: usize,
    total_width: usize,
) -> String {
    if is_image_line(base_line) {
        return base_line.to_string();
    }

    let after_start = start_col.saturating_add(overlay_width);
    let base = extract_overlay_segments(
        base_line,
        start_col,
        after_start,
        total_width.saturating_sub(after_start),
    );
    let overlay = slice_with_width(overlay_line, 0, overlay_width, true);

    let before_pad = start_col.saturating_sub(base.before_width);
    let overlay_pad = overlay_width.saturating_sub(overlay.width);
    let actual_before_width = start_col.max(base.before_width);
    let actual_overlay_width = overlay_width.max(overlay.width);
    let after_target = total_width
        .saturating_sub(actual_before_width)
        .saturating_sub(actual_overlay_width);
    let after_pad = after_target.saturating_sub(base.after_width);

    let mut result = String::new();
    result.push_str(&base.before);
    result.push_str(&" ".repeat(before_pad));
    result.push_str(SEGMENT_RESET);
    result.push_str(&overlay.text);
    result.push_str(&" ".repeat(overlay_pad));
    result.push_str(SEGMENT_RESET);
    result.push_str(&base.after);
    result.push_str(&" ".repeat(after_pad));

    if visible_width(&result) <= total_width {
        ensure_trailing_sgr_reset(result)
    } else {
        ensure_trailing_sgr_reset(slice_by_column(&result, 0, total_width, true))
    }
}

fn ensure_trailing_sgr_reset(mut line: String) -> String {
    if !line.ends_with(SEGMENT_RESET) {
        line.push_str(SEGMENT_RESET);
    }
    line
}

pub fn composite_overlays(
    lines: Vec<String>,
    overlays: &[RenderedOverlay],
    term_width: usize,
    term_height: usize,
) -> Vec<String> {
    if overlays.is_empty() {
        return lines;
    }

    let mut result = lines;
    let mut rendered = Vec::new();
    let mut min_lines_needed = result.len();
    let mut sorted_overlays = overlays.iter().collect::<Vec<_>>();
    sorted_overlays.sort_by_key(|overlay| overlay.focus_order);

    for overlay in sorted_overlays {
        let initial_layout =
            resolve_overlay_layout(Some(&overlay.options), 0, term_width, term_height);
        let mut overlay_lines = overlay.lines.clone();
        if let Some(max_height) = initial_layout.max_height {
            overlay_lines.truncate(max_height);
        }

        let final_layout = resolve_overlay_layout(
            Some(&overlay.options),
            overlay_lines.len(),
            term_width,
            term_height,
        );
        min_lines_needed = min_lines_needed.max(final_layout.row + overlay_lines.len());
        rendered.push((
            overlay_lines,
            final_layout.row,
            final_layout.col,
            final_layout.width,
        ));
    }

    let working_height = result.len().max(term_height).max(min_lines_needed);
    while result.len() < working_height {
        result.push(String::new());
    }
    let viewport_start = working_height.saturating_sub(term_height);

    for (overlay_lines, row, col, width) in rendered {
        for (line_index, overlay_line) in overlay_lines.into_iter().enumerate() {
            let target_index = viewport_start + row + line_index;
            if target_index >= result.len() {
                continue;
            }

            let truncated_overlay = if visible_width(&overlay_line) > width {
                slice_by_column(&overlay_line, 0, width, true)
            } else {
                overlay_line
            };
            result[target_index] = composite_line_at(
                &result[target_index],
                &truncated_overlay,
                col,
                width,
                term_width,
            );
        }
    }

    result
}

pub fn apply_line_resets(lines: Vec<String>) -> Vec<String> {
    lines
        .into_iter()
        .map(|line| {
            if is_image_line(&line) {
                line
            } else {
                format!("{}{}", normalize_terminal_output(&line), LINE_RESET)
            }
        })
        .collect()
}

pub fn extract_kitty_image_ids(line: &str) -> Vec<u32> {
    let Some(sequence_start) = line.find(KITTY_SEQUENCE_PREFIX) else {
        return Vec::new();
    };

    let params_start = sequence_start + KITTY_SEQUENCE_PREFIX.len();
    let Some(params_end_offset) = line[params_start..].find(';') else {
        return Vec::new();
    };
    let params_end = params_start + params_end_offset;
    let params = &line[params_start..params_end];

    for param in params.split(',') {
        let Some((key, value)) = param.split_once('=') else {
            continue;
        };
        if key != "i" {
            continue;
        }
        let Ok(id) = value.parse::<u64>() else {
            continue;
        };
        if id > 0 && id <= u32::MAX as u64 {
            return vec![id as u32];
        }
    }

    Vec::new()
}

pub fn collect_kitty_image_ids(lines: &[String]) -> Vec<u32> {
    let mut ids = BTreeSet::new();
    for line in lines {
        for id in extract_kitty_image_ids(line) {
            ids.insert(id);
        }
    }
    ids.into_iter().collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct OverlaySegments {
    before: String,
    before_width: usize,
    after: String,
    after_width: usize,
}

fn extract_overlay_segments(
    line: &str,
    before_end: usize,
    after_start: usize,
    after_len: usize,
) -> OverlaySegments {
    let before = slice_by_column(line, 0, before_end, false);
    let after = slice_with_width(line, after_start, after_len, true);

    OverlaySegments {
        before_width: visible_width(&before),
        before,
        after_width: after.width,
        after: after.text,
    }
}
