use super::{BackgroundFn, Component};
use crate::{apply_background_to_line, visible_width};

#[derive(Clone)]
struct RenderCache {
    child_lines: Vec<String>,
    width: usize,
    bg_sample: Option<String>,
    lines: Vec<String>,
}

pub struct BoxComponent {
    children: Vec<Box<dyn Component>>,
    padding_x: usize,
    padding_y: usize,
    bg_fn: Option<BackgroundFn>,
    cache: Option<RenderCache>,
}

impl BoxComponent {
    pub fn new(padding_x: usize, padding_y: usize) -> Self {
        Self {
            children: Vec::new(),
            padding_x,
            padding_y,
            bg_fn: None,
            cache: None,
        }
    }

    pub fn with_background(mut self, bg_fn: BackgroundFn) -> Self {
        self.bg_fn = Some(bg_fn);
        self
    }

    pub fn add_child(&mut self, component: Box<dyn Component>) {
        self.children.push(component);
        self.invalidate_cache();
    }

    pub fn remove_child_at(&mut self, index: usize) -> Option<Box<dyn Component>> {
        if index >= self.children.len() {
            return None;
        }
        self.invalidate_cache();
        Some(self.children.remove(index))
    }

    pub fn clear(&mut self) {
        self.children.clear();
        self.invalidate_cache();
    }

    pub fn set_bg_fn(&mut self, bg_fn: Option<BackgroundFn>) {
        self.bg_fn = bg_fn;
    }

    fn invalidate_cache(&mut self) {
        self.cache = None;
    }

    fn matches_cache(
        &self,
        width: usize,
        child_lines: &[String],
        bg_sample: &Option<String>,
    ) -> bool {
        self.cache.as_ref().is_some_and(|cache| {
            cache.width == width
                && cache.bg_sample == *bg_sample
                && cache.child_lines == child_lines
        })
    }

    fn apply_bg(&self, line: &str, width: usize) -> String {
        let padding_needed = width.saturating_sub(visible_width(line));
        let padded = format!("{line}{}", " ".repeat(padding_needed));
        if let Some(bg_fn) = &self.bg_fn {
            apply_background_to_line(&padded, width, |text| bg_fn(text))
        } else {
            padded
        }
    }
}

impl Default for BoxComponent {
    fn default() -> Self {
        Self::new(1, 1)
    }
}

impl Component for BoxComponent {
    fn render(&mut self, width: usize) -> Vec<String> {
        if self.children.is_empty() {
            return Vec::new();
        }

        let content_width = width.saturating_sub(self.padding_x * 2).max(1);
        let left_pad = " ".repeat(self.padding_x);
        let mut child_lines = Vec::new();
        for child in &mut self.children {
            for line in child.render(content_width) {
                child_lines.push(format!("{left_pad}{line}"));
            }
        }

        if child_lines.is_empty() {
            return Vec::new();
        }

        let bg_sample = self.bg_fn.as_ref().map(|bg_fn| bg_fn("test"));
        if self.matches_cache(width, &child_lines, &bg_sample) {
            return self
                .cache
                .as_ref()
                .map(|cache| cache.lines.clone())
                .unwrap_or_default();
        }

        let mut result = Vec::new();
        for _ in 0..self.padding_y {
            result.push(self.apply_bg("", width));
        }
        for line in &child_lines {
            result.push(self.apply_bg(line, width));
        }
        for _ in 0..self.padding_y {
            result.push(self.apply_bg("", width));
        }

        self.cache = Some(RenderCache {
            child_lines,
            width,
            bg_sample,
            lines: result.clone(),
        });
        result
    }

    fn invalidate(&mut self) {
        self.invalidate_cache();
        for child in &mut self.children {
            child.invalidate();
        }
    }
}
