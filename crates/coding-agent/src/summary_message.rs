#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SummaryMessageLine {
    Label(String),
    Blank,
    Text(String),
    Markdown(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchSummaryMessageState {
    summary: String,
    expanded: bool,
}

impl BranchSummaryMessageState {
    pub fn new(summary: String) -> Self {
        Self {
            summary,
            expanded: false,
        }
    }

    pub fn expanded(&self) -> bool {
        self.expanded
    }

    pub fn set_expanded(&mut self, expanded: bool) {
        self.expanded = expanded;
    }

    pub fn render_lines(&self, expand_key_text: &str) -> Vec<SummaryMessageLine> {
        let mut lines = vec![
            SummaryMessageLine::Label("[branch]".to_string()),
            SummaryMessageLine::Blank,
        ];

        if self.expanded {
            lines.push(SummaryMessageLine::Markdown(format!(
                "**Branch Summary**\n\n{}",
                self.summary
            )));
        } else {
            lines.push(SummaryMessageLine::Text(format!(
                "Branch summary ({expand_key_text} to expand)"
            )));
        }

        lines
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompactionSummaryMessageState {
    summary: String,
    tokens_before: u64,
    expanded: bool,
}

impl CompactionSummaryMessageState {
    pub fn new(summary: String, tokens_before: u64) -> Self {
        Self {
            summary,
            tokens_before,
            expanded: false,
        }
    }

    pub fn expanded(&self) -> bool {
        self.expanded
    }

    pub fn set_expanded(&mut self, expanded: bool) {
        self.expanded = expanded;
    }

    pub fn render_lines(&self, expand_key_text: &str) -> Vec<SummaryMessageLine> {
        let token_text = format_grouped_u64(self.tokens_before);
        let mut lines = vec![
            SummaryMessageLine::Label("[compaction]".to_string()),
            SummaryMessageLine::Blank,
        ];

        if self.expanded {
            lines.push(SummaryMessageLine::Markdown(format!(
                "**Compacted from {token_text} tokens**\n\n{}",
                self.summary
            )));
        } else {
            lines.push(SummaryMessageLine::Text(format!(
                "Compacted from {token_text} tokens ({expand_key_text} to expand)"
            )));
        }

        lines
    }
}

fn format_grouped_u64(value: u64) -> String {
    let digits = value.to_string();
    let mut formatted = String::with_capacity(digits.len() + digits.len() / 3);

    for (index, digit) in digits.chars().enumerate() {
        let remaining = digits.len() - index;
        formatted.push(digit);
        if remaining > 1 && remaining % 3 == 1 {
            formatted.push(',');
        }
    }

    formatted
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillInvocationMessageState {
    name: String,
    content: String,
    expanded: bool,
}

impl SkillInvocationMessageState {
    pub fn new(name: String, content: String) -> Self {
        Self {
            name,
            content,
            expanded: false,
        }
    }

    pub fn expanded(&self) -> bool {
        self.expanded
    }

    pub fn set_expanded(&mut self, expanded: bool) {
        self.expanded = expanded;
    }

    pub fn render_lines(&self, expand_key_text: &str) -> Vec<SummaryMessageLine> {
        if self.expanded {
            vec![
                SummaryMessageLine::Label("[skill]".to_string()),
                SummaryMessageLine::Markdown(format!("**{}**\n\n{}", self.name, self.content)),
            ]
        } else {
            vec![SummaryMessageLine::Text(format!(
                "[skill] {} ({expand_key_text} to expand)",
                self.name
            ))]
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MessageContentBlock {
    Text(String),
    Image,
    Other(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CustomMessageContent {
    Text(String),
    Blocks(Vec<MessageContentBlock>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CustomMessageState {
    custom_type: String,
    content: CustomMessageContent,
}

impl CustomMessageState {
    pub fn new(custom_type: String, content: CustomMessageContent) -> Self {
        Self {
            custom_type,
            content,
        }
    }

    pub fn render_lines(&self) -> Vec<SummaryMessageLine> {
        vec![
            SummaryMessageLine::Label(format!("[{}]", self.custom_type)),
            SummaryMessageLine::Blank,
            SummaryMessageLine::Markdown(self.markdown_text()),
        ]
    }

    fn markdown_text(&self) -> String {
        match &self.content {
            CustomMessageContent::Text(text) => text.clone(),
            CustomMessageContent::Blocks(blocks) => blocks
                .iter()
                .filter_map(|block| match block {
                    MessageContentBlock::Text(text) => Some(text.as_str()),
                    MessageContentBlock::Image | MessageContentBlock::Other(_) => None,
                })
                .collect::<Vec<_>>()
                .join("\n"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        BranchSummaryMessageState, CompactionSummaryMessageState, CustomMessageContent,
        CustomMessageState, MessageContentBlock, SkillInvocationMessageState, SummaryMessageLine,
    };

    #[test]
    fn branch_summary_renders_collapsed_hint_like_pi() {
        let state = BranchSummaryMessageState::new("Created a branch".to_string());

        assert_eq!(
            state.render_lines("ctrl+o"),
            vec![
                SummaryMessageLine::Label("[branch]".to_string()),
                SummaryMessageLine::Blank,
                SummaryMessageLine::Text("Branch summary (ctrl+o to expand)".to_string()),
            ]
        );
    }

    #[test]
    fn branch_summary_renders_expanded_markdown_with_header() {
        let mut state = BranchSummaryMessageState::new("Copied active path".to_string());
        state.set_expanded(true);

        assert_eq!(
            state.render_lines("ctrl+o"),
            vec![
                SummaryMessageLine::Label("[branch]".to_string()),
                SummaryMessageLine::Blank,
                SummaryMessageLine::Markdown(
                    "**Branch Summary**\n\nCopied active path".to_string()
                ),
            ]
        );
    }

    #[test]
    fn compaction_summary_renders_collapsed_hint_with_grouped_tokens() {
        let state = CompactionSummaryMessageState::new("Kept recent context".to_string(), 12345);

        assert_eq!(
            state.render_lines("ctrl+o"),
            vec![
                SummaryMessageLine::Label("[compaction]".to_string()),
                SummaryMessageLine::Blank,
                SummaryMessageLine::Text(
                    "Compacted from 12,345 tokens (ctrl+o to expand)".to_string(),
                ),
            ]
        );
    }

    #[test]
    fn compaction_summary_renders_expanded_markdown_with_token_header() {
        let mut state = CompactionSummaryMessageState::new("Kept decisions".to_string(), 9876543);
        state.set_expanded(true);

        assert_eq!(
            state.render_lines("ctrl+o"),
            vec![
                SummaryMessageLine::Label("[compaction]".to_string()),
                SummaryMessageLine::Blank,
                SummaryMessageLine::Markdown(
                    "**Compacted from 9,876,543 tokens**\n\nKept decisions".to_string(),
                ),
            ]
        );
    }

    #[test]
    fn skill_invocation_renders_collapsed_single_line_like_pi() {
        let state =
            SkillInvocationMessageState::new("planner".to_string(), "Use a plan".to_string());

        assert_eq!(
            state.render_lines("ctrl+o"),
            vec![SummaryMessageLine::Text(
                "[skill] planner (ctrl+o to expand)".to_string()
            )]
        );
    }

    #[test]
    fn skill_invocation_renders_expanded_label_and_markdown() {
        let mut state =
            SkillInvocationMessageState::new("planner".to_string(), "Use a plan".to_string());
        state.set_expanded(true);

        assert_eq!(
            state.render_lines("ctrl+o"),
            vec![
                SummaryMessageLine::Label("[skill]".to_string()),
                SummaryMessageLine::Markdown("**planner**\n\nUse a plan".to_string()),
            ]
        );
    }

    #[test]
    fn custom_message_renders_string_content_with_label_and_markdown() {
        let state = CustomMessageState::new(
            "review".to_string(),
            CustomMessageContent::Text("Looks good".to_string()),
        );

        assert_eq!(
            state.render_lines(),
            vec![
                SummaryMessageLine::Label("[review]".to_string()),
                SummaryMessageLine::Blank,
                SummaryMessageLine::Markdown("Looks good".to_string()),
            ]
        );
    }

    #[test]
    fn custom_message_extracts_only_text_blocks_like_pi_default_renderer() {
        let state = CustomMessageState::new(
            "audit".to_string(),
            CustomMessageContent::Blocks(vec![
                MessageContentBlock::Text("first".to_string()),
                MessageContentBlock::Image,
                MessageContentBlock::Text("second".to_string()),
            ]),
        );

        assert_eq!(
            state.render_lines(),
            vec![
                SummaryMessageLine::Label("[audit]".to_string()),
                SummaryMessageLine::Blank,
                SummaryMessageLine::Markdown("first\nsecond".to_string()),
            ]
        );
    }
}
