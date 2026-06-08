use crate::session_manager::SessionInfo;
use regex::Regex;
use tui::fuzzy_match;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionSortMode {
    Threaded,
    Recent,
    Relevance,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionNameFilter {
    All,
    Named,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SearchToken {
    Fuzzy(String),
    Phrase(String),
}

#[derive(Debug, Clone)]
pub enum ParsedSessionSearchQuery {
    Tokens {
        tokens: Vec<SearchToken>,
    },
    Regex {
        regex: Option<Regex>,
        error: Option<String>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SessionMatchResult {
    pub matches: bool,
    pub score: f64,
}

pub fn has_session_name(session: &SessionInfo) -> bool {
    session
        .name
        .as_deref()
        .is_some_and(|name| !name.trim().is_empty())
}

pub fn parse_session_search_query(query: &str) -> ParsedSessionSearchQuery {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return ParsedSessionSearchQuery::Tokens { tokens: Vec::new() };
    }

    if let Some(pattern) = trimmed.strip_prefix("re:") {
        let pattern = pattern.trim();
        if pattern.is_empty() {
            return ParsedSessionSearchQuery::Regex {
                regex: None,
                error: Some("Empty regex".to_string()),
            };
        }
        return match Regex::new(&format!("(?i){pattern}")) {
            Ok(regex) => ParsedSessionSearchQuery::Regex {
                regex: Some(regex),
                error: None,
            },
            Err(error) => ParsedSessionSearchQuery::Regex {
                regex: None,
                error: Some(error.to_string()),
            },
        };
    }

    ParsedSessionSearchQuery::Tokens {
        tokens: parse_token_query(trimmed),
    }
}

pub fn match_session(
    session: &SessionInfo,
    parsed: &ParsedSessionSearchQuery,
) -> SessionMatchResult {
    let text = session_search_text(session);
    match parsed {
        ParsedSessionSearchQuery::Regex { regex, .. } => {
            let Some(regex) = regex else {
                return SessionMatchResult {
                    matches: false,
                    score: 0.0,
                };
            };
            let Some(found) = regex.find(&text) else {
                return SessionMatchResult {
                    matches: false,
                    score: 0.0,
                };
            };
            SessionMatchResult {
                matches: true,
                score: found.start() as f64 * 0.1,
            }
        }
        ParsedSessionSearchQuery::Tokens { tokens } => {
            if tokens.is_empty() {
                return SessionMatchResult {
                    matches: true,
                    score: 0.0,
                };
            }

            let mut total_score = 0.0;
            let mut normalized_text = None::<String>;
            for token in tokens {
                match token {
                    SearchToken::Phrase(phrase) => {
                        let text = normalized_text.get_or_insert_with(|| {
                            normalize_whitespace_lower(&session_search_text(session))
                        });
                        let phrase = normalize_whitespace_lower(phrase);
                        if phrase.is_empty() {
                            continue;
                        }
                        let Some(index) = text.find(&phrase) else {
                            return SessionMatchResult {
                                matches: false,
                                score: 0.0,
                            };
                        };
                        total_score += index as f64 * 0.1;
                    }
                    SearchToken::Fuzzy(value) => {
                        let matched = fuzzy_match(value, &text);
                        if !matched.matches {
                            return SessionMatchResult {
                                matches: false,
                                score: 0.0,
                            };
                        }
                        total_score += matched.score;
                    }
                }
            }
            SessionMatchResult {
                matches: true,
                score: total_score,
            }
        }
    }
}

pub fn filter_and_sort_sessions(
    sessions: &[SessionInfo],
    query: &str,
    sort_mode: SessionSortMode,
    name_filter: SessionNameFilter,
) -> Vec<SessionInfo> {
    let name_filtered = sessions
        .iter()
        .filter(|session| match name_filter {
            SessionNameFilter::All => true,
            SessionNameFilter::Named => has_session_name(session),
        })
        .cloned()
        .collect::<Vec<_>>();

    if query.trim().is_empty() {
        return name_filtered;
    }

    let parsed = parse_session_search_query(query);
    if matches!(
        parsed,
        ParsedSessionSearchQuery::Regex { error: Some(_), .. }
    ) {
        return Vec::new();
    }

    if sort_mode == SessionSortMode::Recent {
        return name_filtered
            .into_iter()
            .filter(|session| match_session(session, &parsed).matches)
            .collect();
    }

    let mut scored = name_filtered
        .into_iter()
        .filter_map(|session| {
            let matched = match_session(&session, &parsed);
            matched.matches.then_some((session, matched.score))
        })
        .collect::<Vec<_>>();
    scored.sort_by(|(left, left_score), (right, right_score)| {
        left_score
            .partial_cmp(right_score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(right.modified_millis.cmp(&left.modified_millis))
    });
    scored.into_iter().map(|(session, _)| session).collect()
}

fn parse_token_query(trimmed: &str) -> Vec<SearchToken> {
    let mut tokens = Vec::new();
    let mut buffer = String::new();
    let mut in_quote = false;
    let mut had_unclosed_quote = false;

    for character in trimmed.chars() {
        if character == '"' {
            if in_quote {
                flush_token(&mut tokens, &mut buffer, true);
                in_quote = false;
            } else {
                flush_token(&mut tokens, &mut buffer, false);
                in_quote = true;
            }
            continue;
        }

        if !in_quote && character.is_whitespace() {
            flush_token(&mut tokens, &mut buffer, false);
            continue;
        }

        buffer.push(character);
    }

    if in_quote {
        had_unclosed_quote = true;
    }
    if had_unclosed_quote {
        return trimmed
            .split_whitespace()
            .filter(|token| !token.trim().is_empty())
            .map(|token| SearchToken::Fuzzy(token.trim().to_string()))
            .collect();
    }

    flush_token(&mut tokens, &mut buffer, in_quote);
    tokens
}

fn flush_token(tokens: &mut Vec<SearchToken>, buffer: &mut String, phrase: bool) {
    let value = buffer.trim().to_string();
    buffer.clear();
    if value.is_empty() {
        return;
    }
    if phrase {
        tokens.push(SearchToken::Phrase(value));
    } else {
        tokens.push(SearchToken::Fuzzy(value));
    }
}

fn normalize_whitespace_lower(text: &str) -> String {
    text.to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn session_search_text(session: &SessionInfo) -> String {
    format!(
        "{} {} {} {}",
        session.id,
        session.name.as_deref().unwrap_or_default(),
        session.all_messages_text,
        session.cwd
    )
}

#[cfg(test)]
mod tests {
    use super::{
        filter_and_sort_sessions, has_session_name, match_session, parse_session_search_query,
        ParsedSessionSearchQuery, SearchToken, SessionNameFilter, SessionSortMode,
    };
    use crate::session_manager::SessionInfo;

    #[test]
    fn parses_token_phrase_regex_and_unclosed_quote_queries_like_pi() {
        assert!(matches!(
            parse_session_search_query("foo \"bar baz\""),
            ParsedSessionSearchQuery::Tokens { tokens } if tokens == vec![
                SearchToken::Fuzzy("foo".to_string()),
                SearchToken::Phrase("bar baz".to_string())
            ]
        ));
        assert!(matches!(
            parse_session_search_query("re:foo.*bar"),
            ParsedSessionSearchQuery::Regex {
                regex: Some(_),
                error: None
            }
        ));
        assert!(matches!(
            parse_session_search_query("re:"),
            ParsedSessionSearchQuery::Regex {
                regex: None,
                error: Some(_)
            }
        ));
        assert!(matches!(
            parse_session_search_query("\"foo bar"),
            ParsedSessionSearchQuery::Tokens { tokens } if tokens == vec![
                SearchToken::Fuzzy("\"foo".to_string()),
                SearchToken::Fuzzy("bar".to_string())
            ]
        ));
    }

    #[test]
    fn matches_regex_phrase_and_fuzzy_against_session_text() {
        let session = session("a", Some("Release Work"), "fix node cve", "/repo", 10);

        assert!(match_session(&session, &parse_session_search_query("release cve")).matches);
        assert!(match_session(&session, &parse_session_search_query("\"node cve\"")).matches);
        assert!(match_session(&session, &parse_session_search_query("re:node\\s+cve")).matches);
        assert!(!match_session(&session, &parse_session_search_query("\"cve node\"")).matches);
    }

    #[test]
    fn filters_named_sessions_and_recent_mode_keeps_input_order() {
        let sessions = vec![
            session("old", Some("Named"), "alpha", "/repo", 1),
            session("new", None, "alpha", "/repo", 99),
        ];

        assert!(has_session_name(&sessions[0]));
        assert!(!has_session_name(&sessions[1]));
        assert_eq!(
            filter_and_sort_sessions(
                &sessions,
                "alpha",
                SessionSortMode::Recent,
                SessionNameFilter::All
            )
            .into_iter()
            .map(|session| session.id)
            .collect::<Vec<_>>(),
            vec!["old", "new"]
        );
        assert_eq!(
            filter_and_sort_sessions(
                &sessions,
                "alpha",
                SessionSortMode::Recent,
                SessionNameFilter::Named
            )
            .into_iter()
            .map(|session| session.id)
            .collect::<Vec<_>>(),
            vec!["old"]
        );
    }

    #[test]
    fn relevance_mode_sorts_by_score_then_modified_desc() {
        let sessions = vec![
            session("older", Some("alpha"), "same", "/repo", 1),
            session("newer", Some("alpha"), "same", "/repo", 99),
            session("alpha-best", Some("x"), "alpha starts here", "/repo", 5),
        ];

        assert_eq!(
            filter_and_sort_sessions(
                &sessions,
                "alpha",
                SessionSortMode::Relevance,
                SessionNameFilter::All
            )
            .into_iter()
            .map(|session| session.id)
            .collect::<Vec<_>>(),
            vec!["alpha-best", "newer", "older"]
        );
    }

    fn session(
        id: &str,
        name: Option<&str>,
        all_messages_text: &str,
        cwd: &str,
        modified_millis: u128,
    ) -> SessionInfo {
        SessionInfo {
            path: format!("/sessions/{id}.jsonl"),
            id: id.to_string(),
            cwd: cwd.to_string(),
            name: name.map(str::to_string),
            parent_session_path: None,
            created_millis: 0,
            modified_millis,
            message_count: 1,
            first_message: all_messages_text.to_string(),
            all_messages_text: all_messages_text.to_string(),
        }
    }
}
