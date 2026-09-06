use crate::export_html::theme::ExportTheme;

const TEMPLATE_HTML: &str = include_str!("assets/template.html");
const TEMPLATE_CSS: &str = include_str!("assets/template.css");
const TEMPLATE_JS: &str = include_str!("assets/template.js");
const MARKED_JS: &str = include_str!("assets/vendor/marked.min.js");
const HIGHLIGHT_JS: &str = include_str!("assets/vendor/highlight.min.js");

pub fn render_template(session_json: &str, theme: &ExportTheme) -> String {
    let css = TEMPLATE_CSS
        .replace("{{THEME_VARS}}", &theme.css_vars())
        .replace("{{THEME_NAME}}", &theme.name);

    TEMPLATE_HTML
        .replace("{{CSS}}", &css)
        .replace("{{JS}}", TEMPLATE_JS)
        .replace("{{MARKED_JS}}", MARKED_JS)
        .replace("{{HIGHLIGHT_JS}}", HIGHLIGHT_JS)
        .replace("{{SESSION_DATA}}", &base64_encode(session_json.as_bytes()))
}

fn base64_encode(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut output = String::with_capacity(bytes.len().div_ceil(3) * 4);

    for chunk in bytes.chunks(3) {
        let b0 = chunk[0];
        let b1 = *chunk.get(1).unwrap_or(&0);
        let b2 = *chunk.get(2).unwrap_or(&0);

        output.push(TABLE[(b0 >> 2) as usize] as char);
        output.push(TABLE[(((b0 & 0b0000_0011) << 4) | (b1 >> 4)) as usize] as char);
        if chunk.len() > 1 {
            output.push(TABLE[(((b1 & 0b0000_1111) << 2) | (b2 >> 6)) as usize] as char);
        } else {
            output.push('=');
        }
        if chunk.len() > 2 {
            output.push(TABLE[(b2 & 0b0011_1111) as usize] as char);
        } else {
            output.push('=');
        }
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embeds_json_without_breaking_script_tag() {
        let html = render_template(r#"{"text":"</script><div>&"}"#, &ExportTheme::resolve(None));

        assert!(html.contains("eyJ0ZXh0IjoiPC9zY3JpcHQ+"));
        assert!(!html.contains("</script><div>"));
    }

    #[test]
    fn template_contains_skill_block_rendering_hooks_like_pi() {
        assert!(TEMPLATE_JS.contains("parseSkillBlock"));
        assert!(TEMPLATE_JS.contains("skillBlock.userMessage"));
        assert!(TEMPLATE_JS.contains("skill-invocation"));
        assert!(TEMPLATE_JS.contains("hasUserContent"));
        assert!(TEMPLATE_JS.contains("skill-user-entry"));
        assert!(TEMPLATE_CSS.contains(".skill-invocation"));
    }

    #[test]
    fn template_escapes_message_image_data_urls_like_pi() {
        assert!(!TEMPLATE_JS.contains("${image.mimeType || \"image/png\"}"));
        assert!(!TEMPLATE_JS.contains("${image.data || \"\"}"));
        assert!(TEMPLATE_JS.contains("escapeHtml(image.mimeType || \"image/png\")"));
        assert!(TEMPLATE_JS.contains("escapeHtml(image.data || \"\")"));
    }

    #[test]
    fn template_contains_markdown_link_sanitizer_hooks_like_pi() {
        assert!(TEMPLATE_JS.contains("link(token)"));
        assert!(TEMPLATE_JS.contains("image(token)"));
        assert!(TEMPLATE_JS.contains("javascript"));
        assert!(TEMPLATE_JS.contains("vbscript"));
        assert!(TEMPLATE_JS.contains("!href.startsWith(\"data:\")"));
        assert!(TEMPLATE_JS.contains("escapeHtml(href)"));
    }

    #[test]
    fn template_preserves_ansi_line_whitespace_like_pi() {
        assert!(TEMPLATE_CSS.contains(".ansi-line"));
        assert!(TEMPLATE_CSS.contains("white-space: pre;"));
    }

    #[test]
    fn render_template_embeds_marked_and_highlight_vendor_assets_like_pi() {
        let html = render_template(r#"{"entries":[]}"#, &ExportTheme::resolve(None));

        assert!(!html.contains("{{MARKED_JS}}"));
        assert!(!html.contains("{{HIGHLIGHT_JS}}"));
        assert!(html.contains("marked"));
        assert!(html.contains("hljs"));
    }

    #[test]
    fn template_renders_message_markdown_with_marked_like_pi() {
        assert!(TEMPLATE_JS.contains("function safeMarkedParse"));
        assert!(TEMPLATE_JS.contains("marked.use"));
        assert!(TEMPLATE_JS.contains("hljs.highlight"));
        assert!(TEMPLATE_JS.contains("safeMarkedParse(text)"));
        assert!(TEMPLATE_JS
            .contains("appendMarkdown(body, \"assistant-text markdown-content\", block.text)"));
        assert!(TEMPLATE_JS.contains("safeMarkedParse(skillBlock.content)"));
        assert!(TEMPLATE_JS
            .contains("appendMarkdown(user, \"markdown-content\", skillBlock.userMessage)"));
        assert!(!TEMPLATE_JS.contains("prompt.textContent = skillBlock.userMessage"));
        assert!(!TEMPLATE_JS.contains("content.textContent = skillBlock.content"));
    }

    #[test]
    fn template_renders_tool_calls_with_results_like_pi() {
        assert!(TEMPLATE_JS.contains("function findToolResult"));
        assert!(TEMPLATE_JS.contains("function renderToolCall"));
        assert!(TEMPLATE_JS.contains("function messageRole"));
        assert!(TEMPLATE_JS.contains("block.type === \"toolCall\""));
        assert!(TEMPLATE_JS.contains("appendToolResult"));
        assert!(TEMPLATE_JS.contains("role === \"toolresult\""));
        assert!(TEMPLATE_JS.contains("return;"));
    }

    #[test]
    fn template_styles_tool_rendering_like_pi() {
        assert!(TEMPLATE_CSS.contains(".tool-execution"));
        assert!(TEMPLATE_CSS.contains(".tool-execution.success"));
        assert!(TEMPLATE_CSS.contains(".tool-execution.error"));
        assert!(TEMPLATE_CSS.contains(".tool-header"));
        assert!(TEMPLATE_CSS.contains(".tool-output"));
        assert!(TEMPLATE_CSS.contains(".tool-image"));
        assert!(TEMPLATE_CSS.contains(".hljs"));
    }

    #[test]
    fn render_template_embeds_session_data_as_base64_like_pi() {
        let html = render_template(r#"{"text":"中文</script>"}"#, &ExportTheme::resolve(None));

        assert!(TEMPLATE_JS.contains("atob"));
        assert!(TEMPLATE_JS.contains("TextDecoder"));
        assert!(!html.contains(r#"{"text":"中文"#));
        assert!(html.contains("eyJ0ZXh0Ijoi"));
    }

    #[test]
    fn template_supports_jsonl_download_like_pi() {
        assert!(TEMPLATE_HTML.contains("download-json-btn"));
        assert!(TEMPLATE_JS.contains("window.downloadSessionJson"));
        assert!(TEMPLATE_JS.contains("application/x-ndjson"));
        assert!(TEMPLATE_JS.contains("JSON.stringify({ type: \"header\", ...header })"));
        assert!(TEMPLATE_JS.contains("a.download = `${header.id || \"session\"}.jsonl`"));
        assert!(TEMPLATE_CSS.contains(".download-json-btn"));
    }

    #[test]
    fn template_supports_entry_deep_links_and_copy_links_like_pi() {
        assert!(TEMPLATE_JS.contains("const urlParams = new URLSearchParams"));
        assert!(TEMPLATE_JS.contains("const targetId = urlParams.get(\"targetId\")"));
        assert!(TEMPLATE_JS.contains("item.id = entry.id"));
        assert!(TEMPLATE_JS.contains("copyEntryLink"));
        assert!(TEMPLATE_JS.contains("navigator.clipboard.writeText"));
        assert!(TEMPLATE_JS.contains("scrollIntoView"));
        assert!(TEMPLATE_CSS.contains(".entry-target"));
        assert!(TEMPLATE_CSS.contains(".copy-link-btn"));
    }

    #[test]
    fn template_renders_session_stats_and_system_prompt_like_pi() {
        assert!(TEMPLATE_JS.contains("const systemPrompt = data.systemPrompt"));
        assert!(TEMPLATE_JS.contains("function computeStats"));
        assert!(TEMPLATE_JS.contains("toolCalls"));
        assert!(TEMPLATE_JS.contains("usage.cacheRead"));
        assert!(TEMPLATE_JS.contains("renderSessionInfo"));
        assert!(TEMPLATE_JS.contains("System Prompt"));
        assert!(TEMPLATE_CSS.contains(".session-info"));
        assert!(TEMPLATE_CSS.contains(".system-prompt"));
    }

    #[test]
    fn template_renders_available_tools_list_like_pi() {
        assert!(TEMPLATE_JS.contains("const tools = Array.isArray(data.tools)"));
        assert!(TEMPLATE_JS.contains("function renderToolsList"));
        assert!(TEMPLATE_JS.contains("Available Tools"));
        assert!(TEMPLATE_JS.contains("tool.parameters"));
        assert!(TEMPLATE_JS.contains("params-expanded"));
        assert!(TEMPLATE_CSS.contains(".tools-list"));
        assert!(TEMPLATE_CSS.contains(".tool-param-required"));
    }

    #[test]
    fn template_supports_thinking_and_tool_output_toggles_like_pi() {
        assert!(TEMPLATE_HTML.contains("data-action=\"toggle-thinking\""));
        assert!(TEMPLATE_HTML.contains("data-action=\"toggle-tools\""));
        assert!(TEMPLATE_JS.contains("function toggleThinking"));
        assert!(TEMPLATE_JS.contains("function toggleToolOutputs"));
        assert!(TEMPLATE_JS.contains("document.addEventListener(\"keydown\""));
        assert!(TEMPLATE_JS.contains(".thinking-text"));
        assert!(TEMPLATE_JS.contains(".thinking-collapsed"));
        assert!(TEMPLATE_JS.contains(".tool-output.expandable"));
        assert!(TEMPLATE_CSS.contains(".header-toggle-btn"));
        assert!(TEMPLATE_CSS.contains(".tool-output.expandable.expanded"));
    }

    #[test]
    fn template_supports_search_and_filter_controls_like_pi() {
        assert!(TEMPLATE_HTML.contains("id=\"entry-search\""));
        assert!(TEMPLATE_HTML.contains("data-filter=\"user-only\""));
        assert!(TEMPLATE_HTML.contains("data-filter=\"no-tools\""));
        assert!(TEMPLATE_JS.contains("function applyEntryFilters"));
        assert!(TEMPLATE_JS.contains("function entryMatchesFilter"));
        assert!(TEMPLATE_JS.contains("function entrySearchText"));
        assert!(TEMPLATE_JS.contains("entry-filter-hidden"));
        assert!(TEMPLATE_JS.contains("keydown"));
        assert!(TEMPLATE_CSS.contains(".entry-search"));
        assert!(TEMPLATE_CSS.contains(".filter-btn.active"));
        assert!(TEMPLATE_CSS.contains(".entry-filter-hidden"));
    }

    #[test]
    fn template_uses_default_filter_to_hide_settings_entries_like_pi() {
        assert!(TEMPLATE_HTML.contains("data-filter=\"default\""));
        assert!(TEMPLATE_HTML.contains("filter-btn active\" data-filter=\"default\""));
        assert!(TEMPLATE_JS.contains("let filterMode = \"default\""));
        assert!(TEMPLATE_JS.contains("function isSettingsEntry"));
        assert!(TEMPLATE_JS.contains("filterMode === \"default\""));
        assert!(TEMPLATE_JS.contains("entry.type === \"label\""));
        assert!(TEMPLATE_JS.contains("entry.type === \"model_change\""));
        assert!(TEMPLATE_JS.contains("entry.type === \"thinking_level_change\""));
    }

    #[test]
    fn template_renders_sidebar_tree_navigation_like_pi() {
        assert!(TEMPLATE_HTML.contains("id=\"sidebar-tree\""));
        assert!(TEMPLATE_HTML.contains("id=\"content\""));
        assert!(TEMPLATE_JS.contains("function buildEntryTree"));
        assert!(TEMPLATE_JS.contains("function renderSidebarTree"));
        assert!(TEMPLATE_JS.contains("function sidebarEntryLabel"));
        assert!(TEMPLATE_JS.contains("sidebar-tree-node"));
        assert!(TEMPLATE_JS.contains("scrollToEntry"));
        assert!(TEMPLATE_CSS.contains(".export-shell"));
        assert!(TEMPLATE_CSS.contains(".sidebar"));
        assert!(TEMPLATE_CSS.contains(".sidebar-tree-node"));
    }

    #[test]
    fn template_supports_leaf_path_navigation_like_pi() {
        assert!(TEMPLATE_JS.contains("const urlLeafId = urlParams.get(\"leafId\")"));
        assert!(TEMPLATE_JS.contains("let currentLeafId"));
        assert!(TEMPLATE_JS.contains("function getPath"));
        assert!(TEMPLATE_JS.contains("function findNewestLeaf"));
        assert!(TEMPLATE_JS.contains("function buildActivePathIds"));
        assert!(TEMPLATE_JS.contains("function navigateTo"));
        assert!(TEMPLATE_JS.contains("sidebar-active-path"));
        assert!(TEMPLATE_JS.contains("sidebar-current-leaf"));
        assert!(TEMPLATE_CSS.contains(".sidebar-active-path"));
        assert!(TEMPLATE_CSS.contains(".sidebar-current-leaf"));
    }

    #[test]
    fn template_syncs_sidebar_with_search_and_filters_like_pi() {
        assert!(TEMPLATE_JS.contains("function syncSidebarFilters"));
        assert!(TEMPLATE_JS.contains("sidebar-filter-hidden"));
        assert!(TEMPLATE_JS.contains("entryMatchesFilter(entry, terms)"));
        assert!(TEMPLATE_JS.contains("buildActivePathIds(currentLeafId)"));
        assert!(TEMPLATE_JS.contains("sidebarVisible"));
        assert!(TEMPLATE_CSS.contains(".sidebar-filter-hidden"));
    }

    #[test]
    fn template_supports_labeled_sidebar_filter_like_pi() {
        assert!(TEMPLATE_HTML.contains("data-filter=\"labeled-only\""));
        assert!(TEMPLATE_JS.contains("const labelMap = new Map()"));
        assert!(TEMPLATE_JS.contains("labelMap.set(entry.targetId, entry.label)"));
        assert!(TEMPLATE_JS.contains("labelMap.get(entry.id)"));
        assert!(TEMPLATE_JS.contains("filterMode === \"labeled-only\""));
        assert!(TEMPLATE_JS.contains("entryLabel(entry)"));
        assert!(TEMPLATE_JS.contains("[${escapeHtml(label)}]"));
        assert!(TEMPLATE_CSS.contains(".sidebar-label"));
    }

    #[test]
    fn template_supports_image_modal_like_pi() {
        assert!(TEMPLATE_HTML.contains("id=\"image-modal\""));
        assert!(TEMPLATE_HTML.contains("id=\"modal-image\""));
        assert!(TEMPLATE_JS.contains("function attachImageModalHandlers"));
        assert!(TEMPLATE_JS.contains(".message-image, .tool-image"));
        assert!(TEMPLATE_JS.contains("function openImageModal"));
        assert!(TEMPLATE_JS.contains("function closeImageModal"));
        assert!(TEMPLATE_JS.contains("event.key === \"Escape\""));
        assert!(TEMPLATE_CSS.contains(".image-modal"));
        assert!(TEMPLATE_CSS.contains(".image-modal.open"));
    }

    #[test]
    fn template_renders_pre_rendered_custom_tools_like_pi() {
        assert!(TEMPLATE_JS.contains("const renderedTools = data.renderedTools"));
        assert!(TEMPLATE_JS.contains("const rendered = renderedTools[block.id]"));
        assert!(TEMPLATE_JS.contains("rendered.callHtml"));
        assert!(TEMPLATE_JS.contains("resultHtmlCollapsed"));
        assert!(TEMPLATE_JS.contains("resultHtmlExpanded"));
        assert!(TEMPLATE_JS.contains("ansi-rendered"));
        assert!(TEMPLATE_JS.contains("output-preview"));
        assert!(TEMPLATE_JS.contains("output-full"));
        assert!(TEMPLATE_CSS.contains(".ansi-rendered"));
        assert!(TEMPLATE_CSS.contains(".output-preview"));
        assert!(TEMPLATE_CSS.contains(".output-full"));
    }

    #[test]
    fn template_renders_builtin_file_tools_like_pi() {
        assert!(TEMPLATE_JS.contains("function renderBuiltinToolCall"));
        assert!(TEMPLATE_JS.contains("case \"read\""));
        assert!(TEMPLATE_JS.contains("case \"write\""));
        assert!(TEMPLATE_JS.contains("case \"ls\""));
        assert!(TEMPLATE_JS.contains("function shortenPath"));
        assert!(TEMPLATE_JS.contains("function formatExpandableOutput"));
        assert!(TEMPLATE_JS.contains("function appendToolHeader"));
        assert!(TEMPLATE_JS.contains("tool-name"));
        assert!(TEMPLATE_JS.contains("tool-path"));
        assert!(TEMPLATE_JS.contains("line-count"));
        assert!(TEMPLATE_CSS.contains(".tool-name"));
        assert!(TEMPLATE_CSS.contains(".tool-path"));
        assert!(TEMPLATE_CSS.contains(".line-count"));
    }

    #[test]
    fn template_renders_edit_diff_like_pi() {
        assert!(TEMPLATE_JS.contains("case \"edit\""));
        assert!(TEMPLATE_JS.contains("result.details.diff"));
        assert!(TEMPLATE_JS.contains("function appendToolDiff"));
        assert!(TEMPLATE_JS.contains("tool-diff"));
        assert!(TEMPLATE_JS.contains("diff-added"));
        assert!(TEMPLATE_JS.contains("diff-removed"));
        assert!(TEMPLATE_JS.contains("diff-context"));
        assert!(TEMPLATE_CSS.contains(".tool-diff"));
        assert!(TEMPLATE_CSS.contains(".diff-added"));
        assert!(TEMPLATE_CSS.contains(".diff-removed"));
        assert!(TEMPLATE_CSS.contains(".diff-context"));
    }

    #[test]
    fn template_renders_bash_tool_like_pi() {
        assert!(TEMPLATE_JS.contains("case \"bash\""));
        assert!(TEMPLATE_JS.contains("function appendToolCommand"));
        assert!(TEMPLATE_JS.contains("args.command"));
        assert!(TEMPLATE_JS.contains("appendExpandableOutput(card, output, 5)"));
        assert!(TEMPLATE_JS.contains("tool-command"));
        assert!(TEMPLATE_CSS.contains(".tool-command"));
    }

    #[test]
    fn template_formats_tool_call_summaries_like_pi() {
        assert!(TEMPLATE_JS.contains("function formatToolCall"));
        assert!(TEMPLATE_JS.contains("case \"grep\""));
        assert!(TEMPLATE_JS.contains("case \"find\""));
        assert!(TEMPLATE_JS.contains("[grep: /${params.pattern || \"\"}/ in"));
        assert!(TEMPLATE_JS.contains("[find: ${params.pattern || \"\"} in"));
        assert!(TEMPLATE_JS.contains("formatToolCall(block.name, block.arguments)"));
        assert!(TEMPLATE_JS.contains("formatToolCall(toolCall.name, toolCall.arguments)"));
    }
}
