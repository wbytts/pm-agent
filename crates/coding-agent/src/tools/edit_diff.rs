#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edit {
    pub old_text: String,
    pub new_text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FuzzyMatchResult {
    pub found: bool,
    pub index: usize,
    pub match_length: usize,
    pub used_fuzzy_match: bool,
    pub content_for_replacement: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppliedEditsResult {
    pub base_content: String,
    pub new_content: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffStringResult {
    pub diff: String,
    pub first_changed_line: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct MatchedEdit {
    edit_index: usize,
    match_index: usize,
    match_length: usize,
    new_text: String,
}

pub fn detect_line_ending(content: &str) -> &'static str {
    let crlf_index = content.find("\r\n");
    let lf_index = content.find('\n');
    match (crlf_index, lf_index) {
        (_, None) => "\n",
        (None, Some(_)) => "\n",
        (Some(crlf), Some(lf)) if crlf < lf => "\r\n",
        _ => "\n",
    }
}

pub fn normalize_to_lf(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

pub fn restore_line_endings(text: &str, ending: &str) -> String {
    if ending == "\r\n" {
        text.replace('\n', "\r\n")
    } else {
        text.to_string()
    }
}

pub fn strip_bom(content: &str) -> (&str, &str) {
    content
        .strip_prefix('\u{FEFF}')
        .map_or(("", content), |text| ("\u{FEFF}", text))
}

pub fn normalize_for_fuzzy_match(text: &str) -> String {
    normalize_unicode_compatibility(text)
        .split('\n')
        .map(|line| line.trim_end().to_string())
        .collect::<Vec<_>>()
        .join("\n")
        .chars()
        .map(normalize_fuzzy_char)
        .collect()
}

fn normalize_unicode_compatibility(text: &str) -> String {
    text.chars()
        .map(|ch| match ch {
            '\u{00A0}' | '\u{2002}'..='\u{200A}' | '\u{202F}' | '\u{205F}' | '\u{3000}' => ' ',
            _ => ch,
        })
        .collect()
}

fn normalize_fuzzy_char(ch: char) -> char {
    match ch {
        '\u{2018}' | '\u{2019}' | '\u{201A}' | '\u{201B}' => '\'',
        '\u{201C}' | '\u{201D}' | '\u{201E}' | '\u{201F}' => '"',
        '\u{2010}' | '\u{2011}' | '\u{2012}' | '\u{2013}' | '\u{2014}' | '\u{2015}'
        | '\u{2212}' => '-',
        _ => ch,
    }
}

pub fn fuzzy_find_text(content: &str, old_text: &str) -> FuzzyMatchResult {
    if let Some(index) = content.find(old_text) {
        return FuzzyMatchResult {
            found: true,
            index,
            match_length: old_text.len(),
            used_fuzzy_match: false,
            content_for_replacement: content.to_string(),
        };
    }

    let fuzzy_content = normalize_for_fuzzy_match(content);
    let fuzzy_old_text = normalize_for_fuzzy_match(old_text);
    if let Some(index) = fuzzy_content.find(&fuzzy_old_text) {
        return FuzzyMatchResult {
            found: true,
            index,
            match_length: fuzzy_old_text.len(),
            used_fuzzy_match: true,
            content_for_replacement: fuzzy_content,
        };
    }

    FuzzyMatchResult {
        found: false,
        index: 0,
        match_length: 0,
        used_fuzzy_match: false,
        content_for_replacement: content.to_string(),
    }
}

pub fn apply_edits_to_normalized_content(
    normalized_content: &str,
    edits: &[Edit],
    path: &str,
) -> Result<AppliedEditsResult, String> {
    let normalized_edits = edits
        .iter()
        .map(|edit| Edit {
            old_text: normalize_to_lf(&edit.old_text),
            new_text: normalize_to_lf(&edit.new_text),
        })
        .collect::<Vec<_>>();

    for (index, edit) in normalized_edits.iter().enumerate() {
        if edit.old_text.is_empty() {
            return Err(empty_old_text_error(path, index, normalized_edits.len()));
        }
    }

    let initial_matches = normalized_edits
        .iter()
        .map(|edit| fuzzy_find_text(normalized_content, &edit.old_text))
        .collect::<Vec<_>>();
    let base_content = if initial_matches.iter().any(|item| item.used_fuzzy_match) {
        normalize_for_fuzzy_match(normalized_content)
    } else {
        normalized_content.to_string()
    };

    let mut matched_edits = Vec::new();
    for (index, edit) in normalized_edits.iter().enumerate() {
        let match_result = fuzzy_find_text(&base_content, &edit.old_text);
        if !match_result.found {
            return Err(not_found_error(path, index, normalized_edits.len()));
        }

        let occurrences = count_occurrences(&base_content, &edit.old_text);
        if occurrences > 1 {
            return Err(duplicate_error(
                path,
                index,
                normalized_edits.len(),
                occurrences,
            ));
        }

        matched_edits.push(MatchedEdit {
            edit_index: index,
            match_index: match_result.index,
            match_length: match_result.match_length,
            new_text: edit.new_text.clone(),
        });
    }

    matched_edits.sort_by_key(|edit| edit.match_index);
    for pair in matched_edits.windows(2) {
        let previous = &pair[0];
        let current = &pair[1];
        if previous.match_index + previous.match_length > current.match_index {
            return Err(format!(
                "edits[{}] and edits[{}] overlap in {path}. Merge them into one edit or target disjoint regions.",
                previous.edit_index, current.edit_index
            ));
        }
    }

    let mut new_content = base_content.clone();
    for edit in matched_edits.iter().rev() {
        new_content.replace_range(
            edit.match_index..edit.match_index + edit.match_length,
            &edit.new_text,
        );
    }

    if base_content == new_content {
        return Err(no_change_error(path, normalized_edits.len()));
    }

    Ok(AppliedEditsResult {
        base_content,
        new_content,
    })
}

pub fn generate_unified_patch(path: &str, old_content: &str, new_content: &str) -> String {
    let diff = generate_diff_string(old_content, new_content, 4);
    let mut output = vec![
        format!("--- {path}"),
        format!("+++ {path}"),
        "@@".to_string(),
    ];
    if !diff.diff.is_empty() {
        output.push(diff.diff);
    }
    output.join("\n")
}

pub fn generate_diff_string(
    old_content: &str,
    new_content: &str,
    context_lines: usize,
) -> DiffStringResult {
    let old_lines = split_diff_lines(old_content);
    let new_lines = split_diff_lines(new_content);
    let ops = diff_line_ops(&old_lines, &new_lines);
    let max_line_num = old_lines.len().max(new_lines.len()).max(1);
    let line_num_width = max_line_num.to_string().len();
    let mut old_line_num = 1usize;
    let mut new_line_num = 1usize;
    let mut output = Vec::new();
    let mut first_changed_line = None;

    for (index, op) in ops.iter().enumerate() {
        match op {
            DiffOp::Equal(lines) => {
                let previous_is_change = index > 0 && ops[index - 1].is_change();
                let next_is_change = index + 1 < ops.len() && ops[index + 1].is_change();
                emit_context_lines(
                    &mut output,
                    lines,
                    &mut old_line_num,
                    &mut new_line_num,
                    line_num_width,
                    context_lines,
                    previous_is_change,
                    next_is_change,
                );
            }
            DiffOp::Insert(lines) => {
                first_changed_line.get_or_insert(new_line_num);
                for line in lines {
                    output.push(format!(
                        "+{} {}",
                        new_line_num.to_string().pad_left(line_num_width),
                        line
                    ));
                    new_line_num += 1;
                }
            }
            DiffOp::Delete(lines) => {
                first_changed_line.get_or_insert(new_line_num);
                for line in lines {
                    output.push(format!(
                        "-{} {}",
                        old_line_num.to_string().pad_left(line_num_width),
                        line
                    ));
                    old_line_num += 1;
                }
            }
        }
    }

    DiffStringResult {
        diff: output.join("\n"),
        first_changed_line,
    }
}

fn split_diff_lines(content: &str) -> Vec<String> {
    let mut lines = content
        .split('\n')
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    if lines.last().is_some_and(|line| line.is_empty()) {
        lines.pop();
    }
    lines
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum DiffOp {
    Equal(Vec<String>),
    Insert(Vec<String>),
    Delete(Vec<String>),
}

impl DiffOp {
    fn is_change(&self) -> bool {
        matches!(self, DiffOp::Insert(_) | DiffOp::Delete(_))
    }
}

fn diff_line_ops(old_lines: &[String], new_lines: &[String]) -> Vec<DiffOp> {
    let mut lcs = vec![vec![0usize; new_lines.len() + 1]; old_lines.len() + 1];
    for old_index in (0..old_lines.len()).rev() {
        for new_index in (0..new_lines.len()).rev() {
            lcs[old_index][new_index] = if old_lines[old_index] == new_lines[new_index] {
                lcs[old_index + 1][new_index + 1] + 1
            } else {
                lcs[old_index + 1][new_index].max(lcs[old_index][new_index + 1])
            };
        }
    }

    let mut old_index = 0usize;
    let mut new_index = 0usize;
    let mut ops = Vec::new();
    while old_index < old_lines.len() && new_index < new_lines.len() {
        if old_lines[old_index] == new_lines[new_index] {
            push_diff_op(&mut ops, DiffOp::Equal(vec![old_lines[old_index].clone()]));
            old_index += 1;
            new_index += 1;
        } else if lcs[old_index + 1][new_index] >= lcs[old_index][new_index + 1] {
            push_diff_op(&mut ops, DiffOp::Delete(vec![old_lines[old_index].clone()]));
            old_index += 1;
        } else {
            push_diff_op(&mut ops, DiffOp::Insert(vec![new_lines[new_index].clone()]));
            new_index += 1;
        }
    }
    while old_index < old_lines.len() {
        push_diff_op(&mut ops, DiffOp::Delete(vec![old_lines[old_index].clone()]));
        old_index += 1;
    }
    while new_index < new_lines.len() {
        push_diff_op(&mut ops, DiffOp::Insert(vec![new_lines[new_index].clone()]));
        new_index += 1;
    }
    ops
}

fn push_diff_op(ops: &mut Vec<DiffOp>, op: DiffOp) {
    match (ops.last_mut(), op) {
        (Some(DiffOp::Equal(existing)), DiffOp::Equal(mut lines))
        | (Some(DiffOp::Insert(existing)), DiffOp::Insert(mut lines))
        | (Some(DiffOp::Delete(existing)), DiffOp::Delete(mut lines)) => {
            existing.append(&mut lines)
        }
        (_, op) => ops.push(op),
    }
}

fn emit_context_lines(
    output: &mut Vec<String>,
    lines: &[String],
    old_line_num: &mut usize,
    new_line_num: &mut usize,
    line_num_width: usize,
    context_lines: usize,
    has_leading_change: bool,
    has_trailing_change: bool,
) {
    if has_leading_change && has_trailing_change {
        if lines.len() <= context_lines * 2 {
            emit_context_slice(output, lines, old_line_num, new_line_num, line_num_width);
        } else {
            emit_context_slice(
                output,
                &lines[..context_lines],
                old_line_num,
                new_line_num,
                line_num_width,
            );
            let skipped = lines.len() - context_lines * 2;
            output.push(format!(" {} ...", "".pad_left(line_num_width)));
            *old_line_num += skipped;
            *new_line_num += skipped;
            emit_context_slice(
                output,
                &lines[lines.len() - context_lines..],
                old_line_num,
                new_line_num,
                line_num_width,
            );
        }
    } else if has_leading_change {
        let shown = lines.len().min(context_lines);
        emit_context_slice(
            output,
            &lines[..shown],
            old_line_num,
            new_line_num,
            line_num_width,
        );
        let skipped = lines.len() - shown;
        if skipped > 0 {
            output.push(format!(" {} ...", "".pad_left(line_num_width)));
            *old_line_num += skipped;
            *new_line_num += skipped;
        }
    } else if has_trailing_change {
        let skipped = lines.len().saturating_sub(context_lines);
        if skipped > 0 {
            output.push(format!(" {} ...", "".pad_left(line_num_width)));
            *old_line_num += skipped;
            *new_line_num += skipped;
        }
        emit_context_slice(
            output,
            &lines[skipped..],
            old_line_num,
            new_line_num,
            line_num_width,
        );
    } else {
        *old_line_num += lines.len();
        *new_line_num += lines.len();
    }
}

fn emit_context_slice(
    output: &mut Vec<String>,
    lines: &[String],
    old_line_num: &mut usize,
    new_line_num: &mut usize,
    line_num_width: usize,
) {
    for line in lines {
        output.push(format!(
            " {} {}",
            old_line_num.to_string().pad_left(line_num_width),
            line
        ));
        *old_line_num += 1;
        *new_line_num += 1;
    }
}

trait PadLeft {
    fn pad_left(&self, width: usize) -> String;
}

impl PadLeft for str {
    fn pad_left(&self, width: usize) -> String {
        format!("{self:>width$}")
    }
}

impl PadLeft for String {
    fn pad_left(&self, width: usize) -> String {
        self.as_str().pad_left(width)
    }
}

fn count_occurrences(content: &str, old_text: &str) -> usize {
    let fuzzy_content = normalize_for_fuzzy_match(content);
    let fuzzy_old_text = normalize_for_fuzzy_match(old_text);
    fuzzy_content.matches(&fuzzy_old_text).count()
}

fn not_found_error(path: &str, edit_index: usize, total_edits: usize) -> String {
    if total_edits == 1 {
        return format!(
            "Could not find the exact text in {path}. The old text must match exactly including all whitespace and newlines."
        );
    }
    format!(
        "Could not find edits[{edit_index}] in {path}. The oldText must match exactly including all whitespace and newlines."
    )
}

fn duplicate_error(
    path: &str,
    edit_index: usize,
    total_edits: usize,
    occurrences: usize,
) -> String {
    if total_edits == 1 {
        return format!(
            "Found {occurrences} occurrences of the text in {path}. The text must be unique. Please provide more context to make it unique."
        );
    }
    format!(
        "Found {occurrences} occurrences of edits[{edit_index}] in {path}. Each oldText must be unique. Please provide more context to make it unique."
    )
}

fn empty_old_text_error(path: &str, edit_index: usize, total_edits: usize) -> String {
    if total_edits == 1 {
        return format!("oldText must not be empty in {path}.");
    }
    format!("edits[{edit_index}].oldText must not be empty in {path}.")
}

fn no_change_error(path: &str, total_edits: usize) -> String {
    if total_edits == 1 {
        return format!(
            "No changes made to {path}. The replacement produced identical content. This might indicate an issue with special characters or the text not existing as expected."
        );
    }
    format!("No changes made to {path}. The replacements produced identical content.")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_and_restores_line_endings() {
        assert_eq!(detect_line_ending("a\r\nb\n"), "\r\n");
        assert_eq!(normalize_to_lf("a\r\nb\rc"), "a\nb\nc");
        assert_eq!(restore_line_endings("a\nb", "\r\n"), "a\r\nb");
    }

    #[test]
    fn strips_utf8_bom() {
        let (bom, text) = strip_bom("\u{FEFF}hello");
        assert_eq!(bom, "\u{FEFF}");
        assert_eq!(text, "hello");
    }

    #[test]
    fn fuzzy_match_normalizes_quotes_dashes_spaces_and_trailing_whitespace() {
        let result = fuzzy_find_text("let name = “pm-agent”;\u{00A0}  ", "name = \"pm-agent\";");
        assert!(result.found);
        assert!(result.used_fuzzy_match);
    }

    #[test]
    fn applies_multiple_disjoint_edits_from_original_offsets() {
        let result = apply_edits_to_normalized_content(
            "one\ntwo\nthree\n",
            &[
                Edit {
                    old_text: "one".to_string(),
                    new_text: "1".to_string(),
                },
                Edit {
                    old_text: "three".to_string(),
                    new_text: "3".to_string(),
                },
            ],
            "demo.txt",
        )
        .expect("edits should apply");

        assert_eq!(result.new_content, "1\ntwo\n3\n");
    }

    #[test]
    fn rejects_duplicate_and_overlapping_edits() {
        let duplicate = apply_edits_to_normalized_content(
            "same\nsame\n",
            &[Edit {
                old_text: "same".to_string(),
                new_text: "other".to_string(),
            }],
            "demo.txt",
        )
        .expect_err("duplicate should fail");
        assert!(duplicate.contains("Found 2 occurrences"));

        let overlap = apply_edits_to_normalized_content(
            "abcdef",
            &[
                Edit {
                    old_text: "abc".to_string(),
                    new_text: "x".to_string(),
                },
                Edit {
                    old_text: "bcd".to_string(),
                    new_text: "y".to_string(),
                },
            ],
            "demo.txt",
        )
        .expect_err("overlap should fail");
        assert!(overlap.contains("overlap"));
    }

    #[test]
    fn generates_display_diff_with_line_numbers_and_context() {
        let diff = generate_diff_string("a\nb\nc\nd\ne\nf\n", "a\nb\nC\nd\ne\nf\n", 1);

        assert_eq!(diff.first_changed_line, Some(3));
        assert_eq!(diff.diff, "   ...\n 2 b\n-3 c\n+3 C\n 4 d\n   ...");
    }

    #[test]
    fn generates_simple_unified_patch_header() {
        let patch = generate_unified_patch("demo.txt", "old\n", "new\n");

        assert!(patch.starts_with("--- demo.txt\n+++ demo.txt\n@@\n"));
        assert!(patch.contains("-1 old"));
        assert!(patch.contains("+1 new"));
    }
}
