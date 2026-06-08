use std::cmp::Ordering;

#[derive(Debug, Clone, PartialEq)]
pub struct FuzzyMatch {
    pub matches: bool,
    pub score: f64,
}

pub fn fuzzy_match(query: &str, text: &str) -> FuzzyMatch {
    let query_lower = query.to_lowercase();
    let text_lower = text.to_lowercase();

    let primary_match = match_query(&query_lower, &text_lower);
    if primary_match.matches {
        return primary_match;
    }

    let swapped_query = swapped_alpha_numeric_query(&query_lower);
    if swapped_query.is_empty() {
        return primary_match;
    }

    let swapped_match = match_query(&swapped_query, &text_lower);
    if !swapped_match.matches {
        return primary_match;
    }

    FuzzyMatch {
        matches: true,
        score: swapped_match.score + 5.0,
    }
}

pub fn fuzzy_filter<T: Clone>(items: &[T], query: &str, get_text: impl Fn(&T) -> String) -> Vec<T> {
    if query.trim().is_empty() {
        return items.to_vec();
    }

    let tokens = query
        .split_whitespace()
        .filter(|token| !token.is_empty())
        .collect::<Vec<_>>();
    if tokens.is_empty() {
        return items.to_vec();
    }

    let mut results = Vec::new();
    for item in items {
        let text = get_text(item);
        let mut total_score = 0.0;
        let mut all_match = true;
        for token in &tokens {
            let matched = fuzzy_match(token, &text);
            if matched.matches {
                total_score += matched.score;
            } else {
                all_match = false;
                break;
            }
        }
        if all_match {
            results.push((item.clone(), total_score));
        }
    }

    results.sort_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(Ordering::Equal));
    results.into_iter().map(|(item, _)| item).collect()
}

fn match_query(query: &str, text: &str) -> FuzzyMatch {
    if query.is_empty() {
        return FuzzyMatch {
            matches: true,
            score: 0.0,
        };
    }
    if query.chars().count() > text.chars().count() {
        return FuzzyMatch {
            matches: false,
            score: 0.0,
        };
    }

    let query_chars = query.chars().collect::<Vec<_>>();
    let text_chars = text.chars().collect::<Vec<_>>();
    let mut query_index = 0;
    let mut score = 0.0;
    let mut last_match_index: Option<usize> = None;
    let mut consecutive_matches = 0.0;

    for index in 0..text_chars.len() {
        if query_index >= query_chars.len() {
            break;
        }
        if text_chars[index] != query_chars[query_index] {
            continue;
        }

        let is_word_boundary = index == 0 || is_word_separator(text_chars[index - 1]);
        if last_match_index == Some(index.saturating_sub(1)) {
            consecutive_matches += 1.0;
            score -= consecutive_matches * 5.0;
        } else {
            consecutive_matches = 0.0;
            if let Some(last) = last_match_index {
                score += (index - last - 1) as f64 * 2.0;
            }
        }

        if is_word_boundary {
            score -= 10.0;
        }
        score += index as f64 * 0.1;
        last_match_index = Some(index);
        query_index += 1;
    }

    if query_index < query_chars.len() {
        return FuzzyMatch {
            matches: false,
            score: 0.0,
        };
    }

    if query == text {
        score -= 100.0;
    }

    FuzzyMatch {
        matches: true,
        score,
    }
}

fn swapped_alpha_numeric_query(query: &str) -> String {
    let split = query
        .char_indices()
        .find(|(_, character)| !character.is_ascii_alphabetic())
        .map(|(index, _)| index);
    if let Some(index) = split {
        let (letters, digits) = query.split_at(index);
        if !letters.is_empty()
            && letters
                .chars()
                .all(|character| character.is_ascii_alphabetic())
            && !digits.is_empty()
            && digits.chars().all(|character| character.is_ascii_digit())
        {
            return format!("{digits}{letters}");
        }
    }

    let split = query
        .char_indices()
        .find(|(_, character)| !character.is_ascii_digit())
        .map(|(index, _)| index);
    if let Some(index) = split {
        let (digits, letters) = query.split_at(index);
        if !digits.is_empty()
            && digits.chars().all(|character| character.is_ascii_digit())
            && !letters.is_empty()
            && letters
                .chars()
                .all(|character| character.is_ascii_alphabetic())
        {
            return format!("{letters}{digits}");
        }
    }

    String::new()
}

fn is_word_separator(character: char) -> bool {
    matches!(character, ' ' | '\t' | '\n' | '-' | '_' | '.' | '/' | ':')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fuzzy_match_handles_empty_and_exact_query() {
        assert!(fuzzy_match("", "anything").matches);
        let exact = fuzzy_match("abc", "abc");
        assert!(exact.matches);
        assert!(exact.score < -100.0);
    }

    #[test]
    fn fuzzy_filter_sorts_best_matches_first() {
        let items = vec!["alpha-beta", "alphabet", "beta-alpha"];
        let filtered = fuzzy_filter(&items, "ab", |item| item.to_string());
        assert_eq!(filtered[0], "alpha-beta");
        assert_eq!(filtered.len(), 2);
    }

    #[test]
    fn fuzzy_match_supports_swapped_alpha_numeric_query() {
        let matched = fuzzy_match("foo12", "12foo");
        assert!(matched.matches);
        assert!(matched.score > fuzzy_match("12foo", "12foo").score);
    }
}
