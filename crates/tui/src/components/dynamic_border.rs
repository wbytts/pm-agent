use super::Component;
use std::sync::Arc;

pub type BorderStyleFn = Arc<dyn Fn(&str) -> String + Send + Sync>;

pub struct DynamicBorder {
    style: BorderStyleFn,
}

impl DynamicBorder {
    pub fn new(style: BorderStyleFn) -> Self {
        Self { style }
    }
}

impl Default for DynamicBorder {
    fn default() -> Self {
        Self::new(Arc::new(str::to_string))
    }
}

impl Component for DynamicBorder {
    fn render(&mut self, width: usize) -> Vec<String> {
        vec![(self.style)(&"─".repeat(width.max(1)))]
    }
}

#[cfg(test)]
mod tests {
    use super::DynamicBorder;
    use crate::components::Component;
    use crate::visible_width;
    use std::sync::Arc;

    #[test]
    fn dynamic_border_renders_at_least_one_horizontal_line() {
        let mut border = DynamicBorder::default();

        assert_eq!(border.render(0), vec!["─"]);
        assert_eq!(border.render(4), vec!["────"]);
        assert_eq!(visible_width(&border.render(4)[0]), 4);
    }

    #[test]
    fn dynamic_border_applies_custom_style_function() {
        let mut border = DynamicBorder::new(Arc::new(|line| format!("[{line}]")));

        assert_eq!(border.render(3), vec!["[───]"]);
    }
}
