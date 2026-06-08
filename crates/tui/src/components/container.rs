use super::Component;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::widgets::Widget;

pub struct Container {
    children: Vec<Box<dyn Component>>,
}

impl Container {
    pub fn new() -> Self {
        Self {
            children: Vec::new(),
        }
    }

    pub fn add_child<C>(&mut self, component: C)
    where
        C: Component + 'static,
    {
        self.children.push(Box::new(component));
    }

    pub fn add_boxed_child(&mut self, component: Box<dyn Component>) {
        self.children.push(component);
    }

    pub fn remove_child_at(&mut self, index: usize) -> Option<Box<dyn Component>> {
        if index >= self.children.len() {
            return None;
        }
        Some(self.children.remove(index))
    }

    pub fn clear(&mut self) {
        self.children.clear();
    }

    pub fn len(&self) -> usize {
        self.children.len()
    }

    pub fn is_empty(&self) -> bool {
        self.children.is_empty()
    }
}

impl Default for Container {
    fn default() -> Self {
        Self::new()
    }
}

impl Component for Container {
    fn render(&mut self, width: usize) -> Vec<String> {
        let mut lines = Vec::new();
        for child in &mut self.children {
            lines.extend(child.render(width));
        }
        lines
    }

    fn invalidate(&mut self) {
        for child in &mut self.children {
            child.invalidate();
        }
    }
}

impl Widget for &mut Container {
    fn render(self, area: Rect, buffer: &mut Buffer) {
        crate::ratatui_bridge::render_component_to_buffer(self, area, buffer);
    }
}
