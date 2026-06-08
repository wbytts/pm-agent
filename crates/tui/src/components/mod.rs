use std::sync::Arc;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

pub mod bordered_loader;
pub mod box_component;
pub mod cancellable_loader;
pub mod container;
pub mod dynamic_border;
pub mod editor;
pub mod image;
pub mod input;
pub mod loader;
pub mod markdown;
pub mod select_list;
pub mod settings_list;
pub mod spacer;
pub mod text;
pub mod truncated_text;

pub use bordered_loader::{BorderedLoader, BorderedLoaderOptions};
pub use box_component::BoxComponent;
pub use cancellable_loader::CancellableLoader;
pub use container::Container;
pub use dynamic_border::DynamicBorder;
pub use editor::{word_wrap_line, Editor, EditorOptions, EditorState, EditorTheme, TextChunk};
pub use image::{Image, ImageOptions, ImageTheme};
pub use input::{Input, CURSOR_MARKER};
pub use loader::{Loader, LoaderIndicatorOptions};
pub use markdown::{DefaultTextStyle, Markdown, MarkdownTheme};
pub use select_list::{
    SelectItem, SelectList, SelectListLayoutOptions, SelectListTheme,
    SelectListTruncatePrimaryContext,
};
pub use settings_list::{
    SettingItem, SettingsList, SettingsListOptions, SettingsListTheme, SettingsSubmenu,
    SettingsSubmenuDone,
};
pub use spacer::Spacer;
pub use text::Text;
pub use truncated_text::TruncatedText;

pub type BackgroundFn = Arc<dyn Fn(&str) -> String + Send + Sync>;

pub trait Component {
    fn render(&mut self, width: usize) -> Vec<String>;

    /// 将现有行渲染协议写入 ratatui buffer，供所有组件直接接入 ratatui 主循环。
    fn render_ratatui(&mut self, area: Rect, buffer: &mut Buffer) {
        crate::ratatui_bridge::render_component_to_buffer(self, area, buffer);
    }

    fn invalidate(&mut self) {}
}

impl<'a, 'b> ratatui::widgets::Widget for &'a mut (dyn Component + 'b) {
    fn render(self, area: Rect, buffer: &mut Buffer) {
        crate::ratatui_bridge::render_component_to_buffer(self, area, buffer);
    }
}

macro_rules! impl_widget_for_component_ref {
    ($($component:ty),+ $(,)?) => {
        $(
            impl ratatui::widgets::Widget for &mut $component {
                fn render(self, area: Rect, buffer: &mut Buffer) {
                    crate::ratatui_bridge::render_component_to_buffer(self, area, buffer);
                }
            }
        )+
    };
}

impl_widget_for_component_ref!(
    BorderedLoader,
    BoxComponent,
    CancellableLoader,
    DynamicBorder,
    Editor,
    Image,
    Input,
    Loader,
    Markdown,
    SettingsList,
    Spacer,
    Text,
    TruncatedText,
);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::visible_width;
    use crate::KeybindingsManager;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    struct InvalidationProbe {
        invalidated: bool,
    }

    impl InvalidationProbe {
        fn new() -> Self {
            Self { invalidated: false }
        }
    }

    impl Component for InvalidationProbe {
        fn render(&mut self, _width: usize) -> Vec<String> {
            if self.invalidated {
                vec!["invalidated".to_string()]
            } else {
                vec!["fresh".to_string()]
            }
        }

        fn invalidate(&mut self) {
            self.invalidated = true;
        }
    }

    #[test]
    fn spacer_renders_empty_lines() {
        let mut spacer = Spacer::new(3);
        assert_eq!(spacer.render(10), vec!["", "", ""]);
        spacer.set_lines(1);
        assert_eq!(spacer.render(10), vec![""]);
    }

    #[test]
    fn text_wraps_and_pads_with_margins() {
        let mut text = Text::new("hello world", 1, 1);
        let lines = text.render(8);
        assert_eq!(lines.len(), 4);
        assert!(lines.iter().all(|line| visible_width(line) == 8));
        assert_eq!(lines[1], " hello  ");
        assert_eq!(lines[2], " world  ");
    }

    #[test]
    fn text_applies_background() {
        let bg: BackgroundFn = Arc::new(|text| format!("[{text}]"));
        let mut text = Text::new("x", 0, 0).with_background(bg);
        assert_eq!(text.render(3), vec!["[x  ]"]);
    }

    #[test]
    fn truncated_text_uses_first_line_and_padding() {
        let mut text = TruncatedText::new("abcdef\nsecond", 1, 1);
        let lines = text.render(6);
        assert_eq!(lines.len(), 3);
        assert_eq!(visible_width(&lines[0]), 6);
        assert_eq!(visible_width(&lines[1]), 6);
        assert!(lines[1].contains("..."));
    }

    #[test]
    fn box_component_renders_children_with_padding() {
        let mut container = BoxComponent::new(1, 1);
        container.add_child(Box::new(Text::new("hi", 0, 0)));
        let lines = container.render(6);
        assert_eq!(lines.len(), 3);
        assert!(lines.iter().all(|line| visible_width(line) == 6));
        assert_eq!(lines[1], " hi   ");
    }

    #[test]
    fn box_component_applies_background_and_clear() {
        let bg: BackgroundFn = Arc::new(|text| format!("<{text}>"));
        let mut container = BoxComponent::new(0, 0).with_background(bg);
        container.add_child(Box::new(Text::new("x", 0, 0)));
        assert_eq!(container.render(3), vec!["<x  >"]);
        container.clear();
        assert!(container.render(3).is_empty());
    }

    #[test]
    fn container_renders_children_in_order_and_can_clear() {
        let mut container = Container::new();
        container.add_child(Text::new("first", 0, 0));
        container.add_child(Text::new("second", 0, 0));

        assert_eq!(
            container.render(8),
            vec!["first   ".to_string(), "second  ".to_string()]
        );

        container.clear();
        assert!(container.render(8).is_empty());
    }

    #[test]
    fn container_invalidate_propagates_to_children() {
        let mut container = Container::new();
        container.add_child(InvalidationProbe::new());

        container.invalidate();

        assert_eq!(container.render(10), vec!["invalidated".to_string()]);
    }

    #[test]
    fn loader_renders_and_ticks_frames() {
        let identity = Arc::new(str::to_string);
        let mut loader = Loader::new(
            identity.clone(),
            identity,
            "Loading...",
            Some(LoaderIndicatorOptions {
                frames: Some(vec!["a".to_string(), "b".to_string()]),
                interval_ms: Some(10),
            }),
        );
        assert!(loader.is_running());
        assert_eq!(loader.interval_ms(), 10);
        assert!(loader.render(20)[1].contains("a Loading..."));
        loader.tick();
        assert!(loader.render(20)[1].contains("b Loading..."));
        loader.stop();
        assert!(!loader.is_running());
    }

    #[test]
    fn loader_renders_into_ratatui_buffer() {
        let identity = Arc::new(str::to_string);
        let mut loader = Loader::new(
            identity.clone(),
            identity,
            "Loading...",
            Some(LoaderIndicatorOptions {
                frames: Some(vec!["a".to_string(), "b".to_string()]),
                interval_ms: Some(10),
            }),
        );
        let area = Rect::new(0, 0, 20, 3);
        let mut buffer = Buffer::empty(area);

        loader.render_ratatui(area, &mut buffer);

        let rendered = buffer
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(rendered.contains("a Loading..."));
    }

    #[test]
    fn component_trait_renders_directly_into_ratatui_buffer() {
        let mut text = Text::new("direct", 0, 0);
        let area = Rect::new(2, 1, 8, 1);
        let mut buffer = Buffer::empty(Rect::new(0, 0, 12, 3));

        text.render_ratatui(area, &mut buffer);

        assert_eq!(buffer.cell((2, 1)).expect("cell").symbol(), "d");
        assert_eq!(buffer.cell((7, 1)).expect("cell").symbol(), "t");
        assert_eq!(buffer.cell((8, 1)).expect("cell").symbol(), " ");
    }

    #[test]
    fn boxed_component_trait_object_renders_as_ratatui_widget() {
        let mut component: Box<dyn Component> = Box::new(Text::new("boxed", 0, 0));
        let area = Rect::new(1, 1, 8, 1);
        let mut buffer = Buffer::empty(Rect::new(0, 0, 12, 3));

        ratatui::widgets::Widget::render(&mut *component, area, &mut buffer);

        assert_eq!(buffer.cell((1, 1)).expect("cell").symbol(), "b");
        assert_eq!(buffer.cell((5, 1)).expect("cell").symbol(), "d");
        assert_eq!(buffer.cell((6, 1)).expect("cell").symbol(), " ");
    }

    #[test]
    fn boxed_component_trait_object_can_call_render_ratatui_directly() {
        let mut component: Box<dyn Component> = Box::new(Text::new("object", 0, 0));
        let area = Rect::new(1, 1, 8, 1);
        let mut buffer = Buffer::empty(Rect::new(0, 0, 12, 3));

        component.render_ratatui(area, &mut buffer);

        assert_eq!(buffer.cell((1, 1)).expect("cell").symbol(), "o");
        assert_eq!(buffer.cell((6, 1)).expect("cell").symbol(), "t");
        assert_eq!(buffer.cell((7, 1)).expect("cell").symbol(), " ");
    }

    #[test]
    fn cancellable_loader_aborts_on_cancel_key() {
        let identity = Arc::new(str::to_string);
        let mut loader = CancellableLoader::new(identity.clone(), identity, "Working", None);
        let calls = Arc::new(AtomicUsize::new(0));
        let calls_for_callback = calls.clone();
        loader.set_on_abort(move || {
            calls_for_callback.fetch_add(1, Ordering::SeqCst);
        });
        loader.handle_input("\x1b", &KeybindingsManager::default());
        assert!(loader.aborted());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn select_list_filters_renders_and_handles_input() {
        let mut list = SelectList::new(
            vec![
                SelectItem {
                    value: "alpha".to_string(),
                    label: "Alpha".to_string(),
                    description: Some("First item".to_string()),
                },
                SelectItem {
                    value: "beta".to_string(),
                    label: "Beta".to_string(),
                    description: Some("Second item".to_string()),
                },
            ],
            5,
            SelectListTheme::default(),
            SelectListLayoutOptions::default(),
        );

        let lines = list.render(60);
        assert!(lines[0].contains("→ Alpha"));
        assert!(lines[0].contains("First item"));
        list.handle_input("\x1b[B", &KeybindingsManager::default());
        assert_eq!(
            list.selected_item().map(|item| item.value.as_str()),
            Some("beta")
        );
        list.set_filter("a");
        assert_eq!(
            list.selected_item().map(|item| item.value.as_str()),
            Some("alpha")
        );
        list.set_filter("z");
        assert_eq!(list.render(20), vec!["  No matching commands"]);
    }

    #[test]
    fn select_list_renders_into_ratatui_buffer() {
        let mut list = SelectList::new(
            vec![
                SelectItem {
                    value: "alpha".to_string(),
                    label: "Alpha".to_string(),
                    description: Some("First item".to_string()),
                },
                SelectItem {
                    value: "beta".to_string(),
                    label: "Beta".to_string(),
                    description: Some("Second item".to_string()),
                },
            ],
            5,
            SelectListTheme::default(),
            SelectListLayoutOptions::default(),
        );
        list.set_selected_index(1);
        let area = Rect::new(0, 0, 60, 4);
        let mut buffer = Buffer::empty(area);

        list.render_ratatui(area, &mut buffer);

        let rendered = buffer
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(rendered.contains("Beta"));
        assert!(rendered.contains("Second item"));
    }

    #[test]
    fn select_list_exposes_ratatui_list_state_for_selected_item() {
        let mut list = SelectList::new(
            vec![
                SelectItem {
                    value: "alpha".to_string(),
                    label: "Alpha".to_string(),
                    description: Some("First item".to_string()),
                },
                SelectItem {
                    value: "beta".to_string(),
                    label: "Beta".to_string(),
                    description: Some("Second item".to_string()),
                },
            ],
            5,
            SelectListTheme::default(),
            SelectListLayoutOptions::default(),
        );
        list.set_selected_index(1);
        let area = Rect::new(0, 0, 60, 4);
        let mut buffer = Buffer::empty(area);
        let mut state = list.ratatui_list_state();

        list.render_ratatui_stateful(area, &mut buffer, &mut state);

        assert_eq!(state.selected(), Some(1));
        let rendered = buffer
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(rendered.contains("Beta"));
    }
}
