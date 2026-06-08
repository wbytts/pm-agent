use crate::session_manager::SessionInfo;
use crate::session_selector_search::{
    filter_and_sort_sessions, has_session_name, SessionNameFilter, SessionSortMode,
};
use crate::utils::canonicalize_path;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionTreeNode {
    pub session: SessionInfo,
    pub children: Vec<SessionTreeNode>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlatSessionNode {
    pub session: SessionInfo,
    pub depth: usize,
    pub is_last: bool,
    pub ancestor_continues: Vec<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionSelectorState {
    all_sessions: Vec<SessionInfo>,
    filtered_sessions: Vec<FlatSessionNode>,
    selected_index: usize,
    sort_mode: SessionSortMode,
    name_filter: SessionNameFilter,
    query: String,
}

impl SessionSelectorState {
    pub fn new(
        sessions: Vec<SessionInfo>,
        sort_mode: SessionSortMode,
        name_filter: SessionNameFilter,
    ) -> Self {
        let mut state = Self {
            all_sessions: sessions,
            filtered_sessions: Vec::new(),
            selected_index: 0,
            sort_mode,
            name_filter,
            query: String::new(),
        };
        state.refilter();
        state
    }

    pub fn filtered_sessions(&self) -> &[FlatSessionNode] {
        &self.filtered_sessions
    }

    pub fn selected_index(&self) -> usize {
        self.selected_index
    }

    pub fn selected_session(&self) -> Option<&SessionInfo> {
        self.filtered_sessions
            .get(self.selected_index)
            .map(|node| &node.session)
    }

    pub fn selected_session_path(&self) -> Option<&str> {
        self.selected_session().map(|session| session.path.as_str())
    }

    pub fn sort_mode(&self) -> SessionSortMode {
        self.sort_mode
    }

    pub fn name_filter(&self) -> SessionNameFilter {
        self.name_filter
    }

    pub fn query(&self) -> &str {
        &self.query
    }

    pub fn set_sessions(&mut self, sessions: Vec<SessionInfo>) {
        self.all_sessions = sessions;
        self.refilter();
    }

    pub fn set_sort_mode(&mut self, sort_mode: SessionSortMode) {
        self.sort_mode = sort_mode;
        self.refilter();
    }

    pub fn set_name_filter(&mut self, name_filter: SessionNameFilter) {
        self.name_filter = name_filter;
        self.refilter();
    }

    pub fn set_query(&mut self, query: impl Into<String>) {
        self.query = query.into();
        self.refilter();
    }

    pub fn move_up(&mut self) {
        self.selected_index = self.selected_index.saturating_sub(1);
    }

    pub fn move_down(&mut self) {
        if self.filtered_sessions.is_empty() {
            self.selected_index = 0;
            return;
        }
        self.selected_index = (self.selected_index + 1).min(self.filtered_sessions.len() - 1);
    }

    pub fn page_up(&mut self, page_size: usize) {
        self.selected_index = self.selected_index.saturating_sub(page_size);
    }

    pub fn page_down(&mut self, page_size: usize) {
        if self.filtered_sessions.is_empty() {
            self.selected_index = 0;
            return;
        }
        self.selected_index =
            (self.selected_index + page_size).min(self.filtered_sessions.len() - 1);
    }

    fn refilter(&mut self) {
        let name_filtered = self
            .all_sessions
            .iter()
            .filter(|session| match self.name_filter {
                SessionNameFilter::All => true,
                SessionNameFilter::Named => has_session_name(session),
            })
            .cloned()
            .collect::<Vec<_>>();

        if self.sort_mode == SessionSortMode::Threaded && self.query.trim().is_empty() {
            self.filtered_sessions = flatten_session_tree(&build_session_tree(&name_filtered));
        } else {
            let filtered = filter_and_sort_sessions(
                &name_filtered,
                &self.query,
                self.sort_mode,
                SessionNameFilter::All,
            );
            let last_index = filtered.len().saturating_sub(1);
            self.filtered_sessions = filtered
                .into_iter()
                .enumerate()
                .map(|(index, session)| FlatSessionNode {
                    session,
                    depth: 0,
                    is_last: index == last_index,
                    ancestor_continues: Vec::new(),
                })
                .collect();
        }

        self.clamp_selected_index();
    }

    fn clamp_selected_index(&mut self) {
        self.selected_index = self
            .selected_index
            .min(self.filtered_sessions.len().saturating_sub(1));
    }
}

pub fn build_session_tree(sessions: &[SessionInfo]) -> Vec<SessionTreeNode> {
    let canonical_paths = sessions
        .iter()
        .map(|session| canonical_session_path(&session.path))
        .collect::<Vec<_>>();
    let mut children_by_index = vec![Vec::<usize>::new(); sessions.len()];
    let mut roots = Vec::<usize>::new();

    for index in 0..sessions.len() {
        let parent_path = sessions[index]
            .parent_session_path
            .as_deref()
            .map(canonical_session_path);
        let parent_index = parent_path
            .as_ref()
            .and_then(|parent| canonical_paths.iter().position(|path| path == parent));

        if let Some(parent_index) = parent_index {
            if parent_index != index {
                children_by_index[parent_index].push(index);
                continue;
            }
        }
        roots.push(index);
    }

    let mut root_nodes = roots
        .into_iter()
        .map(|index| build_node(index, sessions, &children_by_index))
        .collect::<Vec<_>>();
    sort_session_nodes(&mut root_nodes);
    root_nodes
}

fn build_node(
    index: usize,
    sessions: &[SessionInfo],
    children_by_index: &[Vec<usize>],
) -> SessionTreeNode {
    SessionTreeNode {
        session: sessions[index].clone(),
        children: children_by_index[index]
            .iter()
            .map(|child_index| build_node(*child_index, sessions, children_by_index))
            .collect(),
    }
}

pub fn flatten_session_tree(roots: &[SessionTreeNode]) -> Vec<FlatSessionNode> {
    let mut flattened = Vec::new();
    for (index, root) in roots.iter().enumerate() {
        flatten_node(
            root,
            0,
            Vec::new(),
            index == roots.len().saturating_sub(1),
            &mut flattened,
        );
    }
    flattened
}

fn flatten_node(
    node: &SessionTreeNode,
    depth: usize,
    ancestor_continues: Vec<bool>,
    is_last: bool,
    flattened: &mut Vec<FlatSessionNode>,
) {
    flattened.push(FlatSessionNode {
        session: node.session.clone(),
        depth,
        is_last,
        ancestor_continues: ancestor_continues.clone(),
    });

    for (index, child) in node.children.iter().enumerate() {
        let child_is_last = index == node.children.len().saturating_sub(1);
        let continues = if depth > 0 { !is_last } else { false };
        let mut child_ancestors = ancestor_continues.clone();
        child_ancestors.push(continues);
        flatten_node(child, depth + 1, child_ancestors, child_is_last, flattened);
    }
}

fn sort_session_nodes(nodes: &mut [SessionTreeNode]) {
    nodes.sort_by(|left, right| {
        right
            .session
            .modified_millis
            .cmp(&left.session.modified_millis)
    });
    for node in nodes {
        sort_session_nodes(&mut node.children);
    }
}

fn canonical_session_path(path: &str) -> PathBuf {
    canonicalize_path(Path::new(path))
}

#[cfg(test)]
mod tests {
    use super::{build_session_tree, flatten_session_tree, SessionSelectorState};
    use crate::session_manager::SessionInfo;
    use crate::session_selector_search::{SessionNameFilter, SessionSortMode};

    #[test]
    fn session_tree_attaches_children_by_parent_path_and_sorts_each_level_by_modified_desc() {
        let sessions = vec![
            session("root-old", "/sessions/root-old.jsonl", None, 10),
            session(
                "child-new",
                "/sessions/child-new.jsonl",
                Some("/sessions/root-old.jsonl"),
                30,
            ),
            session(
                "child-old",
                "/sessions/child-old.jsonl",
                Some("/sessions/root-old.jsonl"),
                20,
            ),
            session("root-new", "/sessions/root-new.jsonl", None, 40),
        ];

        let roots = build_session_tree(&sessions);

        assert_eq!(
            roots
                .iter()
                .map(|node| node.session.id.as_str())
                .collect::<Vec<_>>(),
            vec!["root-new", "root-old"]
        );
        assert_eq!(
            roots[1]
                .children
                .iter()
                .map(|node| node.session.id.as_str())
                .collect::<Vec<_>>(),
            vec!["child-new", "child-old"]
        );
    }

    #[test]
    fn session_tree_treats_missing_parent_as_root() {
        let sessions = vec![session(
            "orphan",
            "/sessions/orphan.jsonl",
            Some("/sessions/missing.jsonl"),
            10,
        )];

        let roots = build_session_tree(&sessions);

        assert_eq!(roots.len(), 1);
        assert_eq!(roots[0].session.id, "orphan");
    }

    #[test]
    fn flatten_session_tree_preserves_depth_last_and_ancestor_continuation_flags() {
        let sessions = vec![
            session("root-a", "/sessions/root-a.jsonl", None, 50),
            session(
                "child-a",
                "/sessions/child-a.jsonl",
                Some("/sessions/root-a.jsonl"),
                40,
            ),
            session(
                "grandchild-a",
                "/sessions/grandchild-a.jsonl",
                Some("/sessions/child-a.jsonl"),
                30,
            ),
            session(
                "child-b",
                "/sessions/child-b.jsonl",
                Some("/sessions/root-a.jsonl"),
                20,
            ),
            session("root-b", "/sessions/root-b.jsonl", None, 10),
        ];

        let flat = flatten_session_tree(&build_session_tree(&sessions));

        assert_eq!(
            flat.iter()
                .map(|node| (
                    node.session.id.as_str(),
                    node.depth,
                    node.is_last,
                    node.ancestor_continues.clone()
                ))
                .collect::<Vec<_>>(),
            vec![
                ("root-a", 0, false, vec![]),
                ("child-a", 1, false, vec![false]),
                ("grandchild-a", 2, true, vec![false, true]),
                ("child-b", 1, true, vec![false]),
                ("root-b", 0, true, vec![]),
            ]
        );
    }

    #[test]
    fn selector_state_uses_threaded_tree_for_empty_threaded_query() {
        let state = SessionSelectorState::new(
            vec![
                session(
                    "child",
                    "/sessions/child.jsonl",
                    Some("/sessions/root.jsonl"),
                    20,
                ),
                session("root", "/sessions/root.jsonl", None, 10),
            ],
            SessionSortMode::Threaded,
            SessionNameFilter::All,
        );

        assert_eq!(
            state
                .filtered_sessions()
                .iter()
                .map(|node| (node.session.id.as_str(), node.depth))
                .collect::<Vec<_>>(),
            vec![("root", 0), ("child", 1)]
        );
    }

    #[test]
    fn selector_state_flattens_and_sorts_by_relevance_when_threaded_query_is_not_empty() {
        let mut state = SessionSelectorState::new(
            vec![
                session_with_text("older", None, "alpha same", 1),
                session_with_text("newer", None, "alpha same", 99),
                session_with_text("best", None, "alpha starts here", 5),
            ],
            SessionSortMode::Threaded,
            SessionNameFilter::All,
        );

        state.set_query("alpha");

        assert_eq!(
            state
                .filtered_sessions()
                .iter()
                .map(|node| (node.session.id.as_str(), node.depth, node.is_last))
                .collect::<Vec<_>>(),
            vec![("best", 0, false), ("newer", 0, false), ("older", 0, true)]
        );
    }

    #[test]
    fn selector_state_recent_query_preserves_input_order() {
        let mut state = SessionSelectorState::new(
            vec![
                session_with_text("first", None, "alpha", 1),
                session_with_text("second", None, "alpha", 99),
            ],
            SessionSortMode::Recent,
            SessionNameFilter::All,
        );

        state.set_query("alpha");

        assert_eq!(
            state
                .filtered_sessions()
                .iter()
                .map(|node| node.session.id.as_str())
                .collect::<Vec<_>>(),
            vec!["first", "second"]
        );
    }

    #[test]
    fn selector_state_named_filter_hides_unnamed_sessions() {
        let state = SessionSelectorState::new(
            vec![
                named_session("named", Some("Release"), 10),
                named_session("unnamed", None, 20),
                named_session("blank", Some("   "), 30),
            ],
            SessionSortMode::Threaded,
            SessionNameFilter::Named,
        );

        assert_eq!(
            state
                .filtered_sessions()
                .iter()
                .map(|node| node.session.id.as_str())
                .collect::<Vec<_>>(),
            vec!["named"]
        );
    }

    #[test]
    fn selector_state_clamps_selection_and_returns_selected_path() {
        let mut state = SessionSelectorState::new(
            vec![
                session_with_text("one", None, "alpha", 1),
                session_with_text("two", None, "beta", 2),
                session_with_text("three", None, "gamma", 3),
            ],
            SessionSortMode::Recent,
            SessionNameFilter::All,
        );

        state.move_down();
        state.move_down();
        state.move_down();
        assert_eq!(state.selected_index(), 2);
        assert_eq!(state.selected_session_path(), Some("/sessions/three.jsonl"));

        state.set_query("alpha");

        assert_eq!(state.selected_index(), 0);
        assert_eq!(state.selected_session_path(), Some("/sessions/one.jsonl"));

        state.move_up();
        assert_eq!(state.selected_index(), 0);
    }

    fn session(id: &str, path: &str, parent: Option<&str>, modified_millis: u128) -> SessionInfo {
        SessionInfo {
            path: path.to_string(),
            id: id.to_string(),
            cwd: "/repo".to_string(),
            name: None,
            parent_session_path: parent.map(str::to_string),
            created_millis: 0,
            modified_millis,
            message_count: 0,
            first_message: String::new(),
            all_messages_text: String::new(),
        }
    }

    fn session_with_text(
        id: &str,
        name: Option<&str>,
        all_messages_text: &str,
        modified_millis: u128,
    ) -> SessionInfo {
        let mut session = named_session(id, name, modified_millis);
        session.first_message = all_messages_text.to_string();
        session.all_messages_text = all_messages_text.to_string();
        session
    }

    fn named_session(id: &str, name: Option<&str>, modified_millis: u128) -> SessionInfo {
        let mut session = session(id, &format!("/sessions/{id}.jsonl"), None, modified_millis);
        session.name = name.map(str::to_string);
        session
    }
}
