use crate::tools::truncate::{
    truncate_tail, TruncationOptions, DEFAULT_MAX_BYTES, DEFAULT_MAX_LINES,
};
use crate::utils::strip_ansi;

const PREVIEW_LINES: usize = 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BashExecutionStatus {
    Running,
    Complete,
    Cancelled,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BashExecutionLine {
    Command(String),
    Output(String),
    Status(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BashExecutionState {
    command: String,
    output_lines: Vec<String>,
    status: BashExecutionStatus,
    exit_code: Option<i32>,
    context_truncated: bool,
    full_output_path: Option<String>,
    expanded: bool,
    exclude_from_context: bool,
}

impl BashExecutionState {
    pub fn new(command: String, exclude_from_context: bool) -> Self {
        Self {
            command,
            output_lines: Vec::new(),
            status: BashExecutionStatus::Running,
            exit_code: None,
            context_truncated: false,
            full_output_path: None,
            expanded: false,
            exclude_from_context,
        }
    }

    pub fn command(&self) -> &str {
        &self.command
    }

    pub fn output(&self) -> String {
        self.output_lines.join("\n")
    }

    pub fn status(&self) -> BashExecutionStatus {
        self.status
    }

    pub fn expanded(&self) -> bool {
        self.expanded
    }

    pub fn exclude_from_context(&self) -> bool {
        self.exclude_from_context
    }

    pub fn set_expanded(&mut self, expanded: bool) {
        self.expanded = expanded;
    }

    pub fn append_output(&mut self, chunk: &str) {
        let clean = strip_ansi(chunk).replace("\r\n", "\n").replace('\r', "\n");
        let new_lines = clean.split('\n').map(str::to_string).collect::<Vec<_>>();

        if !self.output_lines.is_empty() && !new_lines.is_empty() {
            if let Some(last_line) = self.output_lines.last_mut() {
                last_line.push_str(&new_lines[0]);
            }
            self.output_lines.extend(new_lines.into_iter().skip(1));
        } else {
            self.output_lines.extend(new_lines);
        }
    }

    pub fn set_complete(
        &mut self,
        exit_code: Option<i32>,
        cancelled: bool,
        truncated: bool,
        full_output_path: Option<String>,
    ) {
        self.exit_code = exit_code;
        self.status = if cancelled {
            BashExecutionStatus::Cancelled
        } else if exit_code.is_some_and(|code| code != 0) {
            BashExecutionStatus::Error
        } else {
            BashExecutionStatus::Complete
        };
        self.context_truncated = truncated;
        self.full_output_path = full_output_path;
    }

    pub fn render_lines(
        &self,
        cancel_key_text: &str,
        expand_key_hint: &str,
    ) -> Vec<BashExecutionLine> {
        let full_output = self.output();
        let context_truncation = truncate_tail(
            &full_output,
            TruncationOptions {
                max_lines: DEFAULT_MAX_LINES,
                max_bytes: DEFAULT_MAX_BYTES,
            },
        );
        let available_lines = if context_truncation.content.is_empty() {
            Vec::new()
        } else {
            context_truncation
                .content
                .split('\n')
                .map(str::to_string)
                .collect::<Vec<_>>()
        };
        let preview_start = available_lines.len().saturating_sub(PREVIEW_LINES);
        let hidden_line_count = preview_start;
        let display_lines = if self.expanded {
            &available_lines[..]
        } else {
            &available_lines[preview_start..]
        };

        let mut lines = vec![BashExecutionLine::Command(format!("$ {}", self.command))];
        lines.extend(display_lines.iter().cloned().map(BashExecutionLine::Output));

        if self.status == BashExecutionStatus::Running {
            lines.push(BashExecutionLine::Status(format!(
                "Running... ({cancel_key_text} to cancel)"
            )));
            return lines;
        }

        if hidden_line_count > 0 {
            if self.expanded {
                lines.push(BashExecutionLine::Status(format!(
                    "({expand_key_hint} to collapse)"
                )));
            } else {
                lines.push(BashExecutionLine::Status(format!(
                    "... {hidden_line_count} more lines ({expand_key_hint} to expand)"
                )));
            }
        }

        match self.status {
            BashExecutionStatus::Cancelled => {
                lines.push(BashExecutionLine::Status("(cancelled)".to_string()));
            }
            BashExecutionStatus::Error => {
                if let Some(exit_code) = self.exit_code {
                    lines.push(BashExecutionLine::Status(format!("(exit {exit_code})")));
                }
            }
            BashExecutionStatus::Running | BashExecutionStatus::Complete => {}
        }

        if self.context_truncated || context_truncation.truncated {
            if let Some(path) = &self.full_output_path {
                lines.push(BashExecutionLine::Status(format!(
                    "Output truncated. Full output: {path}"
                )));
            }
        }

        lines
    }
}

#[cfg(test)]
mod tests {
    use super::{BashExecutionLine, BashExecutionState, BashExecutionStatus};

    #[test]
    fn bash_execution_appends_chunks_strips_ansi_and_normalizes_line_endings() {
        let mut state = BashExecutionState::new("echo hi".to_string(), false);

        state.append_output("\x1b[31mhe");
        state.append_output("llo\r\nworld\ragain");

        assert_eq!(state.output(), "hello\nworld\nagain");
        assert_eq!(state.command(), "echo hi");
    }

    #[test]
    fn bash_execution_complete_sets_status_from_exit_code_and_cancelled_flag() {
        let mut state = BashExecutionState::new("run".to_string(), false);

        state.set_complete(Some(2), false, false, None);
        assert_eq!(state.status(), BashExecutionStatus::Error);

        state.set_complete(Some(0), false, false, None);
        assert_eq!(state.status(), BashExecutionStatus::Complete);

        state.set_complete(None, true, false, None);
        assert_eq!(state.status(), BashExecutionStatus::Cancelled);
    }

    #[test]
    fn bash_execution_collapsed_render_shows_last_twenty_lines_and_expand_hint() {
        let mut state = BashExecutionState::new("seq 25".to_string(), false);
        state.append_output(
            &(1..=25)
                .map(|line| format!("line {line}"))
                .collect::<Vec<_>>()
                .join("\n"),
        );
        state.set_complete(Some(0), false, false, None);

        let lines = state.render_lines("esc", "ctrl+o");

        assert_eq!(
            lines.first(),
            Some(&BashExecutionLine::Command("$ seq 25".to_string()))
        );
        assert!(lines.contains(&BashExecutionLine::Output("line 6".to_string())));
        assert!(!lines.contains(&BashExecutionLine::Output("line 5".to_string())));
        assert_eq!(
            lines.last(),
            Some(&BashExecutionLine::Status(
                "... 5 more lines (ctrl+o to expand)".to_string()
            ))
        );
    }

    #[test]
    fn bash_execution_expanded_render_shows_all_available_lines_and_collapse_hint() {
        let mut state = BashExecutionState::new("seq 21".to_string(), false);
        state.append_output(
            &(1..=21)
                .map(|line| format!("line {line}"))
                .collect::<Vec<_>>()
                .join("\n"),
        );
        state.set_expanded(true);
        state.set_complete(Some(0), false, false, None);

        let lines = state.render_lines("esc", "ctrl+o");

        assert!(lines.contains(&BashExecutionLine::Output("line 1".to_string())));
        assert_eq!(
            lines.last(),
            Some(&BashExecutionLine::Status(
                "(ctrl+o to collapse)".to_string()
            ))
        );
    }

    #[test]
    fn bash_execution_render_includes_running_error_cancelled_and_truncation_status() {
        let mut running = BashExecutionState::new("sleep".to_string(), false);
        assert!(running
            .render_lines("esc", "ctrl+o")
            .contains(&BashExecutionLine::Status(
                "Running... (esc to cancel)".to_string()
            )));

        running.set_complete(Some(9), false, true, Some("/tmp/full.log".to_string()));
        let lines = running.render_lines("esc", "ctrl+o");
        assert!(lines.contains(&BashExecutionLine::Status("(exit 9)".to_string())));
        assert!(lines.contains(&BashExecutionLine::Status(
            "Output truncated. Full output: /tmp/full.log".to_string()
        )));

        running.set_complete(None, true, false, None);
        assert!(running
            .render_lines("esc", "ctrl+o")
            .contains(&BashExecutionLine::Status("(cancelled)".to_string())));
    }
}
