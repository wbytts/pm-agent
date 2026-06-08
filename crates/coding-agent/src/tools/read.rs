use std::fs;

use crate::tools::common::success;
use crate::tools::truncate::{format_size, truncate_head, TruncatedBy, TruncationOptions};
use crate::types::{
    CodingAgentError, CodingAgentResult, CodingContentBlock, CodingToolResult, CodingWorkspace,
};
use crate::utils::base64::encode_base64;
use crate::utils::mime::detect_supported_image_mime_type_from_file;
use crate::workspace::resolve_read_workspace_path;
use serde_json::json;

pub fn read_file(
    workspace: &CodingWorkspace,
    path: String,
    offset: Option<usize>,
    limit: Option<usize>,
) -> CodingAgentResult<CodingToolResult> {
    let raw_path = path;
    let path = resolve_read_workspace_path(workspace, &raw_path)?;
    if let Some(mime_type) = detect_supported_image_mime_type_from_file(&path).map_err(|error| {
        CodingAgentError::File(format!("检查文件类型 {} 失败：{error}", path.display()))
    })? {
        let bytes = fs::read(&path).map_err(|error| {
            CodingAgentError::File(format!("读取文件 {} 失败：{error}", path.display()))
        })?;
        let text = format!("Read image file [{mime_type}]");
        return Ok(CodingToolResult {
            success: true,
            output: text.clone(),
            details: None,
            content: Some(vec![
                CodingContentBlock::Text { text },
                CodingContentBlock::Image {
                    data: encode_base64(&bytes),
                    mime_type: mime_type.to_string(),
                },
            ]),
        });
    }

    let bytes = fs::read(&path).map_err(|error| {
        CodingAgentError::File(format!("读取文件 {} 失败：{error}", path.display()))
    })?;
    let content = String::from_utf8_lossy(&bytes);
    let all_lines = content.split('\n').collect::<Vec<_>>();
    let total_file_lines = all_lines.len();
    let start_line = offset.unwrap_or(1).saturating_sub(1);
    if start_line >= total_file_lines {
        return Err(CodingAgentError::File(format!(
            "Offset {} is beyond end of file ({} lines total)",
            offset.unwrap_or(1),
            total_file_lines
        )));
    }

    let selected_end = limit.map(|limit| (start_line + limit).min(total_file_lines));
    let selected_lines = match selected_end {
        Some(end) => &all_lines[start_line..end],
        None => &all_lines[start_line..],
    };
    let selected_content = selected_lines.join("\n");
    let truncation = truncate_head(&selected_content, TruncationOptions::default());
    if !truncation.truncated {
        if let Some(end) = selected_end {
            if end < total_file_lines {
                let remaining = total_file_lines - end;
                let next_offset = end + 1;
                return success(format!(
                    "{}\n\n[{remaining} more lines in file. Use offset={next_offset} to continue.]",
                    truncation.content
                ));
            }
        }
        return success(truncation.content);
    }

    let mut output = truncation.content.clone();
    let start_line_display = start_line + 1;
    if truncation.first_line_exceeds_limit {
        let first_line_size = all_lines
            .get(start_line)
            .map(|line| line.len())
            .unwrap_or_default();
        output = format!(
            "[Line {} is {}, exceeds {} limit. Use bash: sed -n '{}p' {} | head -c {}]",
            start_line_display,
            format_size(first_line_size),
            format_size(truncation.max_bytes),
            start_line_display,
            raw_path,
            truncation.max_bytes
        );
    } else if truncation.truncated_by == Some(TruncatedBy::Lines) {
        let end_line_display = start_line_display + truncation.output_lines.saturating_sub(1);
        let next_offset = end_line_display + 1;
        output.push_str(&format!(
            "\n\n[Showing lines {}-{} of {}. Use offset={} to continue.]",
            start_line_display, end_line_display, total_file_lines, next_offset
        ));
    } else {
        let end_line_display = start_line_display + truncation.output_lines.saturating_sub(1);
        let next_offset = end_line_display + 1;
        output.push_str(&format!(
            "\n\n[Showing lines {}-{} of {} ({} limit). Use offset={} to continue.]",
            start_line_display,
            end_line_display,
            total_file_lines,
            format_size(truncation.max_bytes),
            next_offset
        ));
    }
    Ok(CodingToolResult {
        success: true,
        output,
        details: Some(json!({ "truncation": truncation })),
        content: None,
    })
}
