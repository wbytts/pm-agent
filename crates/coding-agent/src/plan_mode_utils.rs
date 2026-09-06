#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TodoItem {
    pub step: usize,
    pub text: String,
    pub completed: bool,
}

pub fn is_safe_command(command: &str) -> bool {
    !is_destructive_command(command) && is_read_only_command(command)
}

pub fn clean_step_text(text: &str) -> String {
    let mut cleaned = strip_markdown_marks(text);
    cleaned = strip_leading_action_words(&cleaned);
    cleaned = normalize_whitespace(&cleaned);
    if let Some(first) = cleaned.chars().next() {
        let rest = &cleaned[first.len_utf8()..];
        cleaned = format!("{}{}", first.to_uppercase(), rest);
    }
    if cleaned.len() > 50 {
        cleaned = format!("{}...", &cleaned[..47]);
    }
    cleaned
}

pub fn extract_todo_items(message: &str) -> Vec<TodoItem> {
    let Some(plan_start) = plan_section_start(message) else {
        return Vec::new();
    };
    let mut items = Vec::new();
    for line in message[plan_start..].lines() {
        let Some(raw_text) = numbered_item_text(line) else {
            continue;
        };
        let text = raw_text.trim().trim_end_matches('*').trim();
        if text.len() <= 5 || text.starts_with(['`', '/', '-']) {
            continue;
        }
        let cleaned = clean_step_text(text);
        if cleaned.len() > 3 {
            items.push(TodoItem {
                step: items.len() + 1,
                text: cleaned,
                completed: false,
            });
        }
    }
    items
}

pub fn extract_done_steps(message: &str) -> Vec<usize> {
    let lower = message.to_ascii_lowercase();
    let mut steps = Vec::new();
    let mut index = 0;
    while let Some(start) = lower[index..].find("[done:") {
        let marker_start = index + start + "[done:".len();
        let Some(end) = lower[marker_start..].find(']') else {
            break;
        };
        let value = &message[marker_start..marker_start + end];
        if let Ok(step) = value.parse::<usize>() {
            steps.push(step);
        }
        index = marker_start + end + 1;
    }
    steps
}

pub fn mark_completed_steps(message: &str, items: &mut [TodoItem]) -> usize {
    let done_steps = extract_done_steps(message);
    for step in &done_steps {
        if let Some(item) = items.iter_mut().find(|item| item.step == *step) {
            item.completed = true;
        }
    }
    done_steps.len()
}

fn is_destructive_command(command: &str) -> bool {
    let lower = command.to_ascii_lowercase();
    let tokens = shell_words(&lower);

    if lower.contains(">>") || single_output_redirect(&lower) {
        return true;
    }
    if tokens.iter().any(|token| {
        matches!(
            *token,
            "rm" | "rmdir"
                | "mv"
                | "cp"
                | "mkdir"
                | "touch"
                | "chmod"
                | "chown"
                | "chgrp"
                | "ln"
                | "tee"
                | "truncate"
                | "dd"
                | "shred"
                | "sudo"
                | "su"
                | "kill"
                | "pkill"
                | "killall"
                | "reboot"
                | "shutdown"
                | "vim"
                | "vi"
                | "nano"
                | "emacs"
                | "code"
                | "subl"
        )
    }) {
        return true;
    }

    match tokens.as_slice() {
        ["npm", action, ..] => matches!(
            *action,
            "install" | "uninstall" | "update" | "ci" | "link" | "publish"
        ),
        ["yarn", action, ..] => matches!(*action, "add" | "remove" | "install" | "publish"),
        ["pnpm", action, ..] => matches!(*action, "add" | "remove" | "install" | "publish"),
        ["pip", action, ..] => matches!(*action, "install" | "uninstall"),
        ["apt", action, ..] | ["apt-get", action, ..] => {
            matches!(
                *action,
                "install" | "remove" | "purge" | "update" | "upgrade"
            )
        }
        ["brew", action, ..] => matches!(*action, "install" | "uninstall" | "upgrade"),
        ["git", action, rest @ ..] => git_action_is_destructive(action, rest),
        ["systemctl", action, ..] => {
            matches!(*action, "start" | "stop" | "restart" | "enable" | "disable")
        }
        ["service", _, action, ..] => matches!(*action, "start" | "stop" | "restart"),
        _ => false,
    }
}

fn is_read_only_command(command: &str) -> bool {
    let lower = command.trim_start().to_ascii_lowercase();
    let tokens = shell_words(&lower);
    match tokens.as_slice() {
        [command, ..]
            if matches!(
                *command,
                "cat"
                    | "head"
                    | "tail"
                    | "less"
                    | "more"
                    | "grep"
                    | "find"
                    | "ls"
                    | "pwd"
                    | "echo"
                    | "printf"
                    | "wc"
                    | "sort"
                    | "uniq"
                    | "diff"
                    | "file"
                    | "stat"
                    | "du"
                    | "df"
                    | "tree"
                    | "which"
                    | "whereis"
                    | "type"
                    | "env"
                    | "printenv"
                    | "uname"
                    | "whoami"
                    | "id"
                    | "date"
                    | "cal"
                    | "uptime"
                    | "ps"
                    | "top"
                    | "htop"
                    | "free"
                    | "jq"
                    | "awk"
                    | "rg"
                    | "fd"
                    | "bat"
                    | "eza"
            ) =>
        {
            true
        }
        ["git", action, rest @ ..] => {
            matches!(
                *action,
                "status" | "log" | "diff" | "show" | "branch" | "remote"
            ) || (*action == "config" && rest.first().copied() == Some("--get"))
                || action.starts_with("ls-")
        }
        ["npm", action, ..] => matches!(
            *action,
            "list" | "ls" | "view" | "info" | "search" | "outdated" | "audit"
        ),
        ["yarn", action, ..] => matches!(*action, "list" | "info" | "why" | "audit"),
        ["node", "--version", ..] | ["python", "--version", ..] => true,
        ["curl", ..] => true,
        ["wget", "-o", "-", ..] => true,
        ["sed", "-n", ..] => true,
        _ => false,
    }
}

fn git_action_is_destructive(action: &str, rest: &[&str]) -> bool {
    matches!(
        action,
        "add"
            | "commit"
            | "push"
            | "pull"
            | "merge"
            | "rebase"
            | "reset"
            | "checkout"
            | "stash"
            | "cherry-pick"
            | "revert"
            | "tag"
            | "init"
            | "clone"
    ) || (action == "branch" && rest.first().is_some_and(|arg| *arg == "-d" || *arg == "-D"))
}

fn single_output_redirect(command: &str) -> bool {
    let bytes = command.as_bytes();
    for (index, byte) in bytes.iter().enumerate() {
        if *byte == b'>' {
            let previous = index.checked_sub(1).and_then(|i| bytes.get(i)).copied();
            let next = bytes.get(index + 1).copied();
            if previous != Some(b'<') && next != Some(b'>') {
                return true;
            }
        }
    }
    false
}

fn shell_words(command: &str) -> Vec<&str> {
    command.split_whitespace().collect()
}

fn strip_markdown_marks(text: &str) -> String {
    let without_code = strip_wrapped_segments(text, '`');
    strip_emphasis(&without_code)
}

fn strip_wrapped_segments(text: &str, marker: char) -> String {
    let mut output = String::new();
    let mut inside = false;
    for ch in text.chars() {
        if ch == marker {
            inside = !inside;
            continue;
        }
        output.push(ch);
    }
    output
}

fn strip_emphasis(text: &str) -> String {
    text.chars().filter(|ch| *ch != '*').collect()
}

fn strip_leading_action_words(text: &str) -> String {
    const ACTIONS: &[&str] = &[
        "use", "run", "execute", "create", "write", "read", "check", "verify", "update", "modify",
        "add", "remove", "delete", "install",
    ];
    let trimmed = text.trim_start();
    let lower = trimmed.to_ascii_lowercase();
    for action in ACTIONS {
        let Some(rest) = lower.strip_prefix(action) else {
            continue;
        };
        if rest.starts_with(char::is_whitespace) {
            let original_rest = &trimmed[action.len()..].trim_start();
            if let Some(after_the) = original_rest
                .to_ascii_lowercase()
                .strip_prefix("the ")
                .map(|_| &original_rest[4..])
            {
                return after_the.to_string();
            }
            return original_rest.to_string();
        }
    }
    trimmed.to_string()
}

fn normalize_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn plan_section_start(message: &str) -> Option<usize> {
    let lower = message.to_ascii_lowercase();
    let candidates = ["plan:\n", "plan:\r\n", "**plan:**\n", "**plan:**\r\n"];
    candidates
        .iter()
        .filter_map(|candidate| lower.find(candidate).map(|index| index + candidate.len()))
        .min()
}

fn numbered_item_text(line: &str) -> Option<&str> {
    let trimmed = line.trim_start();
    let digit_len = trimmed
        .chars()
        .take_while(|ch| ch.is_ascii_digit())
        .map(char::len_utf8)
        .sum::<usize>();
    if digit_len == 0 {
        return None;
    }
    let rest = &trimmed[digit_len..];
    let rest = rest.strip_prefix('.').or_else(|| rest.strip_prefix(')'))?;
    rest.strip_prefix(char::is_whitespace).map(str::trim_start)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_command_detection_matches_plan_mode_example() {
        for command in [
            "ls -la",
            "cat file.txt",
            "head -n 10 file.txt",
            "tail -f log.txt",
            "grep pattern file",
            "find . -name '*.ts'",
            "git status",
            "git log --oneline",
            "git diff",
            "git branch",
            "npm list",
            "npm outdated",
            "yarn info react",
            "pwd",
            "echo hello",
            "wc -l file.txt",
            "du -sh .",
            "df -h",
            "  ls -la",
        ] {
            assert!(is_safe_command(command), "{command} should be safe");
        }

        for command in [
            "rm file.txt",
            "rm -rf dir",
            "mv old new",
            "cp src dst",
            "mkdir newdir",
            "touch newfile",
            "git add .",
            "git commit -m 'msg'",
            "git push",
            "git checkout main",
            "git reset --hard",
            "npm install lodash",
            "yarn add react",
            "pip install requests",
            "brew install node",
            "echo hello > file.txt",
            "cat foo >> bar",
            ">file.txt",
            "sudo rm -rf /",
            "kill -9 1234",
            "reboot",
            "vim file.txt",
            "nano file.txt",
            "code .",
            "unknown-command",
            "my-script.sh",
            "  rm file",
        ] {
            assert!(!is_safe_command(command), "{command} should be blocked");
        }
    }

    #[test]
    fn clean_step_text_matches_plan_mode_example() {
        assert_eq!(clean_step_text("**bold text**"), "Bold text");
        assert_eq!(clean_step_text("*italic text*"), "Italic text");
        assert_eq!(clean_step_text("run `npm install`"), "Npm install");
        assert_eq!(
            clean_step_text("check the `config.json` file"),
            "Config.json file"
        );
        assert_eq!(clean_step_text("Create the new file"), "New file");
        assert_eq!(clean_step_text("Run the tests"), "Tests");
        assert_eq!(clean_step_text("Check the status"), "Status");
        assert_eq!(clean_step_text("update config"), "Config");
        assert_eq!(
            clean_step_text("multiple   spaces   here"),
            "Multiple spaces here"
        );

        let result = clean_step_text(
            "This is a very long step description that exceeds the maximum allowed length for display",
        );
        assert_eq!(result.len(), 50);
        assert!(result.ends_with("..."));
    }

    #[test]
    fn extracts_todo_items_after_plan_header_like_example() {
        let items = extract_todo_items(
            "Here's what we'll do:\n\nPlan:\n1. First step here\n2. Second step here\n3. Third step here",
        );
        assert_eq!(
            items,
            vec![
                TodoItem {
                    step: 1,
                    text: "First step here".to_string(),
                    completed: false,
                },
                TodoItem {
                    step: 2,
                    text: "Second step here".to_string(),
                    completed: false,
                },
                TodoItem {
                    step: 3,
                    text: "Third step here".to_string(),
                    completed: false,
                },
            ]
        );

        assert_eq!(extract_todo_items("**Plan:**\n1. Do something").len(), 1);
        assert_eq!(
            extract_todo_items("Plan:\n1) First item\n2) Second item").len(),
            2
        );
        assert!(
            extract_todo_items("Here are some steps:\n1. First step\n2. Second step").is_empty()
        );
        assert_eq!(
            extract_todo_items("Plan:\n1. OK\n2. This is a proper step")
                .first()
                .map(|item| item.text.as_str()),
            Some("This is a proper step")
        );
        assert_eq!(
            extract_todo_items("Plan:\n1. `npm install`\n2. Run the build process").len(),
            1
        );
    }

    #[test]
    fn extracts_done_steps_like_example() {
        assert_eq!(extract_done_steps("I've completed [DONE:1]"), vec![1]);
        assert_eq!(
            extract_done_steps("Did [DONE:1] and [DONE:2] and [DONE:3]"),
            vec![1, 2, 3]
        );
        assert_eq!(
            extract_done_steps("[done:1] [DONE:2] [Done:3]"),
            vec![1, 2, 3]
        );
        assert!(extract_done_steps("No markers here").is_empty());
        assert_eq!(extract_done_steps("[DONE:abc] [DONE:] [DONE:1]"), vec![1]);
    }

    #[test]
    fn marks_completed_steps_like_example() {
        let mut items = vec![
            TodoItem {
                step: 1,
                text: "First".to_string(),
                completed: false,
            },
            TodoItem {
                step: 2,
                text: "Second".to_string(),
                completed: false,
            },
            TodoItem {
                step: 3,
                text: "Third".to_string(),
                completed: false,
            },
        ];
        assert_eq!(mark_completed_steps("[DONE:1] [DONE:3]", &mut items), 2);
        assert!(items[0].completed);
        assert!(!items[1].completed);
        assert!(items[2].completed);

        let mut missing = vec![TodoItem {
            step: 1,
            text: "First".to_string(),
            completed: false,
        }];
        assert_eq!(mark_completed_steps("[DONE:99]", &mut missing), 1);
        assert!(!missing[0].completed);
    }
}
