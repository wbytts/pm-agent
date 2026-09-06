use super::*;
use crate::tools::common::collect_temp_workspace;
use crate::types::{CodingAgentError, CodingToolEdit, CodingToolRequest, CodingWorkspace};
use serde_json::json;
use std::fs;

#[test]
fn executes_file_tools_inside_workspace() {
    let workspace = temp_workspace();
    let write = execute_tool(
        &workspace,
        CodingToolRequest::WriteFile {
            path: "notes/todo.txt".to_string(),
            content: "hello".to_string(),
        },
    )
    .expect("write should work");
    assert!(write.output.contains("Successfully wrote 5 bytes to"));

    let read = execute_tool(
        &workspace,
        CodingToolRequest::ReadFile {
            path: "notes/todo.txt".to_string(),
            offset: None,
            limit: None,
        },
    )
    .expect("read should work");
    assert_eq!(read.output, "hello");
}

#[test]
fn plans_initial_active_and_allowed_tools_like_pi_sdk() {
    assert_eq!(
        plan_tool_activation(None, None),
        ToolActivationPlan {
            initial_active_tool_names: vec![
                "read".to_string(),
                "bash".to_string(),
                "edit".to_string(),
                "write".to_string(),
            ],
            allowed_tool_names: None,
        }
    );

    assert_eq!(
        plan_tool_activation(None, Some(NoToolsMode::All)),
        ToolActivationPlan {
            initial_active_tool_names: Vec::new(),
            allowed_tool_names: Some(Vec::new()),
        }
    );

    assert_eq!(
        plan_tool_activation(None, Some(NoToolsMode::Builtin)),
        ToolActivationPlan {
            initial_active_tool_names: Vec::new(),
            allowed_tool_names: None,
        }
    );

    let explicit = vec!["read".to_string(), "grep".to_string()];
    assert_eq!(
        plan_tool_activation(Some(explicit.clone()), Some(NoToolsMode::All)),
        ToolActivationPlan {
            initial_active_tool_names: explicit.clone(),
            allowed_tool_names: Some(explicit),
        }
    );
}

#[test]
fn prepares_legacy_edit_fields_like_pi_prepare_arguments() {
    let request = prepare_tool_request(
        "edit",
        json!({
            "path": "file.txt",
            "oldText": "before",
            "newText": "after"
        }),
    )
    .expect("legacy edit input should prepare");

    assert_eq!(
        request,
        CodingToolRequest::EditFileBlocks {
            path: "file.txt".to_string(),
            edits: vec![CodingToolEdit {
                search: "before".to_string(),
                replace: "after".to_string(),
            }],
        }
    );
}

#[test]
fn appends_legacy_edit_fields_to_existing_edits_like_pi_prepare_arguments() {
    let request = prepare_tool_request(
        "edit",
        json!({
            "path": "file.txt",
            "edits": [{"oldText": "a", "newText": "b"}],
            "oldText": "c",
            "newText": "d"
        }),
    )
    .expect("legacy edit input should prepare");

    assert_eq!(
        request,
        CodingToolRequest::EditFileBlocks {
            path: "file.txt".to_string(),
            edits: vec![
                CodingToolEdit {
                    search: "a".to_string(),
                    replace: "b".to_string(),
                },
                CodingToolEdit {
                    search: "c".to_string(),
                    replace: "d".to_string(),
                },
            ],
        }
    );
}

#[test]
fn parses_stringified_edit_blocks_like_pi_prepare_arguments() {
    let request = prepare_tool_request(
        "edit",
        json!({
            "path": "file.txt",
            "edits": r#"[{"oldText":"a","newText":"b"}]"#
        }),
    )
    .expect("stringified edits should prepare");

    assert_eq!(
        request,
        CodingToolRequest::EditFileBlocks {
            path: "file.txt".to_string(),
            edits: vec![CodingToolEdit {
                search: "a".to_string(),
                replace: "b".to_string(),
            }],
        }
    );
}

#[test]
fn rejects_parent_directory_paths() {
    let workspace = temp_workspace();
    let error = execute_tool(
        &workspace,
        CodingToolRequest::ReadFile {
            path: "../secret.txt".to_string(),
            offset: None,
            limit: None,
        },
    )
    .expect_err("parent path should fail");
    assert!(matches!(error, CodingAgentError::UnsafePath(_)));
}

#[test]
fn lists_finds_and_greps_workspace_files() {
    let workspace = temp_workspace();
    execute_tool(
        &workspace,
        CodingToolRequest::WriteFile {
            path: "src/main.rs".to_string(),
            content: "fn main() {\n  println!(\"hello\");\n}".to_string(),
        },
    )
    .expect("write should work");

    let listed = execute_tool(
        &workspace,
        CodingToolRequest::Ls {
            path: Some("src".to_string()),
            limit: None,
        },
    )
    .expect("ls should work");
    assert!(listed.output.contains("main.rs"));

    let found = execute_tool(
        &workspace,
        CodingToolRequest::Find {
            pattern: "*.rs".to_string(),
            path: None,
            limit: None,
        },
    )
    .expect("find should work");
    assert!(found.output.contains("src/main.rs"));

    let grep = execute_tool(
        &workspace,
        CodingToolRequest::Grep {
            pattern: "println".to_string(),
            path: Some("src".to_string()),
            glob: None,
            ignore_case: false,
            literal: true,
            context: None,
            limit: None,
        },
    )
    .expect("grep should work");
    assert!(grep.output.contains("main.rs:2"));
}

#[test]
fn grep_defaults_to_regex_and_literal_keeps_pattern_verbatim_like_pi() {
    let workspace = temp_workspace();
    execute_tool(
        &workspace,
        CodingToolRequest::WriteFile {
            path: "notes.txt".to_string(),
            content: "hello\nh.llo\n".to_string(),
        },
    )
    .expect("write should work");

    let regex = execute_tool(
        &workspace,
        CodingToolRequest::Grep {
            pattern: "h.llo".to_string(),
            path: Some("notes.txt".to_string()),
            glob: None,
            ignore_case: false,
            literal: false,
            context: None,
            limit: None,
        },
    )
    .expect("regex grep should work");
    assert!(regex.output.contains("notes.txt:1: hello"));
    assert!(regex.output.contains("notes.txt:2: h.llo"));

    let literal = execute_tool(
        &workspace,
        CodingToolRequest::Grep {
            pattern: "h.llo".to_string(),
            path: Some("notes.txt".to_string()),
            glob: None,
            ignore_case: false,
            literal: true,
            context: None,
            limit: None,
        },
    )
    .expect("literal grep should work");
    assert!(!literal.output.contains("notes.txt:1: hello"));
    assert!(literal.output.contains("notes.txt:2: h.llo"));
}

#[test]
fn grep_supports_glob_filter_like_pi() {
    let workspace = temp_workspace();
    execute_tool(
        &workspace,
        CodingToolRequest::WriteFile {
            path: "src/main.rs".to_string(),
            content: "needle rust".to_string(),
        },
    )
    .expect("rust file should be written");
    execute_tool(
        &workspace,
        CodingToolRequest::WriteFile {
            path: "src/main.ts".to_string(),
            content: "needle typescript".to_string(),
        },
    )
    .expect("typescript file should be written");

    let grep = execute_tool(
        &workspace,
        CodingToolRequest::Grep {
            pattern: "needle".to_string(),
            path: Some("src".to_string()),
            glob: Some("*.rs".to_string()),
            ignore_case: false,
            literal: true,
            context: None,
            limit: None,
        },
    )
    .expect("grep should work");

    assert!(grep.output.contains("main.rs:1: needle rust"));
    assert!(!grep.output.contains("main.ts"));
}

#[test]
fn grep_supports_context_lines_like_pi() {
    let workspace = temp_workspace();
    execute_tool(
        &workspace,
        CodingToolRequest::WriteFile {
            path: "src/main.rs".to_string(),
            content: "before\nneedle\n after\nneedle again".to_string(),
        },
    )
    .expect("file should be written");

    let grep = execute_tool(
        &workspace,
        CodingToolRequest::Grep {
            pattern: "needle".to_string(),
            path: Some("src".to_string()),
            glob: None,
            ignore_case: false,
            literal: true,
            context: Some(1),
            limit: Some(1),
        },
    )
    .expect("grep should work");

    assert!(grep.output.contains("main.rs-1- before"));
    assert!(grep.output.contains("main.rs:2: needle"));
    assert!(grep.output.contains("main.rs-3-  after"));
    assert!(!grep.output.contains("main.rs:4: needle again"));
    assert!(grep.output.contains("1 matches limit reached"));
    let details = grep.details.expect("grep match limit details");
    assert_eq!(details["matchLimitReached"], 1);
    assert!(details.get("truncation").is_none());
}

#[test]
fn grep_clamps_zero_limit_to_one_like_pi() {
    let workspace = temp_workspace();
    execute_tool(
        &workspace,
        CodingToolRequest::WriteFile {
            path: "notes.txt".to_string(),
            content: "needle one\nneedle two".to_string(),
        },
    )
    .expect("file should be written");

    let grep = execute_tool(
        &workspace,
        CodingToolRequest::Grep {
            pattern: "needle".to_string(),
            path: Some("notes.txt".to_string()),
            glob: None,
            ignore_case: false,
            literal: true,
            context: None,
            limit: Some(0),
        },
    )
    .expect("grep should work");

    assert!(
        grep.output.contains("notes.txt:1: needle one"),
        "{}",
        grep.output
    );
    assert!(!grep.output.contains("notes.txt:2: needle two"));
    assert!(grep.output.contains("1 matches limit reached"));
    let details = grep.details.expect("grep limit details");
    assert_eq!(details["matchLimitReached"], 1);
}

#[test]
fn find_path_globs_match_nested_paths_like_pi_fd_full_path_mode() {
    let workspace = temp_workspace();
    execute_tool(
        &workspace,
        CodingToolRequest::WriteFile {
            path: "nested/src/main.rs".to_string(),
            content: "fn main() {}".to_string(),
        },
    )
    .expect("write should work");

    let found = execute_tool(
        &workspace,
        CodingToolRequest::Find {
            pattern: "src/**/*.rs".to_string(),
            path: None,
            limit: None,
        },
    )
    .expect("find should work");

    assert!(found.output.contains("nested/src/main.rs"));
}

#[test]
fn find_outputs_are_sorted_case_insensitively_like_pi_fd() {
    let workspace = temp_workspace();
    for path in ["src/zeta.rs", "src/Alpha.rs", "src/beta.rs"] {
        execute_tool(
            &workspace,
            CodingToolRequest::WriteFile {
                path: path.to_string(),
                content: String::new(),
            },
        )
        .expect("file should be written");
    }

    let found = execute_tool(
        &workspace,
        CodingToolRequest::Find {
            pattern: "*.rs".to_string(),
            path: Some("src".to_string()),
            limit: None,
        },
    )
    .expect("find should work");

    assert_eq!(found.output, "Alpha.rs\nbeta.rs\nzeta.rs");
}

#[test]
fn find_includes_result_limit_details_like_pi() {
    let workspace = temp_workspace();
    for path in ["a.txt", "b.txt", "c.txt"] {
        execute_tool(
            &workspace,
            CodingToolRequest::WriteFile {
                path: path.to_string(),
                content: String::new(),
            },
        )
        .expect("file should be written");
    }

    let found = execute_tool(
        &workspace,
        CodingToolRequest::Find {
            pattern: "*.txt".to_string(),
            path: None,
            limit: Some(2),
        },
    )
    .expect("find should work");

    assert_eq!(
        found.output,
        "a.txt\nb.txt\n\n[2 results limit reached. Use limit=4 for more, or refine pattern]"
    );
    let details = found.details.expect("find limit details");
    assert_eq!(details["resultLimitReached"], 2);
    assert!(details.get("truncation").is_none());
}

#[test]
fn ls_includes_entry_limit_details_like_pi() {
    let workspace = temp_workspace();
    for path in ["a.txt", "b.txt", "c.txt"] {
        execute_tool(
            &workspace,
            CodingToolRequest::WriteFile {
                path: path.to_string(),
                content: String::new(),
            },
        )
        .expect("file should be written");
    }

    let listed = execute_tool(
        &workspace,
        CodingToolRequest::Ls {
            path: None,
            limit: Some(2),
        },
    )
    .expect("ls should work");

    assert_eq!(
        listed.output,
        "a.txt\nb.txt\n\n[2 entries limit reached. Use limit=4 for more]"
    );
    let details = listed.details.expect("ls limit details");
    assert_eq!(details["entryLimitReached"], 2);
    assert!(details.get("truncation").is_none());
}

#[test]
fn ls_resolves_pi_path_input_variants_inside_workspace() {
    let workspace = temp_workspace();
    fs::create_dir_all(workspace.cwd.join("space name")).expect("directory should be written");
    fs::write(workspace.cwd.join("space name/file.txt"), "ok").expect("file should be written");

    let listed = execute_tool(
        &workspace,
        CodingToolRequest::Ls {
            path: Some("@space\u{00a0}name".to_string()),
            limit: None,
        },
    )
    .expect("ls should work");

    assert_eq!(listed.output, "file.txt");
}

#[test]
fn write_reports_original_path_like_pi_after_path_normalization() {
    let workspace = temp_workspace();
    let write = execute_tool(
        &workspace,
        CodingToolRequest::WriteFile {
            path: "@space\u{00a0}name/file.txt".to_string(),
            content: "ok".to_string(),
        },
    )
    .expect("write should work");

    assert_eq!(
        fs::read_to_string(workspace.cwd.join("space name/file.txt")).expect("file should exist"),
        "ok"
    );
    assert_eq!(
        write.output,
        "Successfully wrote 2 bytes to @space\u{00a0}name/file.txt"
    );
}

#[test]
fn read_truncates_large_text_outputs() {
    let workspace = temp_workspace();
    let content = (0..2100)
        .map(|index| format!("line {index}"))
        .collect::<Vec<_>>()
        .join("\n");
    execute_tool(
        &workspace,
        CodingToolRequest::WriteFile {
            path: "large.txt".to_string(),
            content,
        },
    )
    .expect("write should work");

    let read = execute_tool(
        &workspace,
        CodingToolRequest::ReadFile {
            path: "large.txt".to_string(),
            offset: None,
            limit: None,
        },
    )
    .expect("read should work");

    assert!(read.output.contains("line 1999"));
    assert!(!read.output.contains("line 2000"));
    assert!(read
        .output
        .contains("[Showing lines 1-2000 of 2100. Use offset=2001 to continue.]"));
    let details = read
        .details
        .expect("read should include truncation details");
    assert_eq!(details["truncation"]["truncated"], true);
    assert_eq!(details["truncation"]["truncatedBy"], "lines");
    assert_eq!(details["truncation"]["outputLines"], 2000);
}

#[test]
fn read_supports_offset_and_limit_continuation() {
    let workspace = temp_workspace();
    execute_tool(
        &workspace,
        CodingToolRequest::WriteFile {
            path: "lines.txt".to_string(),
            content: "one\ntwo\nthree\nfour".to_string(),
        },
    )
    .expect("write should work");

    let read = execute_tool(
        &workspace,
        CodingToolRequest::ReadFile {
            path: "lines.txt".to_string(),
            offset: Some(2),
            limit: Some(2),
        },
    )
    .expect("read should work");

    assert!(read.output.starts_with("two\nthree"));
    assert!(read
        .output
        .contains("[1 more lines in file. Use offset=4 to continue.]"));
    assert!(!read.output.contains("one"));
    assert!(read.details.is_none());
}

#[test]
fn read_returns_image_content_blocks_like_pi() {
    let workspace = temp_workspace();
    let png = static_png_bytes();
    fs::write(workspace.cwd.join("image.png"), &png).expect("image should be written");

    let read = execute_tool(
        &workspace,
        CodingToolRequest::ReadFile {
            path: "image.png".to_string(),
            offset: None,
            limit: None,
        },
    )
    .expect("read image should work");

    assert_eq!(read.output, "Read image file [image/png]");
    let content = read
        .content
        .expect("image read should include content blocks");
    assert_eq!(content.len(), 2);
    assert_eq!(
        content[0],
        crate::types::CodingContentBlock::Text {
            text: read.output.clone()
        }
    );
    match &content[1] {
        crate::types::CodingContentBlock::Image { data, mime_type } => {
            assert_eq!(mime_type, "image/png");
            assert!(data.starts_with("iVBORw0KGgo"));
        }
        other => panic!("expected image block, got {other:?}"),
    }
}

#[test]
fn read_omits_image_when_inline_limits_cannot_be_met_like_pi() {
    let workspace = temp_workspace();
    let mut png = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    png.extend_from_slice(&13u32.to_be_bytes());
    png.extend_from_slice(b"IHDR");
    png.extend_from_slice(&3000u32.to_be_bytes());
    png.extend_from_slice(&3000u32.to_be_bytes());
    png.extend_from_slice(&[8, 2, 0, 0, 0]);
    png.extend_from_slice(&1u32.to_be_bytes());
    png.extend_from_slice(b"IDAT");
    fs::write(workspace.cwd.join("huge.png"), png).expect("image should be written");

    let read = execute_tool(
        &workspace,
        CodingToolRequest::ReadFile {
            path: "huge.png".to_string(),
            offset: None,
            limit: None,
        },
    )
    .expect("read image should work");

    assert_eq!(
        read.output,
        "Read image file [image/png]\n[Image omitted: could not be resized below the inline image size limit.]"
    );
    assert_eq!(
        read.content,
        Some(vec![crate::types::CodingContentBlock::Text {
            text: read.output.clone()
        }])
    );
}

#[test]
fn read_reports_first_line_exceeds_limit_in_details() {
    let workspace = temp_workspace();
    execute_tool(
        &workspace,
        CodingToolRequest::WriteFile {
            path: "long-line.txt".to_string(),
            content: "x".repeat(60 * 1024),
        },
    )
    .expect("write should work");

    let read = execute_tool(
        &workspace,
        CodingToolRequest::ReadFile {
            path: "long-line.txt".to_string(),
            offset: None,
            limit: None,
        },
    )
    .expect("read should work");

    let details = read
        .details
        .expect("read should include truncation details");
    assert!(read
        .output
        .contains("Use bash: sed -n '1p' long-line.txt | head -c 51200"));
    assert_eq!(details["truncation"]["firstLineExceedsLimit"], true);
    assert_eq!(details["truncation"]["truncatedBy"], "bytes");
}

#[test]
fn read_replaces_invalid_utf8_like_node_buffer_decoding() {
    let workspace = temp_workspace();
    fs::write(workspace.cwd.join("invalid.txt"), [b'a', 0xff, b'b'])
        .expect("file should be written");

    let read = execute_tool(
        &workspace,
        CodingToolRequest::ReadFile {
            path: "invalid.txt".to_string(),
            offset: None,
            limit: None,
        },
    )
    .expect("read should work");

    assert_eq!(read.output, "a\u{FFFD}b");
}

#[test]
fn grep_truncates_long_match_lines_like_pi() {
    let workspace = temp_workspace();
    execute_tool(
        &workspace,
        CodingToolRequest::WriteFile {
            path: "long.txt".to_string(),
            content: format!("needle {}", "x".repeat(600)),
        },
    )
    .expect("write should work");

    let grep = execute_tool(
        &workspace,
        CodingToolRequest::Grep {
            pattern: "needle".to_string(),
            path: Some("long.txt".to_string()),
            glob: None,
            ignore_case: false,
            literal: true,
            context: None,
            limit: None,
        },
    )
    .expect("grep should work");

    assert!(grep.output.contains("... [truncated]"));
    assert!(grep.output.contains("Some lines truncated to 500 chars"));
    let details = grep.details.expect("grep line truncation details");
    assert_eq!(details["linesTruncated"], true);
    assert!(details.get("truncation").is_none());
}

#[test]
fn find_ls_and_grep_report_byte_limits() {
    let workspace = temp_workspace();
    for index in 0..900 {
        let file_name = format!("dir-{index:04}-{}/file-{index:04}.txt", "segment".repeat(8));
        execute_tool(
            &workspace,
            CodingToolRequest::WriteFile {
                path: file_name,
                content: "needle".to_string(),
            },
        )
        .expect("write should work");
    }

    let listed = execute_tool(
        &workspace,
        CodingToolRequest::Ls {
            path: None,
            limit: Some(900),
        },
    )
    .expect("ls should work");
    assert!(listed.output.contains("50.0KB limit reached"));
    let ls_details = listed.details.expect("ls truncation details");
    assert_eq!(ls_details["truncation"]["truncated"], true);
    assert_eq!(ls_details["truncation"]["truncatedBy"], "bytes");

    let found = execute_tool(
        &workspace,
        CodingToolRequest::Find {
            pattern: "*.txt".to_string(),
            path: None,
            limit: Some(900),
        },
    )
    .expect("find should work");
    assert!(found.output.contains("50.0KB limit reached"));
    let find_details = found.details.expect("find truncation details");
    assert_eq!(find_details["truncation"]["truncated"], true);
    assert_eq!(find_details["truncation"]["truncatedBy"], "bytes");

    let grep = execute_tool(
        &workspace,
        CodingToolRequest::Grep {
            pattern: "needle".to_string(),
            path: None,
            glob: None,
            ignore_case: false,
            literal: true,
            context: None,
            limit: Some(900),
        },
    )
    .expect("grep should work");
    assert!(grep.output.contains("50.0KB limit reached"));
    let grep_details = grep.details.expect("grep truncation details");
    assert_eq!(grep_details["truncation"]["truncated"], true);
    assert_eq!(grep_details["truncation"]["truncatedBy"], "bytes");
}

#[test]
fn find_and_grep_respect_gitignore_rules() {
    let workspace = temp_workspace();
    execute_tool(
        &workspace,
        CodingToolRequest::WriteFile {
            path: ".gitignore".to_string(),
            content: "ignored/\n*.log\n!important.log\n".to_string(),
        },
    )
    .expect("gitignore should be written");
    execute_tool(
        &workspace,
        CodingToolRequest::WriteFile {
            path: "ignored/secret.txt".to_string(),
            content: "needle secret".to_string(),
        },
    )
    .expect("ignored file should be written");
    execute_tool(
        &workspace,
        CodingToolRequest::WriteFile {
            path: "debug.log".to_string(),
            content: "needle debug".to_string(),
        },
    )
    .expect("log file should be written");
    execute_tool(
        &workspace,
        CodingToolRequest::WriteFile {
            path: "important.log".to_string(),
            content: "needle important".to_string(),
        },
    )
    .expect("negated file should be written");
    execute_tool(
        &workspace,
        CodingToolRequest::WriteFile {
            path: "visible.txt".to_string(),
            content: "needle visible".to_string(),
        },
    )
    .expect("visible file should be written");

    let found = execute_tool(
        &workspace,
        CodingToolRequest::Find {
            pattern: "*".to_string(),
            path: None,
            limit: Some(100),
        },
    )
    .expect("find should work");
    assert!(found.output.contains("visible.txt"));
    assert!(found.output.contains("important.log"));
    assert!(!found.output.contains("debug.log"));
    assert!(!found.output.contains("ignored/secret.txt"));

    let grep = execute_tool(
        &workspace,
        CodingToolRequest::Grep {
            pattern: "needle".to_string(),
            path: None,
            glob: None,
            ignore_case: false,
            literal: true,
            context: None,
            limit: Some(100),
        },
    )
    .expect("grep should work");
    assert!(grep.output.contains("visible.txt"));
    assert!(grep.output.contains("important.log"));
    assert!(!grep.output.contains("debug.log"));
    assert!(!grep.output.contains("ignored/secret.txt"));
}

#[test]
fn edit_supports_pi_like_fuzzy_matching_and_preserves_file_format() {
    let workspace = temp_workspace();
    let raw = "\u{FEFF}title\r\nlet name = “pm-agent”;\u{00A0}  \r\n";
    execute_tool(
        &workspace,
        CodingToolRequest::WriteFile {
            path: "src/app.ts".to_string(),
            content: raw.to_string(),
        },
    )
    .expect("write should work");

    let result = execute_tool(
        &workspace,
        CodingToolRequest::EditFile {
            path: "src/app.ts".to_string(),
            search: "let name = \"pm-agent\";".to_string(),
            replace: "let name = \"pm\";".to_string(),
        },
    )
    .expect("fuzzy edit should work");

    let written = fs::read_to_string(workspace.cwd.join("src/app.ts")).expect("read edited file");
    assert!(written.starts_with('\u{FEFF}'));
    assert!(written.contains("\r\nlet name = \"pm\";\r\n"));
    let details = result.details.expect("edit details");
    assert_eq!(details["firstChangedLine"], 2);
    assert!(details["diff"]
        .as_str()
        .unwrap_or_default()
        .contains("-2 let name"));
    assert!(details["patch"]
        .as_str()
        .unwrap_or_default()
        .contains("+++ "));
}

#[test]
fn edit_supports_multiple_disjoint_blocks_like_pi() {
    let workspace = temp_workspace();
    execute_tool(
        &workspace,
        CodingToolRequest::WriteFile {
            path: "src/app.ts".to_string(),
            content: "const first = 1;\nconst middle = 2;\nconst last = 3;\n".to_string(),
        },
    )
    .expect("write should work");

    let result = execute_tool(
        &workspace,
        CodingToolRequest::EditFileBlocks {
            path: "src/app.ts".to_string(),
            edits: vec![
                crate::types::CodingToolEdit {
                    search: "const first = 1;".to_string(),
                    replace: "const first = 10;".to_string(),
                },
                crate::types::CodingToolEdit {
                    search: "const last = 3;".to_string(),
                    replace: "const last = 30;".to_string(),
                },
            ],
        },
    )
    .expect("multi edit should work");

    let written = fs::read_to_string(workspace.cwd.join("src/app.ts")).expect("read edited file");
    assert_eq!(
        written,
        "const first = 10;\nconst middle = 2;\nconst last = 30;\n"
    );
    assert!(result.output.contains("2 block(s)"));
    assert!(result.details.expect("details")["diff"]
        .as_str()
        .unwrap_or_default()
        .contains("const last = 30"));
}

#[test]
fn read_resolves_pi_path_input_variants_inside_workspace() {
    let workspace = temp_workspace();
    execute_tool(
        &workspace,
        CodingToolRequest::WriteFile {
            path: "space name.txt".to_string(),
            content: "space".to_string(),
        },
    )
    .expect("space file should be written");
    fs::write(workspace.cwd.join("Capture 1\u{202f}PM.png"), "ampm")
        .expect("ampm file should be written");
    fs::write(workspace.cwd.join("Capture d\u{2019}ecran.txt"), "curly")
        .expect("curly file should be written");
    fs::write(workspace.cwd.join("cafe\u{0301}.txt"), "nfd").expect("nfd file should be written");

    let at_path = execute_tool(
        &workspace,
        CodingToolRequest::ReadFile {
            path: "@space\u{00a0}name.txt".to_string(),
            offset: None,
            limit: None,
        },
    )
    .expect("read should work");
    assert_eq!(at_path.output, "space");

    let ampm = execute_tool(
        &workspace,
        CodingToolRequest::ReadFile {
            path: "Capture 1 PM.png".to_string(),
            offset: None,
            limit: None,
        },
    )
    .expect("read should work");
    assert_eq!(ampm.output, "ampm");

    let curly = execute_tool(
        &workspace,
        CodingToolRequest::ReadFile {
            path: "Capture d'ecran.txt".to_string(),
            offset: None,
            limit: None,
        },
    )
    .expect("read should work");
    assert_eq!(curly.output, "curly");

    let nfd = execute_tool(
        &workspace,
        CodingToolRequest::ReadFile {
            path: "café.txt".to_string(),
            offset: None,
            limit: None,
        },
    )
    .expect("read should work");
    assert_eq!(nfd.output, "nfd");
}

#[test]
fn bash_truncates_large_output_from_tail() {
    let workspace = temp_workspace();
    let bash = execute_tool(
        &workspace,
        CodingToolRequest::Bash {
            command: "for i in $(seq 0 2100); do echo line-$i; done".to_string(),
            timeout: None,
        },
    )
    .expect("bash should run");

    assert!(bash.output.contains("line-2100"));
    assert!(!bash.output.contains("line-0"));
    assert!(bash.output.contains("[Showing lines "));
    assert!(bash.output.contains("Full output:"));
    let details = bash.details.expect("bash truncation details");
    assert_eq!(details["truncation"]["truncated"], true);
    assert_eq!(details["truncation"]["truncatedBy"], "lines");
    assert!(details["fullOutputPath"]
        .as_str()
        .is_some_and(|path| path.contains("pm-agent-bash")));
}

#[test]
fn bash_returns_error_for_non_zero_exit_like_pi() {
    let workspace = temp_workspace();
    let error = execute_tool(
        &workspace,
        CodingToolRequest::Bash {
            command: "printf 'out'; printf 'err' >&2; exit 7".to_string(),
            timeout: None,
        },
    )
    .expect_err("non-zero bash commands should fail");

    let message = error.to_string();
    assert!(message.contains("Command exited with code 7"));
    assert!(message.contains("out"));
    assert!(message.contains("err"));
}

#[test]
fn bash_supports_timeout_like_pi() {
    let workspace = temp_workspace();
    let error = execute_tool(
        &workspace,
        CodingToolRequest::Bash {
            command: "printf before; sleep 2; printf finished-marker".to_string(),
            timeout: Some(1),
        },
    )
    .expect_err("timed out bash commands should fail");

    let message = error.to_string();
    assert!(message.contains("Command timed out after 1 seconds"));
    assert!(message.contains("before"));
    assert!(!message.contains("finished-marker"));
}

fn temp_workspace() -> CodingWorkspace {
    let cwd = collect_temp_workspace("pm-agent-coding-agent-test");
    fs::create_dir_all(&cwd).expect("workspace should be created");
    CodingWorkspace { cwd }
}

fn static_png_bytes() -> Vec<u8> {
    let mut png = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    png.extend_from_slice(&13u32.to_be_bytes());
    png.extend_from_slice(b"IHDR");
    png.extend_from_slice(&2u32.to_be_bytes());
    png.extend_from_slice(&3u32.to_be_bytes());
    png.extend_from_slice(&[0; 9]);
    png.extend_from_slice(&1u32.to_be_bytes());
    png.extend_from_slice(b"IDAT");
    png
}
