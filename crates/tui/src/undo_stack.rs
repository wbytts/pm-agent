pub struct UndoStack<S: Clone> {
    stack: Vec<S>,
}

impl<S: Clone> UndoStack<S> {
    pub fn new() -> Self {
        Self { stack: Vec::new() }
    }

    pub fn push(&mut self, state: &S) {
        self.stack.push(state.clone());
    }

    pub fn pop(&mut self) -> Option<S> {
        self.stack.pop()
    }

    pub fn clear(&mut self) {
        self.stack.clear();
    }

    pub fn len(&self) -> usize {
        self.stack.len()
    }

    pub fn is_empty(&self) -> bool {
        self.stack.is_empty()
    }
}

impl<S: Clone> Default for UndoStack<S> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn undo_stack_clones_snapshots() {
        let mut stack = UndoStack::new();
        let mut state = vec!["a".to_string()];
        stack.push(&state);
        state.push("b".to_string());
        assert_eq!(stack.pop(), Some(vec!["a".to_string()]));
    }
}
