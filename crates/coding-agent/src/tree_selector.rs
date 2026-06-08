use std::collections::{BTreeMap, HashSet};
use tui::KeybindingsManager;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TreeFilterMode {
    Default,
    NoTools,
    UserOnly,
    LabeledOnly,
    All,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TreeSelectorAction {
    None,
    Select(String),
    Cancel,
    EditLabel {
        entry_id: String,
        current_label: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TreeEntryKind {
    Message {
        role: String,
        content: String,
        stop_reason: Option<String>,
        has_text: bool,
        tool_call_id: Option<String>,
        tool_calls: Vec<TreeToolCall>,
    },
    CustomMessage {
        custom_type: String,
        content: String,
    },
    Compaction {
        tokens_before: u64,
    },
    BranchSummary {
        summary: String,
    },
    ModelChange {
        model_id: String,
    },
    ThinkingLevelChange {
        thinking_level: String,
    },
    Custom {
        custom_type: String,
    },
    Label,
    LabelContent {
        label: Option<String>,
    },
    SessionInfo {
        name: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeEntry {
    pub id: String,
    pub parent_id: Option<String>,
    pub kind: TreeEntryKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeToolCall {
    pub id: String,
    pub name: String,
    pub arguments: BTreeMap<String, String>,
}

impl TreeEntry {
    pub fn new(
        id: impl Into<String>,
        parent_id: Option<&str>,
        kind: TreeEntryKind,
        _content: &str,
    ) -> Self {
        Self {
            id: id.into(),
            parent_id: parent_id.map(str::to_string),
            kind,
        }
    }

    pub fn message(id: &str, parent_id: Option<&str>, role: &str, content: &str) -> Self {
        Self {
            id: id.to_string(),
            parent_id: parent_id.map(str::to_string),
            kind: TreeEntryKind::Message {
                role: role.to_string(),
                content: content.to_string(),
                stop_reason: None,
                has_text: !content.trim().is_empty(),
                tool_call_id: None,
                tool_calls: Vec::new(),
            },
        }
    }

    pub fn assistant(
        id: &str,
        parent_id: Option<&str>,
        content: &str,
        stop_reason: Option<&str>,
        has_text: bool,
    ) -> Self {
        Self {
            id: id.to_string(),
            parent_id: parent_id.map(str::to_string),
            kind: TreeEntryKind::Message {
                role: "assistant".to_string(),
                content: content.to_string(),
                stop_reason: stop_reason.map(str::to_string),
                has_text,
                tool_call_id: None,
                tool_calls: Vec::new(),
            },
        }
    }

    pub fn assistant_with_tool_call(
        id: &str,
        parent_id: Option<&str>,
        tool_call: TreeToolCall,
    ) -> Self {
        Self {
            id: id.to_string(),
            parent_id: parent_id.map(str::to_string),
            kind: TreeEntryKind::Message {
                role: "assistant".to_string(),
                content: String::new(),
                stop_reason: Some("toolUse".to_string()),
                has_text: false,
                tool_call_id: None,
                tool_calls: vec![tool_call],
            },
        }
    }

    pub fn tool_result(
        id: &str,
        parent_id: Option<&str>,
        tool_call_id: &str,
        tool_name: &str,
    ) -> Self {
        Self {
            id: id.to_string(),
            parent_id: parent_id.map(str::to_string),
            kind: TreeEntryKind::Message {
                role: "toolResult".to_string(),
                content: tool_name.to_string(),
                stop_reason: None,
                has_text: true,
                tool_call_id: Some(tool_call_id.to_string()),
                tool_calls: Vec::new(),
            },
        }
    }
}

impl From<crate::session_manager::SessionTreeNode> for TreeSessionNode {
    fn from(value: crate::session_manager::SessionTreeNode) -> Self {
        let entry = TreeEntry::from(value.entry);
        Self {
            entry,
            label: value.label,
            label_timestamp: value.label_timestamp,
            children: value
                .children
                .into_iter()
                .map(TreeSessionNode::from)
                .collect(),
        }
    }
}

impl From<agent::harness::SessionTreeEntry> for TreeEntry {
    fn from(value: agent::harness::SessionTreeEntry) -> Self {
        use agent::harness::SessionTreeEntry;
        match value {
            SessionTreeEntry::Message {
                id,
                parent_id,
                message,
                ..
            } => Self {
                id,
                parent_id,
                kind: TreeEntryKind::Message {
                    role: message_role_name(&message.role).to_string(),
                    has_text: !message.content.trim().is_empty(),
                    content: message.content,
                    stop_reason: None,
                    tool_call_id: None,
                    tool_calls: Vec::new(),
                },
            },
            SessionTreeEntry::ThinkingLevelChange {
                id,
                parent_id,
                thinking_level,
                ..
            } => Self {
                id,
                parent_id,
                kind: TreeEntryKind::ThinkingLevelChange {
                    thinking_level: format!("{thinking_level:?}"),
                },
            },
            SessionTreeEntry::ModelChange {
                id,
                parent_id,
                model_id,
                ..
            } => Self {
                id,
                parent_id,
                kind: TreeEntryKind::ModelChange { model_id },
            },
            SessionTreeEntry::Compaction {
                id,
                parent_id,
                tokens_before,
                ..
            } => Self {
                id,
                parent_id,
                kind: TreeEntryKind::Compaction { tokens_before },
            },
            SessionTreeEntry::Custom {
                id,
                parent_id,
                custom_type,
                ..
            } => Self {
                id,
                parent_id,
                kind: TreeEntryKind::Custom { custom_type },
            },
            SessionTreeEntry::CustomMessage {
                id,
                parent_id,
                custom_type,
                content,
                ..
            } => Self {
                id,
                parent_id,
                kind: TreeEntryKind::CustomMessage {
                    custom_type,
                    content,
                },
            },
            SessionTreeEntry::Label {
                id,
                parent_id,
                label,
                ..
            } => Self {
                id,
                parent_id,
                kind: TreeEntryKind::Label,
            }
            .with_label_content(label),
            SessionTreeEntry::SessionInfo {
                id,
                parent_id,
                name,
                ..
            } => Self {
                id,
                parent_id,
                kind: TreeEntryKind::SessionInfo { name: Some(name) },
            },
            SessionTreeEntry::BranchSummary {
                id,
                parent_id,
                summary,
                ..
            } => Self {
                id,
                parent_id,
                kind: TreeEntryKind::BranchSummary { summary },
            },
            SessionTreeEntry::Leaf {
                id,
                parent_id,
                target_id,
                ..
            } => Self {
                id,
                parent_id,
                kind: TreeEntryKind::Custom {
                    custom_type: format!("leaf:{}", target_id.unwrap_or_default()),
                },
            },
        }
    }
}

impl TreeEntry {
    fn with_label_content(mut self, label: Option<String>) -> Self {
        self.kind = TreeEntryKind::LabelContent { label };
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeSessionNode {
    pub entry: TreeEntry,
    pub label: Option<String>,
    pub label_timestamp: Option<String>,
    pub children: Vec<TreeSessionNode>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlatTreeNode {
    node: TreeSessionNode,
    indent: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeSelectorState {
    flat_nodes: Vec<FlatTreeNode>,
    filtered_nodes: Vec<FlatTreeNode>,
    selected_index: usize,
    current_leaf_id: Option<String>,
    max_visible_lines: usize,
    filter_mode: TreeFilterMode,
    search_query: String,
    show_label_timestamps: bool,
    folded_nodes: HashSet<String>,
    active_path_ids: HashSet<String>,
    last_selected_id: Option<String>,
    tool_call_map: BTreeMap<String, TreeToolCall>,
}

impl TreeSelectorState {
    pub fn new(
        tree: Vec<TreeSessionNode>,
        current_leaf_id: Option<String>,
        max_visible_lines: usize,
        initial_selected_id: Option<String>,
        initial_filter_mode: Option<TreeFilterMode>,
    ) -> Self {
        let mut state = Self {
            flat_nodes: Vec::new(),
            filtered_nodes: Vec::new(),
            selected_index: 0,
            current_leaf_id,
            max_visible_lines,
            filter_mode: initial_filter_mode.unwrap_or(TreeFilterMode::Default),
            search_query: String::new(),
            show_label_timestamps: false,
            folded_nodes: HashSet::new(),
            active_path_ids: HashSet::new(),
            last_selected_id: None,
            tool_call_map: BTreeMap::new(),
        };
        state.flat_nodes = state.flatten_tree(&tree);
        state.build_tool_call_map();
        state.build_active_path();
        state.apply_filter();

        let target_id = initial_selected_id
            .as_deref()
            .or(state.current_leaf_id.as_deref());
        state.selected_index = state.find_nearest_visible_index(target_id);
        state.last_selected_id = state.selected_id().map(str::to_string);
        state
    }

    pub fn from_session_tree_nodes(
        tree: Vec<crate::session_manager::SessionTreeNode>,
        current_leaf_id: Option<String>,
        max_visible_lines: usize,
        initial_selected_id: Option<String>,
        initial_filter_mode: Option<TreeFilterMode>,
    ) -> Self {
        Self::new(
            tree.into_iter().map(TreeSessionNode::from).collect(),
            current_leaf_id,
            max_visible_lines,
            initial_selected_id,
            initial_filter_mode,
        )
    }

    pub fn selected_id(&self) -> Option<&str> {
        self.filtered_nodes
            .get(self.selected_index)
            .map(|flat| flat.node.entry.id.as_str())
    }

    pub fn visible_ids(&self) -> Vec<String> {
        self.filtered_nodes
            .iter()
            .map(|flat| flat.node.entry.id.clone())
            .collect()
    }

    pub fn search_query(&self) -> &str {
        &self.search_query
    }

    pub fn filter_mode(&self) -> TreeFilterMode {
        self.filter_mode
    }

    pub fn set_filter_mode(&mut self, filter_mode: TreeFilterMode) {
        self.filter_mode = filter_mode;
        self.folded_nodes.clear();
        self.apply_filter();
    }

    pub fn update_node_label(
        &mut self,
        entry_id: &str,
        label: Option<String>,
        label_timestamp: Option<String>,
    ) {
        for flat in &mut self.flat_nodes {
            if flat.node.entry.id == entry_id {
                flat.node.label = label.clone();
                flat.node.label_timestamp = if label.is_some() {
                    label_timestamp.clone()
                } else {
                    None
                };
            }
        }
        self.apply_filter();
    }

    pub fn set_search_query(&mut self, query: String) {
        self.search_query = query;
        self.apply_filter();
    }

    pub fn is_foldable(&self, entry_id: &str) -> bool {
        self.flat_nodes
            .iter()
            .any(|flat| flat.node.entry.parent_id.as_deref() == Some(entry_id))
    }

    pub fn toggle_fold_selected(&mut self) {
        let Some(selected_id) = self.selected_id().map(str::to_string) else {
            return;
        };
        if !self.is_foldable(&selected_id) {
            return;
        }
        if !self.folded_nodes.insert(selected_id.clone()) {
            self.folded_nodes.remove(&selected_id);
        }
        self.last_selected_id = Some(selected_id);
        self.apply_filter();
    }

    pub fn render_rows(&self) -> Vec<String> {
        let start = self
            .selected_index
            .saturating_sub(self.max_visible_lines / 2)
            .min(
                self.filtered_nodes
                    .len()
                    .saturating_sub(self.max_visible_lines),
            );
        let end = (start + self.max_visible_lines).min(self.filtered_nodes.len());

        self.filtered_nodes[start..end]
            .iter()
            .enumerate()
            .map(|(visible_offset, flat)| {
                let index = start + visible_offset;
                let cursor = if index == self.selected_index {
                    "› "
                } else {
                    "  "
                };
                let fold_marker = if self.folded_nodes.contains(&flat.node.entry.id) {
                    "⊞ "
                } else {
                    ""
                };
                let active_marker = if self.active_path_ids.contains(&flat.node.entry.id) {
                    "• "
                } else {
                    ""
                };
                let label = flat
                    .node
                    .label
                    .as_ref()
                    .map(|label| format!("[{label}] "))
                    .unwrap_or_default();
                let label_timestamp = if self.show_label_timestamps && flat.node.label.is_some() {
                    flat.node
                        .label_timestamp
                        .as_deref()
                        .map(format_label_timestamp)
                        .map(|timestamp| format!("{timestamp} "))
                        .unwrap_or_default()
                } else {
                    String::new()
                };
                format!(
                    "{cursor}{fold_marker}{active_marker}{label}{label_timestamp}{}",
                    self.entry_display_text(&flat.node)
                )
            })
            .collect()
    }

    pub fn handle_input(
        &mut self,
        key_data: &str,
        keybindings: &KeybindingsManager,
    ) -> TreeSelectorAction {
        if keybindings.matches(key_data, "tui.select.up") {
            self.move_up();
            TreeSelectorAction::None
        } else if keybindings.matches(key_data, "tui.select.down") {
            self.move_down();
            TreeSelectorAction::None
        } else if keybindings.matches(key_data, "app.tree.foldOrUp") {
            if self
                .selected_id()
                .is_some_and(|id| self.is_foldable(id) && !self.folded_nodes.contains(id))
            {
                self.toggle_fold_selected();
            } else {
                self.selected_index = self.find_branch_segment_start("up");
                self.last_selected_id = self.selected_id().map(str::to_string);
            }
            TreeSelectorAction::None
        } else if keybindings.matches(key_data, "app.tree.unfoldOrDown") {
            if let Some(selected_id) = self.selected_id().map(str::to_string) {
                if self.folded_nodes.contains(&selected_id) {
                    self.folded_nodes.remove(&selected_id);
                    self.last_selected_id = Some(selected_id);
                    self.apply_filter();
                } else {
                    self.selected_index = self.find_branch_segment_start("down");
                    self.last_selected_id = self.selected_id().map(str::to_string);
                }
            }
            TreeSelectorAction::None
        } else if keybindings.matches(key_data, "tui.editor.cursorLeft")
            || keybindings.matches(key_data, "tui.select.pageUp")
        {
            self.selected_index = self.selected_index.saturating_sub(self.max_visible_lines);
            self.last_selected_id = self.selected_id().map(str::to_string);
            TreeSelectorAction::None
        } else if keybindings.matches(key_data, "tui.editor.cursorRight")
            || keybindings.matches(key_data, "tui.select.pageDown")
        {
            self.selected_index = if self.filtered_nodes.is_empty() {
                0
            } else {
                (self.selected_index + self.max_visible_lines).min(self.filtered_nodes.len() - 1)
            };
            self.last_selected_id = self.selected_id().map(str::to_string);
            TreeSelectorAction::None
        } else if keybindings.matches(key_data, "tui.select.confirm") {
            self.selected_id()
                .map(|id| TreeSelectorAction::Select(id.to_string()))
                .unwrap_or(TreeSelectorAction::None)
        } else if keybindings.matches(key_data, "app.tree.editLabel") {
            self.filtered_nodes
                .get(self.selected_index)
                .map(|flat| TreeSelectorAction::EditLabel {
                    entry_id: flat.node.entry.id.clone(),
                    current_label: flat.node.label.clone(),
                })
                .unwrap_or(TreeSelectorAction::None)
        } else if keybindings.matches(key_data, "tui.select.cancel") {
            if self.search_query.is_empty() {
                TreeSelectorAction::Cancel
            } else {
                self.search_query.clear();
                self.folded_nodes.clear();
                self.apply_filter();
                TreeSelectorAction::None
            }
        } else if keybindings.matches(key_data, "app.tree.filter.default") {
            self.set_filter_mode(TreeFilterMode::Default);
            TreeSelectorAction::None
        } else if keybindings.matches(key_data, "app.tree.filter.noTools") {
            self.toggle_filter_mode(TreeFilterMode::NoTools);
            TreeSelectorAction::None
        } else if keybindings.matches(key_data, "app.tree.filter.userOnly") {
            self.toggle_filter_mode(TreeFilterMode::UserOnly);
            TreeSelectorAction::None
        } else if keybindings.matches(key_data, "app.tree.filter.labeledOnly") {
            self.toggle_filter_mode(TreeFilterMode::LabeledOnly);
            TreeSelectorAction::None
        } else if keybindings.matches(key_data, "app.tree.filter.all") {
            self.toggle_filter_mode(TreeFilterMode::All);
            TreeSelectorAction::None
        } else if keybindings.matches(key_data, "app.tree.filter.cycleForward") {
            self.cycle_filter(1);
            TreeSelectorAction::None
        } else if keybindings.matches(key_data, "app.tree.filter.cycleBackward") {
            self.cycle_filter(-1);
            TreeSelectorAction::None
        } else if keybindings.matches(key_data, "app.tree.toggleLabelTimestamp") {
            self.show_label_timestamps = !self.show_label_timestamps;
            TreeSelectorAction::None
        } else {
            TreeSelectorAction::None
        }
    }

    fn flatten_tree(&self, roots: &[TreeSessionNode]) -> Vec<FlatTreeNode> {
        let contains_active = roots
            .iter()
            .map(|node| (node.entry.id.clone(), self.subtree_contains_active(node)))
            .collect::<std::collections::HashMap<_, _>>();
        let mut roots = roots.to_vec();
        roots.sort_by_key(|node| {
            !contains_active
                .get(&node.entry.id)
                .copied()
                .unwrap_or(false)
        });

        let mut result = Vec::new();
        for root in roots {
            self.flatten_node(root, 0, &mut result);
        }
        result
    }

    fn toggle_filter_mode(&mut self, filter_mode: TreeFilterMode) {
        if self.filter_mode == filter_mode {
            self.set_filter_mode(TreeFilterMode::Default);
        } else {
            self.set_filter_mode(filter_mode);
        }
    }

    fn cycle_filter(&mut self, direction: isize) {
        let modes = [
            TreeFilterMode::Default,
            TreeFilterMode::NoTools,
            TreeFilterMode::UserOnly,
            TreeFilterMode::LabeledOnly,
            TreeFilterMode::All,
        ];
        let current_index = modes
            .iter()
            .position(|mode| *mode == self.filter_mode)
            .unwrap_or(0) as isize;
        let next_index = (current_index + direction).rem_euclid(modes.len() as isize) as usize;
        self.set_filter_mode(modes[next_index]);
    }

    fn find_branch_segment_start(&self, direction: &str) -> usize {
        if self.filtered_nodes.is_empty() {
            return 0;
        }
        let current_depth = self.filtered_nodes[self.selected_index].indent;
        match direction {
            "up" => {
                let mut index = self.selected_index;
                while index > 0 {
                    index -= 1;
                    if self.filtered_nodes[index].indent <= current_depth {
                        return index;
                    }
                }
                0
            }
            "down" => {
                let mut index = self.selected_index + 1;
                while index < self.filtered_nodes.len() {
                    if self.filtered_nodes[index].indent <= current_depth {
                        return index;
                    }
                    index += 1;
                }
                self.filtered_nodes.len() - 1
            }
            _ => self.selected_index,
        }
    }

    fn flatten_node(&self, node: TreeSessionNode, indent: usize, result: &mut Vec<FlatTreeNode>) {
        let mut children = node.children.clone();
        children.sort_by_key(|child| !self.subtree_contains_active(child));
        let child_indent = if children.len() > 1 {
            indent + 1
        } else {
            indent
        };
        result.push(FlatTreeNode { node, indent });
        for child in children {
            self.flatten_node(child, child_indent, result);
        }
    }

    fn subtree_contains_active(&self, node: &TreeSessionNode) -> bool {
        self.current_leaf_id.as_deref() == Some(node.entry.id.as_str())
            || node
                .children
                .iter()
                .any(|child| self.subtree_contains_active(child))
    }

    fn build_active_path(&mut self) {
        let Some(mut current_id) = self.current_leaf_id.clone() else {
            return;
        };
        while let Some(flat) = self
            .flat_nodes
            .iter()
            .find(|flat| flat.node.entry.id == current_id)
        {
            self.active_path_ids.insert(current_id.clone());
            let Some(parent_id) = &flat.node.entry.parent_id else {
                break;
            };
            current_id = parent_id.clone();
        }
    }

    fn build_tool_call_map(&mut self) {
        self.tool_call_map.clear();
        for flat in &self.flat_nodes {
            if let TreeEntryKind::Message { tool_calls, .. } = &flat.node.entry.kind {
                for tool_call in tool_calls {
                    self.tool_call_map
                        .insert(tool_call.id.clone(), tool_call.clone());
                }
            }
        }
    }

    fn apply_filter(&mut self) {
        if !self.filtered_nodes.is_empty() {
            self.last_selected_id = self
                .selected_id()
                .map(str::to_string)
                .or(self.last_selected_id.clone());
        }
        let search_tokens = self
            .search_query
            .to_lowercase()
            .split_whitespace()
            .map(str::to_string)
            .collect::<Vec<_>>();
        let mut skip_set = HashSet::new();
        for flat in &self.flat_nodes {
            if flat.node.entry.parent_id.as_ref().is_some_and(|parent_id| {
                self.folded_nodes.contains(parent_id) || skip_set.contains(parent_id)
            }) {
                skip_set.insert(flat.node.entry.id.clone());
            }
        }

        self.filtered_nodes = self
            .flat_nodes
            .iter()
            .filter(|flat| !skip_set.contains(&flat.node.entry.id))
            .filter(|flat| self.passes_filter(flat))
            .filter(|flat| {
                if search_tokens.is_empty() {
                    return true;
                }
                let text = self.searchable_text(&flat.node).to_lowercase();
                search_tokens.iter().all(|token| text.contains(token))
            })
            .cloned()
            .collect();

        self.selected_index = self.find_nearest_visible_index(self.last_selected_id.as_deref());
        self.last_selected_id = self
            .selected_id()
            .map(str::to_string)
            .or(self.last_selected_id.clone());
    }

    fn passes_filter(&self, flat: &FlatTreeNode) -> bool {
        let entry = &flat.node.entry;
        let is_current_leaf = Some(entry.id.as_str()) == self.current_leaf_id.as_deref();
        let hide_tool_only_assistant = if let TreeEntryKind::Message {
            role,
            stop_reason,
            has_text,
            ..
        } = &entry.kind
        {
            role == "assistant"
                && !is_current_leaf
                && !has_text
                && !stop_reason
                    .as_deref()
                    .is_some_and(|reason| reason != "stop" && reason != "toolUse")
        } else {
            false
        };
        if hide_tool_only_assistant {
            return false;
        }

        let is_settings_entry = matches!(
            entry.kind,
            TreeEntryKind::Label
                | TreeEntryKind::Custom { .. }
                | TreeEntryKind::ModelChange { .. }
                | TreeEntryKind::ThinkingLevelChange { .. }
                | TreeEntryKind::SessionInfo { .. }
        );
        match self.filter_mode {
            TreeFilterMode::UserOnly => matches!(
                &entry.kind,
                TreeEntryKind::Message { role, .. } if role == "user"
            ),
            TreeFilterMode::NoTools => {
                !is_settings_entry
                    && !matches!(
                        &entry.kind,
                        TreeEntryKind::Message { role, .. } if role == "toolResult"
                    )
            }
            TreeFilterMode::LabeledOnly => flat.node.label.is_some(),
            TreeFilterMode::All => true,
            TreeFilterMode::Default => !is_settings_entry,
        }
    }

    fn find_nearest_visible_index(&self, entry_id: Option<&str>) -> usize {
        if self.filtered_nodes.is_empty() {
            return 0;
        }
        let mut current_id = entry_id.map(str::to_string);
        while let Some(id) = current_id {
            if let Some(index) = self
                .filtered_nodes
                .iter()
                .position(|flat| flat.node.entry.id == id)
            {
                return index;
            }
            current_id = self
                .flat_nodes
                .iter()
                .find(|flat| flat.node.entry.id == id)
                .and_then(|flat| flat.node.entry.parent_id.clone());
        }
        self.filtered_nodes.len() - 1
    }

    fn move_up(&mut self) {
        if self.filtered_nodes.is_empty() {
            self.selected_index = 0;
        } else if self.selected_index == 0 {
            self.selected_index = self.filtered_nodes.len() - 1;
        } else {
            self.selected_index -= 1;
        }
        self.last_selected_id = self.selected_id().map(str::to_string);
    }

    fn move_down(&mut self) {
        if self.filtered_nodes.is_empty() || self.selected_index + 1 >= self.filtered_nodes.len() {
            self.selected_index = 0;
        } else {
            self.selected_index += 1;
        }
        self.last_selected_id = self.selected_id().map(str::to_string);
    }

    fn entry_display_text(&self, node: &TreeSessionNode) -> String {
        match &node.entry.kind {
            TreeEntryKind::Message { role, content, .. } if role == "user" => {
                format!("user: {}", normalize(content))
            }
            TreeEntryKind::Message {
                role,
                tool_call_id,
                content,
                ..
            } if role == "toolResult" => {
                if let Some(tool_call) = tool_call_id
                    .as_ref()
                    .and_then(|id| self.tool_call_map.get(id))
                {
                    format_tool_call(&tool_call.name, &tool_call.arguments)
                } else {
                    format!("[{}]", if content.is_empty() { "tool" } else { content })
                }
            }
            TreeEntryKind::Message { role, content, .. } if role == "assistant" => {
                if content.trim().is_empty() {
                    "assistant: (no content)".to_string()
                } else {
                    format!("assistant: {}", normalize(content))
                }
            }
            TreeEntryKind::Message { role, content, .. } => {
                format!("[{role}]: {}", normalize(content))
            }
            TreeEntryKind::CustomMessage {
                custom_type,
                content,
            } => format!("[{custom_type}]: {}", normalize(content)),
            TreeEntryKind::Compaction { tokens_before } => {
                format!("[compaction: {}k tokens]", tokens_before / 1000)
            }
            TreeEntryKind::BranchSummary { summary } => {
                format!("[branch summary]: {}", normalize(summary))
            }
            TreeEntryKind::ModelChange { model_id } => format!("[model: {model_id}]"),
            TreeEntryKind::ThinkingLevelChange { thinking_level } => {
                format!("[thinking: {thinking_level}]")
            }
            TreeEntryKind::Custom { custom_type } => format!("[custom: {custom_type}]"),
            TreeEntryKind::Label => "[label]".to_string(),
            TreeEntryKind::LabelContent { label } => {
                format!("[label: {}]", label.as_deref().unwrap_or("(cleared)"))
            }
            TreeEntryKind::SessionInfo { name } => {
                format!("[title: {}]", name.as_deref().unwrap_or("empty"))
            }
        }
    }

    fn searchable_text(&self, node: &TreeSessionNode) -> String {
        let mut parts = Vec::new();
        if let Some(label) = &node.label {
            parts.push(label.clone());
        }
        parts.push(self.entry_display_text(node));
        if let TreeEntryKind::Message {
            stop_reason: Some(stop_reason),
            ..
        } = &node.entry.kind
        {
            parts.push(stop_reason.clone());
        }
        parts.join(" ")
    }
}

fn normalize(value: &str) -> String {
    value.replace(['\n', '\t'], " ").trim().to_string()
}

fn message_role_name(role: &ai::MessageRole) -> &'static str {
    match role {
        ai::MessageRole::System => "system",
        ai::MessageRole::User => "user",
        ai::MessageRole::Assistant => "assistant",
        ai::MessageRole::Tool => "toolResult",
    }
}

fn format_label_timestamp(timestamp: &str) -> String {
    let Some(time_part) = timestamp.split('T').nth(1) else {
        return timestamp.to_string();
    };
    time_part.chars().take(5).collect()
}

fn format_tool_call(name: &str, args: &BTreeMap<String, String>) -> String {
    let arg = |key: &str| args.get(key).map(String::as_str).unwrap_or("");
    let path_arg = || {
        arg("path")
            .is_empty()
            .then(|| arg("file_path"))
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| arg("path"))
    };

    match name {
        "read" => {
            let mut display = path_arg().to_string();
            let offset = args.get("offset");
            let limit = args.get("limit");
            if offset.is_some() || limit.is_some() {
                let start = offset.map(String::as_str).unwrap_or("1");
                if let (Ok(start_num), Some(limit)) = (start.parse::<usize>(), limit) {
                    if let Ok(limit_num) = limit.parse::<usize>() {
                        display.push_str(&format!(":{start_num}-{}", start_num + limit_num - 1));
                    } else {
                        display.push_str(&format!(":{start}"));
                    }
                } else {
                    display.push_str(&format!(":{start}"));
                }
            }
            format!("[read: {display}]")
        }
        "write" => format!("[write: {}]", path_arg()),
        "edit" => format!("[edit: {}]", path_arg()),
        "bash" => {
            let raw = arg("command");
            let cmd = normalize(raw).chars().take(50).collect::<String>();
            let suffix = if raw.chars().count() > 50 { "..." } else { "" };
            format!("[bash: {cmd}{suffix}]")
        }
        "grep" => format!("[grep: /{}/ in {}]", arg("pattern"), path_arg_or_dot(args)),
        "find" => format!("[find: {} in {}]", arg("pattern"), path_arg_or_dot(args)),
        "ls" => format!("[ls: {}]", path_arg_or_dot(args)),
        other => format!("[{other}: {}]", format_args_preview(args)),
    }
}

fn path_arg_or_dot(args: &BTreeMap<String, String>) -> &str {
    args.get("path").map(String::as_str).unwrap_or(".")
}

fn format_args_preview(args: &BTreeMap<String, String>) -> String {
    let body = args
        .iter()
        .map(|(key, value)| format!("\"{key}\":\"{value}\""))
        .collect::<Vec<_>>()
        .join(",");
    let json = format!("{{{body}}}");
    if json.chars().count() > 40 {
        format!("{}...", json.chars().take(40).collect::<String>())
    } else {
        json
    }
}

#[cfg(test)]
mod tests {
    use super::{
        TreeEntry, TreeEntryKind, TreeFilterMode, TreeSelectorAction, TreeSelectorState,
        TreeSessionNode, TreeToolCall,
    };
    use crate::keybindings::app_keybindings;
    use std::collections::BTreeMap;
    use tui::KeybindingsManager;

    #[test]
    fn tree_selector_flattens_active_branch_first_and_selects_current_leaf() {
        let state = TreeSelectorState::new(tree(), Some("a2".to_string()), 10, None, None);

        assert_eq!(state.visible_ids(), vec!["a", "a1", "a2", "b"]);
        assert_eq!(state.selected_id(), Some("a2"));
        assert_eq!(
            state.render_rows(),
            vec![
                "  • user: root a".to_string(),
                "  • assistant: answer".to_string(),
                "› • user: leaf".to_string(),
                "  user: root b".to_string(),
            ]
        );
    }

    #[test]
    fn tree_selector_default_filter_hides_settings_and_tool_only_assistant_messages() {
        let state =
            TreeSelectorState::new(filter_tree(), Some("tool-only".to_string()), 10, None, None);

        assert_eq!(
            state.visible_ids(),
            vec!["root", "tool-only", "assistant-error", "user"]
        );
        assert_eq!(state.selected_id(), Some("tool-only"));
    }

    #[test]
    fn tree_selector_modes_search_and_cancel_behave_like_pi() {
        let mut state =
            TreeSelectorState::new(filter_tree(), Some("user".to_string()), 10, None, None);
        let keybindings = keybindings();

        state.set_filter_mode(TreeFilterMode::UserOnly);
        assert_eq!(state.visible_ids(), vec!["root", "user"]);

        state.set_filter_mode(TreeFilterMode::All);
        assert!(state.visible_ids().contains(&"label".to_string()));

        state.set_search_query("err".to_string());
        assert_eq!(state.visible_ids(), vec!["user", "assistant-error"]);
        assert_eq!(
            state.handle_input("\x1b", &keybindings),
            TreeSelectorAction::None
        );
        assert_eq!(state.search_query(), "");
        assert_eq!(state.filter_mode(), TreeFilterMode::All);
    }

    #[test]
    fn tree_selector_wraps_navigation_and_confirms_or_cancels() {
        let mut state = TreeSelectorState::new(tree(), Some("a".to_string()), 10, None, None);
        let keybindings = keybindings();

        assert_eq!(
            state.handle_input("\x1b[A", &keybindings),
            TreeSelectorAction::None
        );
        assert_eq!(state.selected_id(), Some("b"));
        assert_eq!(
            state.handle_input("\x1b[B", &keybindings),
            TreeSelectorAction::None
        );
        assert_eq!(state.selected_id(), Some("a"));
        assert_eq!(
            state.handle_input("\r", &keybindings),
            TreeSelectorAction::Select("a".to_string())
        );
        assert_eq!(
            state.handle_input("\x1b", &keybindings),
            TreeSelectorAction::Cancel
        );
    }

    #[test]
    fn tree_selector_fold_hides_descendants_and_keeps_selection_on_folded_node() {
        let mut state = TreeSelectorState::new(tree(), Some("a".to_string()), 10, None, None);

        assert!(state.is_foldable("a"));
        state.toggle_fold_selected();

        assert_eq!(state.visible_ids(), vec!["a", "b"]);
        assert_eq!(state.selected_id(), Some("a"));
        assert_eq!(
            state.render_rows(),
            vec![
                "› ⊞ • user: root a".to_string(),
                "  user: root b".to_string()
            ]
        );
    }

    #[test]
    fn tree_selector_handles_filter_page_fold_and_timestamp_keybindings() {
        let mut state =
            TreeSelectorState::new(filter_tree(), Some("root".to_string()), 2, None, None);
        let keybindings = keybindings();

        assert_eq!(
            state.handle_input("u", &keybindings),
            TreeSelectorAction::None
        );
        assert_eq!(state.filter_mode(), TreeFilterMode::UserOnly);
        assert_eq!(state.visible_ids(), vec!["root", "user"]);

        assert_eq!(
            state.handle_input("\t", &keybindings),
            TreeSelectorAction::None
        );
        assert_eq!(state.filter_mode(), TreeFilterMode::LabeledOnly);

        assert_eq!(
            state.handle_input("\x1b[Z", &keybindings),
            TreeSelectorAction::None
        );
        assert_eq!(state.filter_mode(), TreeFilterMode::UserOnly);

        state.set_filter_mode(TreeFilterMode::All);
        state.handle_input("\x1b[C", &keybindings);
        assert_eq!(state.selected_id(), Some("assistant-error"));
        state.handle_input("\x1b[D", &keybindings);
        assert_eq!(state.selected_id(), Some("root"));

        state.handle_input("h", &keybindings);
        assert_eq!(state.visible_ids(), vec!["root"]);
        state.handle_input("l", &keybindings);
        assert!(state.visible_ids().contains(&"user".to_string()));

        state.update_node_label(
            "user",
            Some("important".to_string()),
            Some("2026-06-07T08:09:00Z".to_string()),
        );
        state.handle_input("t", &keybindings);
        state.set_filter_mode(TreeFilterMode::LabeledOnly);
        assert_eq!(
            state.render_rows(),
            vec!["› [important] 08:09 user: find error"]
        );
    }

    #[test]
    fn tree_selector_formats_tool_results_from_assistant_tool_calls_like_pi() {
        let mut args = BTreeMap::new();
        args.insert("path".to_string(), "/tmp/demo.txt".to_string());
        args.insert("offset".to_string(), "3".to_string());
        args.insert("limit".to_string(), "4".to_string());
        let state = TreeSelectorState::new(
            vec![node(
                "assistant",
                TreeEntry::assistant_with_tool_call(
                    "assistant",
                    None,
                    TreeToolCall {
                        id: "call-1".to_string(),
                        name: "read".to_string(),
                        arguments: args,
                    },
                ),
                vec![node(
                    "result",
                    TreeEntry::tool_result("result", Some("assistant"), "call-1", "read"),
                    vec![],
                )],
            )],
            Some("result".to_string()),
            10,
            None,
            Some(TreeFilterMode::All),
        );

        assert_eq!(
            state.render_rows(),
            vec!["› • [read: /tmp/demo.txt:3-6]".to_string()]
        );
    }

    #[test]
    fn tree_selector_returns_edit_label_action_with_current_label() {
        let mut state = TreeSelectorState::new(tree(), Some("a".to_string()), 10, None, None);
        let keybindings = keybindings();
        state.update_node_label("a", Some("todo".to_string()), None);

        assert_eq!(
            state.handle_input("e", &keybindings),
            TreeSelectorAction::EditLabel {
                entry_id: "a".to_string(),
                current_label: Some("todo".to_string()),
            }
        );
    }

    #[test]
    fn tree_selector_builds_from_real_session_tree_nodes() {
        use agent::harness::SessionTreeEntry;
        use agent::AgentMessage;
        use ai::MessageRole;

        let tree = vec![crate::session_manager::SessionTreeNode {
            entry: SessionTreeEntry::Message {
                id: "user".to_string(),
                parent_id: None,
                timestamp: "2026-06-07T08:00:00Z".to_string(),
                message: AgentMessage::new(MessageRole::User, "hello".to_string()),
            },
            label: Some("start".to_string()),
            label_timestamp: Some("2026-06-07T08:00:30Z".to_string()),
            children: vec![crate::session_manager::SessionTreeNode {
                entry: SessionTreeEntry::BranchSummary {
                    id: "branch".to_string(),
                    parent_id: Some("user".to_string()),
                    timestamp: "2026-06-07T08:01:00Z".to_string(),
                    from_id: "user".to_string(),
                    summary: "forked here".to_string(),
                    details: None,
                    from_hook: false,
                },
                label: None,
                label_timestamp: None,
                children: vec![],
            }],
        }];

        let state = TreeSelectorState::from_session_tree_nodes(
            tree,
            Some("branch".to_string()),
            10,
            None,
            None,
        );

        assert_eq!(state.visible_ids(), vec!["user", "branch"]);
        assert_eq!(
            state.render_rows(),
            vec![
                "  • [start] user: hello".to_string(),
                "› • [branch summary]: forked here".to_string(),
            ]
        );
    }

    fn tree() -> Vec<TreeSessionNode> {
        vec![
            node(
                "a",
                TreeEntry::message("a", None, "user", "root a"),
                vec![node(
                    "a1",
                    TreeEntry::message("a1", Some("a"), "assistant", "answer"),
                    vec![node(
                        "a2",
                        TreeEntry::message("a2", Some("a1"), "user", "leaf"),
                        vec![],
                    )],
                )],
            ),
            node("b", TreeEntry::message("b", None, "user", "root b"), vec![]),
        ]
    }

    fn filter_tree() -> Vec<TreeSessionNode> {
        vec![node(
            "root",
            TreeEntry::message("root", None, "user", "root"),
            vec![
                node(
                    "label",
                    TreeEntry::new("label", Some("root"), TreeEntryKind::Label, ""),
                    vec![],
                ),
                node(
                    "tool-only",
                    TreeEntry::assistant("tool-only", Some("root"), "", None, false),
                    vec![],
                ),
                node(
                    "assistant-error",
                    TreeEntry::assistant("assistant-error", Some("root"), "", Some("error"), false),
                    vec![],
                ),
                node(
                    "user",
                    TreeEntry::message("user", Some("root"), "user", "find error"),
                    vec![],
                ),
            ],
        )]
    }

    fn node(id: &str, entry: TreeEntry, children: Vec<TreeSessionNode>) -> TreeSessionNode {
        assert_eq!(entry.id, id);
        TreeSessionNode {
            entry,
            label: None,
            label_timestamp: None,
            children,
        }
    }

    fn keybindings() -> KeybindingsManager {
        let mut bindings = BTreeMap::new();
        bindings.insert(
            "app.tree.filter.userOnly".to_string(),
            vec!["u".to_string()],
        );
        bindings.insert(
            "app.tree.filter.cycleForward".to_string(),
            vec!["tab".to_string()],
        );
        bindings.insert(
            "app.tree.filter.cycleBackward".to_string(),
            vec!["shift+tab".to_string()],
        );
        bindings.insert(
            "tui.editor.cursorRight".to_string(),
            vec!["right".to_string()],
        );
        bindings.insert(
            "tui.editor.cursorLeft".to_string(),
            vec!["left".to_string()],
        );
        bindings.insert("app.tree.foldOrUp".to_string(), vec!["h".to_string()]);
        bindings.insert("app.tree.unfoldOrDown".to_string(), vec!["l".to_string()]);
        bindings.insert(
            "app.tree.toggleLabelTimestamp".to_string(),
            vec!["t".to_string()],
        );
        bindings.insert("app.tree.editLabel".to_string(), vec!["e".to_string()]);
        KeybindingsManager::new(app_keybindings(), bindings)
    }
}
