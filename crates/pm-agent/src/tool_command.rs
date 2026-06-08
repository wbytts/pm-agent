use coding_agent::CodingToolRequest;

pub(crate) fn parse_tool_prompt(prompt: &str) -> Result<Option<CodingToolRequest>, String> {
    let Some(command) = prompt.strip_prefix('/') else {
        return Ok(None);
    };
    let (name, rest) = command
        .split_once(char::is_whitespace)
        .map_or((command, ""), |(name, rest)| (name, rest.trim_start()));

    match name {
        "read" => {
            let (path, offset, limit) = parse_read_args(rest)?;
            Ok(Some(CodingToolRequest::ReadFile {
                path,
                offset,
                limit,
            }))
        }
        "bash" => Ok(Some(CodingToolRequest::Bash {
            command: require_arg(rest, "/bash 需要命令内容")?.to_string(),
        })),
        "ls" => Ok(Some(CodingToolRequest::Ls {
            path: optional_arg(rest),
            limit: None,
        })),
        "find" => {
            let (pattern, path) = split_first_arg(rest, "/find 需要 glob 模式")?;
            Ok(Some(CodingToolRequest::Find {
                pattern: pattern.to_string(),
                path,
                limit: None,
            }))
        }
        "grep" => {
            let (pattern, path) = split_first_arg(rest, "/grep 需要搜索内容")?;
            Ok(Some(CodingToolRequest::Grep {
                pattern: pattern.to_string(),
                path,
                ignore_case: false,
                literal: true,
                limit: None,
            }))
        }
        "write" => {
            let (path, content) = rest
                .split_once('\n')
                .ok_or_else(|| "/write 需要第一行文件路径，后续内容作为文件内容".to_string())?;
            Ok(Some(CodingToolRequest::WriteFile {
                path: require_arg(path.trim(), "/write 需要文件路径")?.to_string(),
                content: content.to_string(),
            }))
        }
        "edit" => {
            let (path, body) = rest.split_once('\n').ok_or_else(|| {
                "/edit 需要第一行文件路径，后续使用 => 分隔查找和替换内容".to_string()
            })?;
            let path = require_arg(path.trim(), "/edit 需要文件路径")?.to_string();
            let edits = parse_edit_blocks(body)?;
            if edits.len() == 1 {
                let edit = edits.into_iter().next().expect("one edit");
                Ok(Some(CodingToolRequest::EditFile {
                    path,
                    search: edit.search,
                    replace: edit.replace,
                }))
            } else {
                Ok(Some(CodingToolRequest::EditFileBlocks { path, edits }))
            }
        }
        _ => Ok(None),
    }
}

fn parse_edit_blocks(body: &str) -> Result<Vec<coding_agent::CodingToolEdit>, String> {
    let mut edits = Vec::new();
    for block in body.split("\n---\n") {
        let (search, replace) = block
            .split_once("\n=>\n")
            .ok_or_else(|| "/edit 需要使用独立一行 => 分隔查找和替换内容".to_string())?;
        edits.push(coding_agent::CodingToolEdit {
            search: search.to_string(),
            replace: replace.to_string(),
        });
    }
    if edits.is_empty() {
        return Err("/edit 需要至少一个替换块".to_string());
    }
    Ok(edits)
}

fn parse_read_args(value: &str) -> Result<(String, Option<usize>, Option<usize>), String> {
    let value = require_arg(value, "/read 需要文件路径")?;
    let mut parts = value.split_whitespace();
    let path = parts
        .next()
        .ok_or_else(|| "/read 需要文件路径".to_string())?
        .to_string();
    let mut offset = None;
    let mut limit = None;
    for part in parts {
        if let Some(raw) = part.strip_prefix("offset=") {
            offset = Some(parse_positive_usize(raw, "offset")?);
        } else if let Some(raw) = part.strip_prefix("limit=") {
            limit = Some(parse_positive_usize(raw, "limit")?);
        }
    }
    Ok((path, offset, limit))
}

fn parse_positive_usize(value: &str, name: &str) -> Result<usize, String> {
    let parsed = value
        .parse::<usize>()
        .map_err(|_| format!("{name} 必须是正整数"))?;
    if parsed == 0 {
        return Err(format!("{name} 必须是正整数"));
    }
    Ok(parsed)
}

fn require_arg<'a>(value: &'a str, message: &str) -> Result<&'a str, String> {
    if value.trim().is_empty() {
        Err(message.to_string())
    } else {
        Ok(value.trim())
    }
}

fn optional_arg(value: &str) -> Option<String> {
    (!value.trim().is_empty()).then(|| value.trim().to_string())
}

fn split_first_arg<'a>(value: &'a str, message: &str) -> Result<(&'a str, Option<String>), String> {
    let value = require_arg(value, message)?;
    let (first, rest) = value
        .split_once(char::is_whitespace)
        .map_or((value, ""), |(first, rest)| (first, rest.trim()));
    Ok((first, optional_arg(rest)))
}
