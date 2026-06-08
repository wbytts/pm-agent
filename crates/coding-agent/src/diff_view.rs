#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiffSpan {
    Text(String),
    Inverse(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiffLine {
    Context(Vec<DiffSpan>),
    Removed(Vec<DiffSpan>),
    Added(Vec<DiffSpan>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedDiffLine {
    prefix: char,
    line_num: String,
    content: String,
}

pub fn render_diff(diff_text: &str) -> Vec<DiffLine> {
    let lines = diff_text.split('\n').collect::<Vec<_>>();
    let mut result = Vec::new();
    let mut index = 0;

    while index < lines.len() {
        let line = lines[index];
        let Some(parsed) = parse_diff_line(line) else {
            result.push(DiffLine::Context(vec![DiffSpan::Text(replace_tabs(line))]));
            index += 1;
            continue;
        };

        match parsed.prefix {
            '-' => {
                let mut removed = Vec::new();
                while index < lines.len() {
                    let Some(parsed) = parse_diff_line(lines[index]) else {
                        break;
                    };
                    if parsed.prefix != '-' {
                        break;
                    }
                    removed.push(parsed);
                    index += 1;
                }

                let mut added = Vec::new();
                while index < lines.len() {
                    let Some(parsed) = parse_diff_line(lines[index]) else {
                        break;
                    };
                    if parsed.prefix != '+' {
                        break;
                    }
                    added.push(parsed);
                    index += 1;
                }

                if removed.len() == 1 && added.len() == 1 {
                    let (removed_spans, added_spans) =
                        render_intra_line_diff(&removed[0].content, &added[0].content);
                    result.push(DiffLine::Removed(with_prefix(
                        '-',
                        &removed[0].line_num,
                        removed_spans,
                    )));
                    result.push(DiffLine::Added(with_prefix(
                        '+',
                        &added[0].line_num,
                        added_spans,
                    )));
                } else {
                    for line in removed {
                        result.push(DiffLine::Removed(vec![DiffSpan::Text(format!(
                            "-{} {}",
                            line.line_num,
                            replace_tabs(&line.content)
                        ))]));
                    }
                    for line in added {
                        result.push(DiffLine::Added(vec![DiffSpan::Text(format!(
                            "+{} {}",
                            line.line_num,
                            replace_tabs(&line.content)
                        ))]));
                    }
                }
            }
            '+' => {
                result.push(DiffLine::Added(vec![DiffSpan::Text(format!(
                    "+{} {}",
                    parsed.line_num,
                    replace_tabs(&parsed.content)
                ))]));
                index += 1;
            }
            _ => {
                result.push(DiffLine::Context(vec![DiffSpan::Text(format!(
                    " {} {}",
                    parsed.line_num,
                    replace_tabs(&parsed.content)
                ))]));
                index += 1;
            }
        }
    }

    result
}

fn parse_diff_line(line: &str) -> Option<ParsedDiffLine> {
    let mut chars = line.chars();
    let prefix = chars.next()?;
    if prefix != '+' && prefix != '-' && prefix != ' ' {
        return None;
    }

    let rest = chars.as_str();
    let split_index = rest.find(' ')?;
    let line_num = rest[..split_index].to_string();
    let content = rest[split_index + 1..].to_string();
    Some(ParsedDiffLine {
        prefix,
        line_num,
        content,
    })
}

fn replace_tabs(text: &str) -> String {
    text.replace('\t', "   ")
}

fn with_prefix(prefix: char, line_num: &str, mut spans: Vec<DiffSpan>) -> Vec<DiffSpan> {
    let mut result = Vec::new();
    push_text(&mut result, format!("{prefix}{line_num} "));
    for span in spans.drain(..) {
        match span {
            DiffSpan::Text(value) => push_text(&mut result, value),
            DiffSpan::Inverse(value) => push_inverse(&mut result, value),
        }
    }
    result
}

fn render_intra_line_diff(old_content: &str, new_content: &str) -> (Vec<DiffSpan>, Vec<DiffSpan>) {
    let old_content = replace_tabs(old_content);
    let new_content = replace_tabs(new_content);
    let old_words = split_words(&old_content);
    let new_words = split_words(&new_content);
    let ops = word_diff_ops(&old_words, &new_words);
    let mut removed = Vec::new();
    let mut added = Vec::new();
    let mut first_removed = true;
    let mut first_added = true;

    for op in ops {
        match op {
            WordDiffOp::Equal(parts) => {
                let value = parts.concat();
                push_text(&mut removed, value.clone());
                push_text(&mut added, value);
            }
            WordDiffOp::Delete(parts) => {
                let value = parts.concat();
                let (leading, rest) = strip_first_leading_ws(&value, &mut first_removed);
                push_text(&mut removed, leading);
                push_inverse(&mut removed, rest);
            }
            WordDiffOp::Insert(parts) => {
                let value = parts.concat();
                let (leading, rest) = strip_first_leading_ws(&value, &mut first_added);
                push_text(&mut added, leading);
                push_inverse(&mut added, rest);
            }
        }
    }

    (removed, added)
}

fn strip_first_leading_ws(value: &str, is_first: &mut bool) -> (String, String) {
    if !*is_first {
        return (String::new(), value.to_string());
    }
    *is_first = false;
    let leading_len = value
        .char_indices()
        .take_while(|(_, ch)| ch.is_whitespace())
        .map(|(index, ch)| index + ch.len_utf8())
        .last()
        .unwrap_or(0);
    (
        value[..leading_len].to_string(),
        value[leading_len..].to_string(),
    )
}

fn push_text(spans: &mut Vec<DiffSpan>, value: String) {
    if !value.is_empty() {
        if let Some(DiffSpan::Text(existing)) = spans.last_mut() {
            existing.push_str(&value);
        } else {
            spans.push(DiffSpan::Text(value));
        }
    }
}

fn push_inverse(spans: &mut Vec<DiffSpan>, value: String) {
    if !value.is_empty() {
        spans.push(DiffSpan::Inverse(value));
    }
}

fn split_words(text: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut current = String::new();
    let mut current_is_ws: Option<bool> = None;

    for ch in text.chars() {
        let is_ws = ch.is_whitespace();
        if current_is_ws == Some(is_ws) || current_is_ws.is_none() {
            current.push(ch);
            current_is_ws = Some(is_ws);
        } else {
            result.push(std::mem::take(&mut current));
            current.push(ch);
            current_is_ws = Some(is_ws);
        }
    }
    if !current.is_empty() {
        result.push(current);
    }
    result
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum WordDiffOp {
    Equal(Vec<String>),
    Delete(Vec<String>),
    Insert(Vec<String>),
}

fn word_diff_ops(old_words: &[String], new_words: &[String]) -> Vec<WordDiffOp> {
    let mut lengths = vec![vec![0usize; new_words.len() + 1]; old_words.len() + 1];
    for old_index in (0..old_words.len()).rev() {
        for new_index in (0..new_words.len()).rev() {
            lengths[old_index][new_index] = if old_words[old_index] == new_words[new_index] {
                lengths[old_index + 1][new_index + 1] + 1
            } else {
                lengths[old_index + 1][new_index].max(lengths[old_index][new_index + 1])
            };
        }
    }

    let mut ops = Vec::new();
    let mut old_index = 0;
    let mut new_index = 0;
    while old_index < old_words.len() && new_index < new_words.len() {
        if old_words[old_index] == new_words[new_index] {
            push_word_op(
                &mut ops,
                WordDiffOp::Equal(vec![old_words[old_index].clone()]),
            );
            old_index += 1;
            new_index += 1;
        } else if lengths[old_index + 1][new_index] >= lengths[old_index][new_index + 1] {
            push_word_op(
                &mut ops,
                WordDiffOp::Delete(vec![old_words[old_index].clone()]),
            );
            old_index += 1;
        } else {
            push_word_op(
                &mut ops,
                WordDiffOp::Insert(vec![new_words[new_index].clone()]),
            );
            new_index += 1;
        }
    }
    while old_index < old_words.len() {
        push_word_op(
            &mut ops,
            WordDiffOp::Delete(vec![old_words[old_index].clone()]),
        );
        old_index += 1;
    }
    while new_index < new_words.len() {
        push_word_op(
            &mut ops,
            WordDiffOp::Insert(vec![new_words[new_index].clone()]),
        );
        new_index += 1;
    }

    ops
}

fn push_word_op(ops: &mut Vec<WordDiffOp>, op: WordDiffOp) {
    match (ops.last_mut(), op) {
        (Some(WordDiffOp::Equal(existing)), WordDiffOp::Equal(mut next))
        | (Some(WordDiffOp::Delete(existing)), WordDiffOp::Delete(mut next))
        | (Some(WordDiffOp::Insert(existing)), WordDiffOp::Insert(mut next)) => {
            existing.append(&mut next);
        }
        (_, op) => ops.push(op),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diff_view_renders_context_added_removed_and_replaces_tabs_like_pi() {
        let lines = render_diff("   ...\n 2 same\tline\n-3 old\n+3 new");

        assert_eq!(
            lines,
            vec![
                DiffLine::Context(vec![DiffSpan::Text("   ...".to_string())]),
                DiffLine::Context(vec![DiffSpan::Text(" 2 same   line".to_string())]),
                DiffLine::Removed(vec![
                    DiffSpan::Text("-3 ".to_string()),
                    DiffSpan::Inverse("old".to_string()),
                ]),
                DiffLine::Added(vec![
                    DiffSpan::Text("+3 ".to_string()),
                    DiffSpan::Inverse("new".to_string()),
                ]),
            ]
        );
    }

    #[test]
    fn diff_view_marks_only_changed_words_for_single_line_replacement() {
        let lines = render_diff("-12 alpha beta gamma\n+12 alpha delta gamma");

        assert_eq!(
            lines,
            vec![
                DiffLine::Removed(vec![
                    DiffSpan::Text("-12 alpha ".to_string()),
                    DiffSpan::Inverse("beta".to_string()),
                    DiffSpan::Text(" gamma".to_string()),
                ]),
                DiffLine::Added(vec![
                    DiffSpan::Text("+12 alpha ".to_string()),
                    DiffSpan::Inverse("delta".to_string()),
                    DiffSpan::Text(" gamma".to_string()),
                ]),
            ]
        );
    }

    #[test]
    fn diff_view_does_not_intraline_highlight_multiple_removed_or_added_lines() {
        let lines = render_diff("-1 old a\n-2 old b\n+1 new a\n+2 new b");

        assert_eq!(
            lines,
            vec![
                DiffLine::Removed(vec![DiffSpan::Text("-1 old a".to_string())]),
                DiffLine::Removed(vec![DiffSpan::Text("-2 old b".to_string())]),
                DiffLine::Added(vec![DiffSpan::Text("+1 new a".to_string())]),
                DiffLine::Added(vec![DiffSpan::Text("+2 new b".to_string())]),
            ]
        );
    }
}
