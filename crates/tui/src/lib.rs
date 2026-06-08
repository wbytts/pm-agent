mod autocomplete;
pub mod components;
mod countdown_timer;
mod editor_component;
mod fuzzy;
mod keybindings;
mod keys;
mod kill_ring;
mod native_modifiers;
mod ratatui_bridge;
mod stdin_buffer;
mod terminal;
mod terminal_image;
mod tui_state;
mod undo_stack;
mod utils;

pub use ratatui::backend::{
    Backend as RatatuiBackend, CrosstermBackend as RatatuiCrosstermBackend,
};
pub use ratatui::buffer::Buffer as RatatuiBuffer;
pub use ratatui::layout::Rect as RatatuiRect;
pub use ratatui::widgets::{StatefulWidget as RatatuiStatefulWidget, Widget as RatatuiWidget};
pub use ratatui::{self as ratatui_runtime, Frame as RatatuiFrame, Terminal as RatatuiRawTerminal};
pub type RatatuiTerminal<W> = RatatuiRawTerminal<RatatuiCrosstermBackend<W>>;

pub use autocomplete::{
    AutocompleteItem, AutocompleteSuggestions, CombinedAutocompleteProvider, CompletionResult,
    SlashCommand,
};
pub use components::{
    BackgroundFn, BorderedLoader, BorderedLoaderOptions, BoxComponent, BoxComponent as Box,
    CancellableLoader, Component, Container, DefaultTextStyle, DynamicBorder, Editor,
    EditorOptions, EditorState, EditorTheme, Image, ImageOptions, ImageTheme, Input, Loader,
    LoaderIndicatorOptions, Markdown, MarkdownTheme, SelectItem, SelectList,
    SelectListLayoutOptions, SelectListTheme, SelectListTruncatePrimaryContext, SettingItem,
    SettingsList, SettingsListOptions, SettingsListTheme, SettingsSubmenu, SettingsSubmenuDone,
    Spacer, Text, TruncatedText, CURSOR_MARKER,
};
pub use countdown_timer::{CountdownTick, CountdownTimer};
pub use editor_component::EditorComponent;
pub use fuzzy::{fuzzy_filter, fuzzy_match, FuzzyMatch};
pub use keybindings::{
    default_keybindings, get_keybindings, set_keybindings, KeybindingConflict,
    KeybindingDefinition, KeybindingsConfig, KeybindingsManager,
};
pub use keys::{
    decode_kitty_printable, decode_printable_key, is_key_release, is_key_repeat,
    is_kitty_protocol_active, matches_key, parse_key, parse_key_id, set_kitty_protocol_active, Key,
    ParsedKeyId,
};
pub use kill_ring::KillRing;
pub use native_modifiers::{is_native_modifier_pressed, ModifierKey};
pub use ratatui_bridge::{
    create_ratatui_terminal, FocusHandle, InputListenerResult, OverlayHandle, RatatuiComponent,
    RatatuiInputComponent, RatatuiRenderError, RatatuiRenderPlan, RatatuiRenderPlanKind,
    RatatuiTui, RenderRequest, Tui, TUI,
};
pub use stdin_buffer::{StdinBuffer, StdinBufferEvent};
pub use terminal::{
    bracketed_paste_sequence, clear_from_cursor_sequence, clear_line_sequence,
    clear_progress_sequence, clear_screen_sequence, disable_kitty_keyboard_protocol_sequence,
    disable_modify_other_keys_sequence, enable_bracketed_paste_sequence,
    enable_kitty_keyboard_protocol_sequence, enable_modify_other_keys_sequence,
    hide_cursor_sequence, is_apple_terminal_session, move_by_sequence,
    normalize_apple_terminal_input, normalize_terminal_input,
    query_kitty_keyboard_protocol_sequence, resolve_terminal_dimensions, set_title_sequence,
    show_cursor_sequence, start_progress_sequence, ProcessTerminal, Terminal, TerminalInputContext,
    APPLE_TERMINAL_SHIFT_ENTER_SEQUENCE, TERMINAL_PROGRESS_KEEPALIVE_MS,
};
pub use terminal_image::{
    allocate_image_id, calculate_image_cell_size, calculate_image_rows, delete_all_kitty_images,
    delete_kitty_image, detect_capabilities, encode_iterm2, encode_kitty, get_capabilities,
    get_cell_dimensions, get_gif_dimensions, get_image_dimensions, get_jpeg_dimensions,
    get_png_dimensions, get_webp_dimensions, hyperlink, image_fallback, is_image_line,
    render_image, reset_capabilities_cache, set_capabilities, set_cell_dimensions, CellDimensions,
    ImageCellSize, ImageDimensions, ImageProtocol, ImageRenderOptions, ImageRenderResult,
    TerminalCapabilities,
};
pub use tui_state::{
    apply_line_resets, collect_kitty_image_ids, composite_line_at, composite_overlays,
    extract_cursor_position, extract_kitty_image_ids, resolve_overlay_layout, CursorPosition,
    OverlayAnchor, OverlayLayout, OverlayMargin, OverlayOptions, RenderedOverlay, SizeValue,
};
pub use undo_stack::UndoStack;
pub use utils::{
    apply_background_to_line, extract_ansi_code, extract_segments, is_punctuation_char,
    is_whitespace_char, normalize_terminal_output, slice_by_column, slice_with_width,
    truncate_to_visual_lines, truncate_to_width, visible_width, wrap_text_with_ansi, AnsiCode,
    ExtractSegmentsResult, SliceWithWidth, VisualTruncateResult,
};

#[cfg(test)]
mod ratatui_tests {
    use super::{
        components::{BoxComponent, DynamicBorder, Loader, Spacer, Text, TruncatedText},
        Component, Container, OverlayAnchor, OverlayOptions, RatatuiComponent,
        RatatuiInputComponent, RatatuiTui, Tui, TUI,
    };
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;
    use ratatui::style::{Color, Modifier};
    use ratatui::widgets::Widget;
    use ratatui::Terminal;
    use std::cell::RefCell;
    use std::rc::Rc;

    #[test]
    fn ratatui_terminal_factory_creates_crossterm_backend_for_real_terminal_loop() {
        let mut terminal = super::create_ratatui_terminal(Vec::<u8>::new()).expect("terminal");
        let mut tui = RatatuiTui::new();
        tui.add_child(Text::new("real", 0, 0));

        terminal
            .draw(|frame| tui.render_frame(frame, frame.area()))
            .expect("draw");

        assert!(tui.last_frame_size_for_test().is_some());
    }

    #[test]
    fn ratatui_component_renders_existing_component_lines_into_buffer() {
        let area = Rect::new(0, 0, 8, 3);
        let mut buffer = Buffer::empty(area);
        let component = RatatuiComponent::new(Text::new("hello", 0, 0));

        component.render(area, &mut buffer);

        assert_eq!(buffer.cell((0, 0)).expect("cell").symbol(), "h");
        assert_eq!(buffer.cell((4, 0)).expect("cell").symbol(), "o");
        assert_eq!(buffer.cell((5, 0)).expect("cell").symbol(), " ");
    }

    #[test]
    fn ratatui_component_can_render_by_mutable_reference_without_consuming_state() {
        let area = Rect::new(0, 0, 8, 3);
        let mut component = RatatuiComponent::new(Text::new("first", 0, 0));

        let mut first_buffer = Buffer::empty(area);
        (&mut component).render(area, &mut first_buffer);

        let mut second_buffer = Buffer::empty(area);
        (&mut component).render(area, &mut second_buffer);

        assert_eq!(first_buffer.cell((0, 0)).expect("cell").symbol(), "f");
        assert_eq!(second_buffer.cell((0, 0)).expect("cell").symbol(), "f");
    }

    #[test]
    fn ratatui_component_renders_existing_component_through_frame() {
        let backend = TestBackend::new(10, 4);
        let mut terminal = Terminal::new(backend).expect("terminal");
        let mut component = RatatuiComponent::new(Text::new("frame", 0, 0));

        terminal
            .draw(|frame| component.render_frame(frame, Rect::new(2, 1, 5, 1)))
            .expect("draw");

        let buffer = terminal.backend().buffer();
        assert_eq!(buffer.cell((2, 1)).expect("cell").symbol(), "f");
        assert_eq!(buffer.cell((6, 1)).expect("cell").symbol(), "e");
        assert_eq!(buffer.cell((7, 1)).expect("cell").symbol(), " ");
    }

    #[test]
    fn common_components_render_as_ratatui_widgets_without_adapter() {
        let backend = TestBackend::new(12, 4);
        let mut terminal = Terminal::new(backend).expect("terminal");
        let mut container = Container::new();
        container.add_child(Text::new("direct", 0, 0));

        terminal
            .draw(|frame| frame.render_widget(&mut container, Rect::new(1, 1, 8, 1)))
            .expect("draw");

        let buffer = terminal.backend().buffer();
        assert_eq!(buffer.cell((1, 1)).expect("cell").symbol(), "d");
        assert_eq!(buffer.cell((6, 1)).expect("cell").symbol(), "t");
        assert_eq!(buffer.cell((7, 1)).expect("cell").symbol(), " ");
    }

    #[test]
    fn base_components_render_as_ratatui_widgets_without_adapter() {
        let backend = TestBackend::new(24, 8);
        let mut terminal = Terminal::new(backend).expect("terminal");
        let identity = std::sync::Arc::new(str::to_string);
        let mut text = Text::new("text", 0, 0);
        let mut truncated = TruncatedText::new("truncate", 0, 0);
        let mut spacer = Spacer::new(1);
        let mut loader = Loader::new(identity.clone(), identity.clone(), "load", None);
        let mut border = DynamicBorder::default();
        let mut boxed = BoxComponent::new(1, 0);
        boxed.add_child(Box::new(Text::new("boxed", 0, 0)));

        terminal
            .draw(|frame| {
                frame.render_widget(&mut text, Rect::new(0, 0, 8, 1));
                frame.render_widget(&mut truncated, Rect::new(0, 1, 8, 1));
                frame.render_widget(&mut spacer, Rect::new(0, 2, 8, 1));
                frame.render_widget(&mut loader, Rect::new(0, 3, 12, 2));
                frame.render_widget(&mut border, Rect::new(0, 5, 8, 1));
                frame.render_widget(&mut boxed, Rect::new(0, 6, 8, 1));
            })
            .expect("draw");

        let buffer = terminal.backend().buffer();
        assert_eq!(buffer.cell((0, 0)).expect("cell").symbol(), "t");
        assert_eq!(buffer.cell((0, 1)).expect("cell").symbol(), "t");
        assert_eq!(buffer.cell((0, 2)).expect("cell").symbol(), " ");
        assert_eq!(buffer.cell((3, 4)).expect("cell").symbol(), "l");
        assert_eq!(buffer.cell((0, 5)).expect("cell").symbol(), "─");
        assert_eq!(buffer.cell((1, 6)).expect("cell").symbol(), "b");
    }

    #[test]
    fn crate_root_reexports_ratatui_runtime_types_for_downstream_loops() {
        let area: super::RatatuiRect = super::RatatuiRect::new(0, 0, 8, 2);
        let mut buffer = super::RatatuiBuffer::empty(area);
        let component = RatatuiComponent::new(Text::new("api", 0, 0));
        super::RatatuiWidget::render(component, area, &mut buffer);
        assert_eq!(buffer.cell((0, 0)).expect("cell").symbol(), "a");

        let mut terminal: super::RatatuiTerminal<Vec<u8>> =
            super::create_ratatui_terminal(Vec::<u8>::new()).expect("terminal");
        let mut tui = RatatuiTui::new();
        terminal
            .draw(|frame: &mut super::RatatuiFrame<'_>| {
                tui.render_frame(frame, frame.area());
            })
            .expect("draw");

        let _backend: super::RatatuiCrosstermBackend<Vec<u8>>;
    }

    #[test]
    fn select_list_renders_as_stateful_ratatui_widget() {
        let backend = TestBackend::new(20, 4);
        let mut terminal = Terminal::new(backend).expect("terminal");
        let mut list = super::SelectList::new(
            vec![
                super::SelectItem {
                    value: "alpha".to_string(),
                    label: "Alpha".to_string(),
                    description: None,
                },
                super::SelectItem {
                    value: "beta".to_string(),
                    label: "Beta".to_string(),
                    description: None,
                },
            ],
            2,
            super::SelectListTheme::default(),
            super::SelectListLayoutOptions::default(),
        );
        list.set_selected_index(1);
        let mut state = list.ratatui_list_state();

        terminal
            .draw(|frame| {
                frame.render_stateful_widget(&mut list, Rect::new(0, 0, 20, 2), &mut state)
            })
            .expect("draw");

        assert_eq!(state.selected(), Some(1));
        let buffer = terminal.backend().buffer();
        assert_eq!(buffer.cell((2, 1)).expect("cell").symbol(), "B");
    }

    #[test]
    fn ratatui_tui_renders_base_components_and_overlays_through_frame() {
        let backend = TestBackend::new(20, 5);
        let mut terminal = Terminal::new(backend).expect("terminal");
        let mut tui = RatatuiTui::new();
        tui.add_child(Text::new("base", 0, 0));
        tui.show_overlay(
            Text::new("overlay", 0, 0),
            OverlayOptions {
                width: Some(super::SizeValue::Cells(10)),
                anchor: Some(OverlayAnchor::BottomRight),
                ..OverlayOptions::default()
            },
        );

        terminal
            .draw(|frame| tui.render_frame(frame, frame.area()))
            .expect("draw");

        let buffer = terminal.backend().buffer();
        assert_eq!(buffer.cell((0, 0)).expect("cell").symbol(), "b");
        assert_eq!(buffer.cell((10, 4)).expect("cell").symbol(), "o");
        assert_eq!(buffer.cell((16, 4)).expect("cell").symbol(), "y");
    }

    #[test]
    fn ratatui_tui_widget_render_commits_render_plan_state_like_direct_render() {
        let area = Rect::new(0, 0, 8, 3);
        let mut buffer = Buffer::empty(area);
        let mut tui = RatatuiTui::new();
        tui.add_child(Text::new("state", 0, 0));

        (&mut tui).render(area, &mut buffer);

        let state = tui.render_state_for_test();
        assert_eq!(tui.last_frame_size_for_test(), Some((8, 3)));
        assert_eq!(state.previous_width, 8);
        assert_eq!(state.previous_height, 3);
        assert!(state.previous_lines[0].starts_with("state   "));
    }

    #[test]
    fn ratatui_tui_overlay_handle_hides_shows_and_removes_overlay() {
        let mut tui = RatatuiTui::new();
        tui.add_child(Text::new("base", 0, 0));
        let overlay = tui.show_overlay(
            Text::new("modal", 0, 0),
            OverlayOptions {
                width: Some(super::SizeValue::Cells(8)),
                anchor: Some(OverlayAnchor::TopLeft),
                ..OverlayOptions::default()
            },
        );

        overlay.set_hidden(&mut tui, true);
        assert!(overlay.is_hidden(&tui));
        assert!(!tui.has_overlay());

        overlay.set_hidden(&mut tui, false);
        assert!(!overlay.is_hidden(&tui));
        assert!(tui.has_overlay());

        overlay.hide(&mut tui);
        assert!(!tui.has_overlay());
        assert!(overlay.is_hidden(&tui));
    }

    #[test]
    fn ratatui_tui_overlay_visibility_predicate_controls_rendering_by_frame_size() {
        let mut tui = RatatuiTui::new();
        tui.add_child(Text::new("base", 0, 0));
        tui.show_overlay_when(
            Text::new("wide", 0, 0),
            OverlayOptions {
                width: Some(super::SizeValue::Cells(6)),
                anchor: Some(OverlayAnchor::TopLeft),
                ..OverlayOptions::default()
            },
            |width, _height| width >= 12,
        );

        let narrow = tui.render_lines_for_test(10, 3);
        assert!(narrow.iter().all(|line| !line.contains("wide")));

        let wide = tui.render_lines_for_test(12, 3);
        assert!(wide.iter().any(|line| line.contains("wide")));
    }

    #[test]
    fn ratatui_tui_input_redirects_when_focused_overlay_is_not_visible_at_last_frame_size() {
        let base_events = Rc::new(RefCell::new(Vec::new()));
        let overlay_events = Rc::new(RefCell::new(Vec::new()));
        let mut tui = RatatuiTui::new();
        let base_focus = tui.add_focusable_child(InputProbe {
            events: base_events.clone(),
        });
        tui.set_focus(Some(base_focus));

        let overlay = tui.show_focusable_overlay_when(
            InputProbe {
                events: overlay_events.clone(),
            },
            OverlayOptions {
                width: Some(super::SizeValue::Cells(6)),
                anchor: Some(OverlayAnchor::TopLeft),
                ..OverlayOptions::default()
            },
            |width, _height| width >= 12,
        );

        assert!(overlay.is_focused(&tui));
        tui.render_lines_for_test(10, 3);

        assert!(tui.handle_input("base"));
        assert_eq!(tui.focused(), Some(base_focus));
        assert_eq!(base_events.borrow().as_slice(), ["base"]);
        assert!(overlay_events.borrow().is_empty());
    }

    #[test]
    fn ratatui_tui_request_render_tracks_pending_request_and_force_resets_frame_state() {
        let mut tui = RatatuiTui::new();
        tui.start();
        tui.take_render_request();
        tui.add_child(Text::new("base", 0, 0));
        tui.render_lines_for_test(10, 3);
        assert_eq!(tui.last_frame_size_for_test(), Some((10, 3)));

        tui.request_render(false);
        tui.request_render(false);

        assert!(tui.render_requested());
        assert_eq!(
            tui.take_render_request(),
            Some(super::RenderRequest { force: false })
        );
        assert_eq!(tui.take_render_request(), None);
        assert_eq!(tui.last_frame_size_for_test(), Some((10, 3)));

        tui.request_render(true);

        assert!(tui.render_requested());
        assert_eq!(tui.last_frame_size_for_test(), None);
        assert_eq!(
            tui.take_render_request(),
            Some(super::RenderRequest { force: true })
        );
    }

    #[test]
    fn ratatui_tui_force_render_resets_previous_render_state_like_pi() {
        let mut tui = RatatuiTui::new();
        tui.start();
        tui.take_render_request();
        tui.add_child(Text::new("\x1b_Ga=T,i=42;payload\x1b\\", 0, 0));

        tui.render_lines_for_test(10, 3);
        let state = tui.render_state_for_test();
        assert_eq!(state.previous_width, 10);
        assert_eq!(state.previous_height, 3);
        assert!(!state.previous_lines.is_empty());
        assert_eq!(state.previous_kitty_image_ids, vec![42]);
        assert!(state.max_lines_rendered > 0);

        tui.request_render(true);
        let state = tui.render_state_for_test();

        assert!(state.previous_lines.is_empty());
        assert!(state.previous_kitty_image_ids.is_empty());
        assert_eq!(state.previous_width, -1);
        assert_eq!(state.previous_height, -1);
        assert_eq!(state.cursor_row, 0);
        assert_eq!(state.hardware_cursor_row, 0);
        assert_eq!(state.max_lines_rendered, 0);
        assert_eq!(state.previous_viewport_top, 0);
        assert_eq!(tui.last_frame_size_for_test(), None);
    }

    #[test]
    fn ratatui_tui_render_plan_tracks_first_full_render_width_change_and_no_change_like_pi() {
        let mut tui = RatatuiTui::new();
        tui.add_child(Text::new("base", 0, 0));

        let first = tui.render_plan_for_test(10, 3);
        assert_eq!(
            first.kind,
            super::RatatuiRenderPlanKind::Full { clear: false }
        );
        assert_eq!(first.first_changed, Some(0));
        assert_eq!(tui.full_redraws_for_test(), 1);

        let unchanged = tui.render_plan_for_test(10, 3);
        assert_eq!(unchanged.kind, super::RatatuiRenderPlanKind::Unchanged);
        assert_eq!(unchanged.first_changed, None);
        assert_eq!(tui.full_redraws_for_test(), 1);

        let resized = tui.render_plan_for_test(12, 3);
        assert_eq!(
            resized.kind,
            super::RatatuiRenderPlanKind::Full { clear: true }
        );
        assert_eq!(resized.first_changed, Some(0));
        assert_eq!(tui.full_redraws_for_test(), 2);
    }

    #[test]
    fn ratatui_tui_render_plan_tracks_changed_range_and_append_start_like_pi() {
        let lines = Rc::new(RefCell::new(vec!["one".to_string(), "two".to_string()]));
        let mut tui = RatatuiTui::new();
        tui.add_child(SharedLinesProbe {
            lines: lines.clone(),
        });

        tui.render_plan_for_test(12, 5);
        lines.borrow_mut().push("three".to_string());
        let appended = tui.render_plan_for_test(12, 5);

        assert_eq!(appended.kind, super::RatatuiRenderPlanKind::Differential);
        assert_eq!(appended.first_changed, Some(2));
        assert_eq!(appended.last_changed, Some(2));
        assert!(appended.append_start);

        lines.borrow_mut()[1] = "TWO".to_string();
        let changed = tui.render_plan_for_test(12, 5);

        assert_eq!(changed.kind, super::RatatuiRenderPlanKind::Differential);
        assert_eq!(changed.first_changed, Some(1));
        assert_eq!(changed.last_changed, Some(1));
        assert!(!changed.append_start);
    }

    #[test]
    fn ratatui_tui_render_plan_distinguishes_deleted_lines_and_viewport_full_redraw_like_pi() {
        let lines = Rc::new(RefCell::new(vec![
            "one".to_string(),
            "two".to_string(),
            "three".to_string(),
            "four".to_string(),
            "five".to_string(),
        ]));
        let mut tui = RatatuiTui::new();
        tui.add_child(SharedLinesProbe {
            lines: lines.clone(),
        });

        tui.render_plan_for_test(12, 3);
        lines.borrow_mut().truncate(2);
        let deleted_above_viewport = tui.render_plan_for_test(12, 3);

        assert_eq!(
            deleted_above_viewport.kind,
            super::RatatuiRenderPlanKind::Full { clear: true }
        );
        assert_eq!(deleted_above_viewport.first_changed, Some(2));
        assert_eq!(deleted_above_viewport.last_changed, Some(4));
        assert_eq!(tui.full_redraws_for_test(), 2);

        lines
            .borrow_mut()
            .extend(["three".to_string(), "four".to_string()]);
        tui.render_plan_for_test(12, 6);
        lines.borrow_mut().truncate(2);
        let deleted_after_content = tui.render_plan_for_test(12, 6);

        assert_eq!(
            deleted_after_content.kind,
            super::RatatuiRenderPlanKind::DeletedLines
        );
        assert_eq!(deleted_after_content.first_changed, Some(2));
        assert_eq!(deleted_after_content.last_changed, Some(3));
    }

    #[test]
    fn ratatui_tui_clear_on_shrink_forces_full_redraw_when_enabled_like_pi() {
        let lines = Rc::new(RefCell::new(vec![
            "one".to_string(),
            "two".to_string(),
            "three".to_string(),
        ]));
        let mut tui = RatatuiTui::new();
        tui.add_child(SharedLinesProbe {
            lines: lines.clone(),
        });

        assert!(!tui.get_clear_on_shrink());
        tui.render_plan_for_test(12, 6);
        lines.borrow_mut().truncate(1);
        let default_shrink = tui.render_plan_for_test(12, 6);
        assert_eq!(
            default_shrink.kind,
            super::RatatuiRenderPlanKind::DeletedLines
        );

        lines
            .borrow_mut()
            .extend(["two".to_string(), "three".to_string()]);
        tui.render_plan_for_test(12, 6);
        tui.set_clear_on_shrink(true);
        assert!(tui.get_clear_on_shrink());
        lines.borrow_mut().truncate(1);

        let forced_clear = tui.render_plan_for_test(12, 6);
        assert_eq!(
            forced_clear.kind,
            super::RatatuiRenderPlanKind::Full { clear: true }
        );
    }

    #[test]
    fn ratatui_tui_clear_on_shrink_defaults_from_pi_environment_flag() {
        let enabled = RatatuiTui::new_with_environment_for_test(None, Some("1"));
        let disabled = RatatuiTui::new_with_environment_for_test(None, Some("0"));
        let missing = RatatuiTui::new_with_environment_for_test(None, None);

        assert!(enabled.get_clear_on_shrink());
        assert!(!disabled.get_clear_on_shrink());
        assert!(!missing.get_clear_on_shrink());
    }

    #[test]
    fn ratatui_tui_render_plan_tracks_render_end_and_final_cursor_row_like_pi() {
        let lines = Rc::new(RefCell::new(vec![
            "one".to_string(),
            "two".to_string(),
            "three".to_string(),
        ]));
        let mut tui = RatatuiTui::new();
        tui.add_child(SharedLinesProbe {
            lines: lines.clone(),
        });

        tui.render_plan_for_test(12, 5);
        lines.borrow_mut()[1] = "TWO".to_string();
        let changed = tui.render_plan_for_test(12, 5);

        assert_eq!(changed.render_end, Some(1));
        assert_eq!(changed.final_cursor_row, Some(1));
        assert_eq!(tui.render_state_for_test().hardware_cursor_row, 1);

        lines.borrow_mut().truncate(2);
        lines.borrow_mut()[0] = "ONE".to_string();
        let shrunk = tui.render_plan_for_test(12, 5);

        assert_eq!(shrunk.kind, super::RatatuiRenderPlanKind::Differential);
        assert_eq!(shrunk.render_end, Some(1));
        assert_eq!(shrunk.final_cursor_row, Some(1));
        assert_eq!(tui.render_state_for_test().hardware_cursor_row, 1);
    }

    #[test]
    fn ratatui_tui_render_plan_keeps_hardware_cursor_row_when_output_is_unchanged_like_pi() {
        let lines = Rc::new(RefCell::new(vec![
            format!("one{}", super::components::CURSOR_MARKER),
            "two".to_string(),
        ]));
        let mut tui = RatatuiTui::new();
        tui.add_child(SharedLinesProbe {
            lines: lines.clone(),
        });

        tui.render_plan_for_test(12, 5);
        lines.borrow_mut()[0] = "one".to_string();
        lines.borrow_mut()[1] = format!("two{}", super::components::CURSOR_MARKER);
        let unchanged_with_cursor_move = tui.render_plan_for_test(12, 5);

        assert_eq!(
            unchanged_with_cursor_move.kind,
            super::RatatuiRenderPlanKind::Unchanged
        );
        assert_eq!(unchanged_with_cursor_move.hardware_cursor_row, Some(1));
        assert_eq!(tui.render_state_for_test().hardware_cursor_row, 1);
    }

    #[test]
    fn ratatui_tui_exposes_hardware_cursor_visibility_toggle_like_pi() {
        let mut tui = RatatuiTui::new();

        assert!(!tui.get_show_hardware_cursor());
        tui.set_show_hardware_cursor(true);
        assert!(tui.get_show_hardware_cursor());
        tui.set_show_hardware_cursor(false);
        assert!(!tui.get_show_hardware_cursor());
    }

    #[test]
    fn ratatui_tui_render_plan_preserves_hardware_cursor_row_and_col_like_pi() {
        let lines = Rc::new(RefCell::new(vec![format!(
            "ab{}",
            super::components::CURSOR_MARKER
        )]));
        let mut tui = RatatuiTui::new();
        tui.add_child(SharedLinesProbe {
            lines: lines.clone(),
        });

        let plan = tui.render_plan_for_test(8, 3);

        assert_eq!(
            plan.hardware_cursor_position,
            Some(super::CursorPosition { row: 0, col: 2 })
        );
        assert_eq!(plan.hardware_cursor_row, Some(0));
        assert!(!plan.lines[0].contains(super::components::CURSOR_MARKER));
    }

    #[test]
    fn ratatui_tui_render_frame_sets_ratatui_cursor_only_when_enabled_like_pi() {
        let mut hidden_terminal = Terminal::new(TestBackend::new(10, 4)).expect("terminal");
        let mut hidden_tui = RatatuiTui::new();
        hidden_tui.add_child(Text::new(
            format!("ab{}", super::components::CURSOR_MARKER),
            0,
            0,
        ));

        hidden_terminal
            .draw(|frame| hidden_tui.render_frame(frame, Rect::new(3, 1, 6, 2)))
            .expect("draw");

        assert!(!hidden_terminal.backend().cursor_visible());

        let mut shown_terminal = Terminal::new(TestBackend::new(10, 4)).expect("terminal");
        let mut shown_tui = RatatuiTui::new();
        shown_tui.set_show_hardware_cursor(true);
        shown_tui.add_child(Text::new(
            format!("ab{}", super::components::CURSOR_MARKER),
            0,
            0,
        ));

        shown_terminal
            .draw(|frame| shown_tui.render_frame(frame, Rect::new(3, 1, 6, 2)))
            .expect("draw");

        assert!(shown_terminal.backend().cursor_visible());
        assert_eq!(shown_terminal.backend().cursor_position(), (5, 1).into());
    }

    #[test]
    fn ratatui_tui_render_plan_reports_non_image_line_width_overflow_like_pi() {
        let lines = Rc::new(RefCell::new(vec!["too-wide".to_string()]));
        let mut tui = RatatuiTui::new();
        tui.add_child(SharedLinesProbe {
            lines: lines.clone(),
        });

        let error = tui.render_plan_result_for_test(3, 2).expect_err("overflow");

        assert_eq!(error.line_index, 0);
        assert_eq!(error.line_width, 8);
        assert_eq!(error.terminal_width, 3);
        assert!(error
            .to_string()
            .contains("Rendered line 0 exceeds terminal width"));

        lines.borrow_mut()[0] = "\x1b_Ga=T,i=7;payload\x1b\\".to_string();
        let image_plan = tui
            .render_plan_result_for_test(3, 2)
            .expect("image lines bypass width guard");
        assert_eq!(
            image_plan.kind,
            super::RatatuiRenderPlanKind::Full { clear: false }
        );
    }

    #[test]
    fn ratatui_tui_start_stop_gate_render_requests_like_pi_lifecycle() {
        let mut tui = RatatuiTui::new();

        assert!(tui.is_stopped());
        tui.request_render(false);
        assert_eq!(tui.take_render_request(), None);

        tui.start();

        assert!(!tui.is_stopped());
        assert_eq!(
            tui.take_render_request(),
            Some(super::RenderRequest { force: false })
        );

        tui.request_render(false);
        tui.stop();

        assert!(tui.is_stopped());
        assert_eq!(tui.take_render_request(), None);
        tui.request_render(true);
        assert_eq!(tui.take_render_request(), None);

        tui.start();

        assert!(!tui.is_stopped());
        assert_eq!(
            tui.take_render_request(),
            Some(super::RenderRequest { force: false })
        );
    }

    #[test]
    fn ratatui_tui_mutating_overlay_and_input_paths_request_render() {
        let base_events = Rc::new(RefCell::new(Vec::new()));
        let overlay_events = Rc::new(RefCell::new(Vec::new()));
        let mut tui = RatatuiTui::new();
        tui.start();
        tui.take_render_request();
        let base_focus = tui.add_focusable_child(InputProbe {
            events: base_events.clone(),
        });
        tui.set_focus(Some(base_focus));

        let overlay = tui.show_focusable_overlay(
            InputProbe {
                events: overlay_events.clone(),
            },
            OverlayOptions::default(),
        );
        assert_eq!(
            tui.take_render_request(),
            Some(super::RenderRequest { force: false })
        );

        overlay.set_hidden(&mut tui, true);
        assert_eq!(
            tui.take_render_request(),
            Some(super::RenderRequest { force: false })
        );

        overlay.set_hidden(&mut tui, false);
        assert_eq!(
            tui.take_render_request(),
            Some(super::RenderRequest { force: false })
        );

        overlay.unfocus(&mut tui);
        assert_eq!(
            tui.take_render_request(),
            Some(super::RenderRequest { force: false })
        );

        overlay.focus(&mut tui);
        assert_eq!(
            tui.take_render_request(),
            Some(super::RenderRequest { force: false })
        );

        assert!(tui.handle_input("overlay"));
        assert_eq!(
            tui.take_render_request(),
            Some(super::RenderRequest { force: false })
        );

        assert!(!tui.handle_input("\x1b[6;24;12t"));
        assert_eq!(
            tui.take_render_request(),
            Some(super::RenderRequest { force: false })
        );

        overlay.hide(&mut tui);
        assert_eq!(
            tui.take_render_request(),
            Some(super::RenderRequest { force: false })
        );
    }

    #[test]
    fn crate_root_exports_container_for_pi_tui_entrypoint_parity() {
        let mut container = Container::new();
        container.add_child(Text::new("root", 0, 0));

        assert_eq!(
            Component::render(&mut container, 6),
            vec!["root  ".to_string()]
        );
    }

    #[test]
    fn crate_root_exports_common_component_types_like_pi_tui_index() {
        let mut boxed = super::Box::new(0, 0);
        boxed.add_child(std::boxed::Box::new(super::Text::new("root", 0, 0)));

        let mut input = super::Input::new();
        input.set_value("value");

        let mut markdown =
            super::Markdown::new("# title", 0, 0, super::MarkdownTheme::default(), None);
        let mut truncated = super::TruncatedText::new("abcdef", 0, 0);
        let mut spacer = super::Spacer::new(1);

        assert_eq!(
            Component::render(&mut boxed, 8),
            vec!["root    ".to_string()]
        );
        assert_eq!(input.value(), "value");
        assert!(!Component::render(&mut markdown, 20).is_empty());
        let truncated_lines = Component::render(&mut truncated, 4);
        assert_eq!(
            super::visible_width(
                truncated_lines
                    .first()
                    .expect("truncated text renders a line")
            ),
            4
        );
        assert_eq!(Component::render(&mut spacer, 4), vec!["".to_string()]);
    }

    #[test]
    fn crate_root_exports_tui_aliases_for_pi_tui_entrypoint_parity() {
        let mut tui = Tui::new();
        tui.add_child(Text::new("alias", 0, 0));
        assert!(tui.render_lines_for_test(8, 3)[0].starts_with("alias"));

        let mut uppercase_tui = TUI::new();
        uppercase_tui.add_child(Text::new("upper", 0, 0));
        assert!(uppercase_tui.render_lines_for_test(8, 3)[0].starts_with("upper"));
    }

    #[derive(Clone)]
    struct InputProbe {
        events: Rc<RefCell<Vec<String>>>,
    }

    impl Component for InputProbe {
        fn render(&mut self, _width: usize) -> Vec<String> {
            Vec::new()
        }
    }

    impl RatatuiInputComponent for InputProbe {
        fn handle_input(&mut self, data: &str) {
            self.events.borrow_mut().push(data.to_string());
        }
    }

    #[test]
    fn ratatui_tui_input_listeners_can_rewrite_and_consume_before_focus_dispatch() {
        let events = Rc::new(RefCell::new(Vec::new()));
        let mut tui = RatatuiTui::new();
        let focus = tui.add_focusable_child(InputProbe {
            events: events.clone(),
        });
        tui.set_focus(Some(focus));

        tui.add_input_listener(|data| {
            if data == "skip" {
                super::InputListenerResult::consume()
            } else {
                super::InputListenerResult::replace(format!("{data}!"))
            }
        });

        assert!(tui.handle_input("go"));
        assert!(!tui.handle_input("skip"));
        assert_eq!(events.borrow().as_slice(), ["go!"]);
    }

    #[test]
    fn ratatui_tui_debug_shortcut_calls_handler_without_forwarding_to_focus() {
        let events = Rc::new(RefCell::new(Vec::new()));
        let debug_calls = Rc::new(RefCell::new(0));
        let mut tui = RatatuiTui::new();
        let focus = tui.add_focusable_child(InputProbe {
            events: events.clone(),
        });
        tui.set_focus(Some(focus));
        tui.set_debug_handler({
            let debug_calls = debug_calls.clone();
            move || {
                *debug_calls.borrow_mut() += 1;
            }
        });

        assert!(!tui.handle_input("\x1b[100;6u"));

        assert_eq!(*debug_calls.borrow(), 1);
        assert!(events.borrow().is_empty());
    }

    #[test]
    fn ratatui_tui_filters_key_release_events_unless_component_opts_in() {
        let default_events = Rc::new(RefCell::new(Vec::new()));
        let mut tui = RatatuiTui::new();
        let default_focus = tui.add_focusable_child(InputProbe {
            events: default_events.clone(),
        });
        tui.set_focus(Some(default_focus));

        assert!(!tui.handle_input("\x1b[65;1:3u"));
        assert!(default_events.borrow().is_empty());

        #[derive(Clone)]
        struct ReleaseProbe {
            events: Rc<RefCell<Vec<String>>>,
        }

        impl Component for ReleaseProbe {
            fn render(&mut self, _width: usize) -> Vec<String> {
                Vec::new()
            }
        }

        impl RatatuiInputComponent for ReleaseProbe {
            fn handle_input(&mut self, data: &str) {
                self.events.borrow_mut().push(data.to_string());
            }

            fn wants_key_release(&self) -> bool {
                true
            }
        }

        let release_events = Rc::new(RefCell::new(Vec::new()));
        let release_focus = tui.add_focusable_child(ReleaseProbe {
            events: release_events.clone(),
        });
        tui.set_focus(Some(release_focus));

        assert!(tui.handle_input("\x1b[65;1:3u"));
        assert_eq!(release_events.borrow().as_slice(), ["\x1b[65;1:3u"]);
    }

    #[derive(Clone)]
    struct SharedInvalidationProbe {
        invalidations: Rc<RefCell<usize>>,
    }

    impl Component for SharedInvalidationProbe {
        fn render(&mut self, _width: usize) -> Vec<String> {
            Vec::new()
        }

        fn invalidate(&mut self) {
            *self.invalidations.borrow_mut() += 1;
        }
    }

    impl RatatuiInputComponent for SharedInvalidationProbe {
        fn handle_input(&mut self, _data: &str) {}
    }

    struct SharedLinesProbe {
        lines: Rc<RefCell<Vec<String>>>,
    }

    impl Component for SharedLinesProbe {
        fn render(&mut self, _width: usize) -> Vec<String> {
            self.lines.borrow().clone()
        }

        fn invalidate(&mut self) {}
    }

    #[test]
    fn ratatui_component_preserves_basic_ansi_sgr_styles_in_buffer() {
        let area = Rect::new(0, 0, 12, 1);
        let mut buffer = Buffer::empty(area);
        let component = SharedLinesProbe {
            lines: Rc::new(RefCell::new(vec!["\x1b[31;1mred\x1b[0m plain".to_string()])),
        };

        RatatuiComponent::new(component).render(area, &mut buffer);

        let red_cell = buffer.cell((0, 0)).expect("red cell");
        assert_eq!(red_cell.symbol(), "r");
        assert_eq!(red_cell.fg, Color::Red);
        assert!(red_cell.modifier.contains(Modifier::BOLD));

        let plain_cell = buffer.cell((4, 0)).expect("plain cell");
        assert_eq!(plain_cell.symbol(), "p");
        assert_eq!(plain_cell.fg, Color::Reset);
        assert!(!plain_cell.modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn ratatui_tui_focusable_overlay_receives_input_and_restores_previous_focus() {
        let base_events = Rc::new(RefCell::new(Vec::new()));
        let overlay_events = Rc::new(RefCell::new(Vec::new()));
        let mut tui = RatatuiTui::new();
        let base_focus = tui.add_focusable_child(InputProbe {
            events: base_events.clone(),
        });
        tui.set_focus(Some(base_focus));

        let overlay = tui.show_focusable_overlay(
            InputProbe {
                events: overlay_events.clone(),
            },
            OverlayOptions {
                width: Some(super::SizeValue::Cells(8)),
                anchor: Some(OverlayAnchor::TopLeft),
                ..OverlayOptions::default()
            },
        );

        assert!(overlay.is_focused(&tui));
        tui.handle_input("overlay");
        assert_eq!(overlay_events.borrow().as_slice(), ["overlay"]);
        assert!(base_events.borrow().is_empty());

        overlay.set_hidden(&mut tui, true);
        assert!(!overlay.is_focused(&tui));
        assert_eq!(tui.focused(), Some(base_focus));
        tui.handle_input("base");
        assert_eq!(base_events.borrow().as_slice(), ["base"]);

        overlay.set_hidden(&mut tui, false);
        assert!(overlay.is_focused(&tui));
        overlay.unfocus(&mut tui);
        assert!(!overlay.is_focused(&tui));
        assert_eq!(tui.focused(), Some(base_focus));
    }

    #[test]
    fn ratatui_tui_non_capturing_overlay_does_not_steal_focus_until_explicitly_focused() {
        let base_events = Rc::new(RefCell::new(Vec::new()));
        let overlay_events = Rc::new(RefCell::new(Vec::new()));
        let mut tui = RatatuiTui::new();
        let base_focus = tui.add_focusable_child(InputProbe {
            events: base_events.clone(),
        });
        tui.set_focus(Some(base_focus));

        let overlay = tui.show_focusable_overlay(
            InputProbe {
                events: overlay_events.clone(),
            },
            OverlayOptions {
                non_capturing: true,
                ..OverlayOptions::default()
            },
        );

        assert!(!overlay.is_focused(&tui));
        assert_eq!(tui.focused(), Some(base_focus));
        tui.handle_input("base");
        assert_eq!(base_events.borrow().as_slice(), ["base"]);
        assert!(overlay_events.borrow().is_empty());

        assert!(overlay.focus(&mut tui));
        assert!(overlay.is_focused(&tui));
        tui.handle_input("overlay");
        assert_eq!(overlay_events.borrow().as_slice(), ["overlay"]);
    }

    #[test]
    fn ratatui_tui_hide_overlay_restores_focus_after_popping_top_overlay() {
        let base_events = Rc::new(RefCell::new(Vec::new()));
        let overlay_events = Rc::new(RefCell::new(Vec::new()));
        let mut tui = RatatuiTui::new();
        let base_focus = tui.add_focusable_child(InputProbe {
            events: base_events.clone(),
        });
        tui.set_focus(Some(base_focus));

        let overlay = tui.show_focusable_overlay(
            InputProbe {
                events: overlay_events.clone(),
            },
            OverlayOptions::default(),
        );

        assert!(overlay.is_focused(&tui));
        assert_eq!(tui.hide_overlay(), Some(()));

        assert_eq!(tui.focused(), Some(base_focus));
        assert!(!overlay.is_present(&tui));
        tui.handle_input("base");
        assert_eq!(base_events.borrow().as_slice(), ["base"]);
        assert!(overlay_events.borrow().is_empty());
    }

    #[test]
    fn ratatui_tui_hidden_focused_overlay_restores_focus_to_top_visible_capturing_overlay() {
        let base_events = Rc::new(RefCell::new(Vec::new()));
        let first_overlay_events = Rc::new(RefCell::new(Vec::new()));
        let second_overlay_events = Rc::new(RefCell::new(Vec::new()));
        let mut tui = RatatuiTui::new();
        let base_focus = tui.add_focusable_child(InputProbe {
            events: base_events.clone(),
        });
        tui.set_focus(Some(base_focus));

        let first_overlay = tui.show_focusable_overlay(
            InputProbe {
                events: first_overlay_events.clone(),
            },
            OverlayOptions::default(),
        );
        let second_overlay = tui.show_focusable_overlay(
            InputProbe {
                events: second_overlay_events.clone(),
            },
            OverlayOptions::default(),
        );

        assert!(second_overlay.is_focused(&tui));
        assert!(first_overlay.focus(&mut tui));
        assert!(first_overlay.is_focused(&tui));
        first_overlay.set_hidden(&mut tui, true);

        assert!(!first_overlay.is_focused(&tui));
        assert!(second_overlay.is_focused(&tui));
        tui.handle_input("second");
        assert!(base_events.borrow().is_empty());
        assert!(first_overlay_events.borrow().is_empty());
        assert_eq!(second_overlay_events.borrow().as_slice(), ["second"]);
    }

    #[test]
    fn ratatui_tui_invalidate_propagates_to_children_focusable_children_and_overlays() {
        let child_invalidations = Rc::new(RefCell::new(0));
        let focus_invalidations = Rc::new(RefCell::new(0));
        let overlay_invalidations = Rc::new(RefCell::new(0));
        let mut tui = RatatuiTui::new();
        tui.add_child(SharedInvalidationProbe {
            invalidations: child_invalidations.clone(),
        });
        tui.add_focusable_child(SharedInvalidationProbe {
            invalidations: focus_invalidations.clone(),
        });
        tui.show_focusable_overlay(
            SharedInvalidationProbe {
                invalidations: overlay_invalidations.clone(),
            },
            OverlayOptions::default(),
        );

        tui.invalidate();

        assert_eq!(*child_invalidations.borrow(), 1);
        assert_eq!(*focus_invalidations.borrow(), 1);
        assert_eq!(*overlay_invalidations.borrow(), 1);
    }

    #[test]
    fn ratatui_tui_consumes_cell_size_response_updates_dimensions_and_invalidates() {
        let events = Rc::new(RefCell::new(Vec::new()));
        let invalidations = Rc::new(RefCell::new(0));
        let mut tui = RatatuiTui::new();
        let focus = tui.add_focusable_child(InputProbe {
            events: events.clone(),
        });
        tui.add_child(SharedInvalidationProbe {
            invalidations: invalidations.clone(),
        });
        tui.set_focus(Some(focus));
        super::set_cell_dimensions(super::CellDimensions {
            width_px: 1,
            height_px: 1,
        });

        assert!(!tui.handle_input("\x1b[6;24;12t"));

        assert_eq!(
            super::get_cell_dimensions(),
            super::CellDimensions {
                width_px: 12,
                height_px: 24,
            }
        );
        assert_eq!(*invalidations.borrow(), 1);
        assert!(events.borrow().is_empty());
    }
}

#[cfg(test)]
mod tui_state_tests {
    use super::{
        apply_line_resets, collect_kitty_image_ids, components::CURSOR_MARKER, composite_line_at,
        composite_overlays, extract_cursor_position, extract_kitty_image_ids,
        resolve_overlay_layout, visible_width, CursorPosition, OverlayAnchor, OverlayMargin,
        OverlayOptions, RenderedOverlay, SizeValue,
    };

    #[test]
    fn extract_cursor_position_strips_marker_and_reports_visible_column() {
        let mut lines = vec![
            "plain".to_string(),
            format!("\x1b[31m红{CURSOR_MARKER}\x1b[0m cursor"),
        ];

        let position = extract_cursor_position(&mut lines, 10);

        assert_eq!(position, Some(CursorPosition { row: 1, col: 2 }));
        assert_eq!(lines[1], "\x1b[31m红\x1b[0m cursor");
    }

    #[test]
    fn extract_cursor_position_scans_only_visible_viewport_from_bottom() {
        let mut lines = vec![
            format!("hidden {CURSOR_MARKER}"),
            format!("visible top {CURSOR_MARKER}"),
            format!("visible bottom {CURSOR_MARKER}"),
        ];

        let position = extract_cursor_position(&mut lines, 2);

        assert_eq!(position, Some(CursorPosition { row: 2, col: 15 }));
        assert!(lines[0].contains(CURSOR_MARKER));
        assert!(lines[1].contains(CURSOR_MARKER));
        assert!(!lines[2].contains(CURSOR_MARKER));
    }

    #[test]
    fn resolve_overlay_layout_matches_pi_defaults_and_anchor_positioning() {
        let layout = resolve_overlay_layout(None, 3, 80, 24);

        assert_eq!(layout.width, 80);
        assert_eq!(layout.row, 10);
        assert_eq!(layout.col, 0);
        assert_eq!(layout.max_height, None);

        let layout = resolve_overlay_layout(
            Some(&OverlayOptions {
                anchor: Some(OverlayAnchor::BottomRight),
                width: Some(SizeValue::Cells(10)),
                ..OverlayOptions::default()
            }),
            1,
            80,
            24,
        );

        assert_eq!(layout.width, 10);
        assert_eq!(layout.row, 23);
        assert_eq!(layout.col, 70);
    }

    #[test]
    fn resolve_overlay_layout_applies_percent_sizes_margins_and_offsets_like_pi() {
        let layout = resolve_overlay_layout(
            Some(&OverlayOptions {
                width: Some(SizeValue::Percent(50.0)),
                min_width: Some(30),
                max_height: Some(SizeValue::Percent(25.0)),
                row: Some(SizeValue::Percent(50.0)),
                col: Some(SizeValue::Percent(100.0)),
                offset_y: Some(1),
                offset_x: Some(-2),
                margin: Some(OverlayMargin::Each {
                    top: 2,
                    right: 4,
                    bottom: 2,
                    left: 4,
                }),
                ..OverlayOptions::default()
            }),
            10,
            100,
            40,
        );

        assert_eq!(layout.width, 50);
        assert_eq!(layout.max_height, Some(10));
        assert_eq!(layout.row, 16);
        assert_eq!(layout.col, 44);
    }

    #[test]
    fn composite_line_at_replaces_visible_columns_and_pads_to_terminal_width() {
        let line = composite_line_at("0123456789", "XY", 3, 4, 10);

        assert_eq!(line, "012\x1b[0mXY  \x1b[0m789\x1b[0m");
        assert_eq!(visible_width(&line), 10);
    }

    #[test]
    fn composite_line_at_truncates_overlay_wide_chars_at_declared_boundary() {
        let line = composite_line_at("abcdefghij", "中文Z", 2, 3, 10);

        assert_eq!(line, "ab\x1b[0m中 \x1b[0mfghij\x1b[0m");
        assert_eq!(visible_width(&line), 10);
    }

    #[test]
    fn composite_line_at_resets_style_when_base_trailing_reset_is_beyond_visible_width_like_pi() {
        let base_line = format!("\x1b[3m{}\x1b[23m", "X".repeat(20));

        let line = composite_line_at(&base_line, "OVR", 5, 3, 20);

        assert_eq!(visible_width(&line), 20);
        assert!(
            line.ends_with("\x1b[0m"),
            "composited line must reset styles before the next terminal row"
        );
    }

    #[test]
    fn composite_line_at_keeps_terminal_image_lines_unchanged() {
        let image_line = "\x1b_Ga=T,f=100;abc\x1b\\";

        assert_eq!(composite_line_at(image_line, "OVER", 2, 4, 20), image_line);
    }

    #[test]
    fn composite_overlays_pads_short_content_to_terminal_height_and_places_overlay() {
        let lines = composite_overlays(
            vec!["base".to_string()],
            &[RenderedOverlay {
                lines: vec!["OVER".to_string()],
                options: OverlayOptions {
                    anchor: Some(OverlayAnchor::BottomRight),
                    width: Some(SizeValue::Cells(6)),
                    ..OverlayOptions::default()
                },
                focus_order: 0,
            }],
            10,
            4,
        );

        assert_eq!(lines.len(), 4);
        assert_eq!(lines[0], "base");
        assert!(lines[3].contains("OVER"));
        assert_eq!(visible_width(&lines[3]), 10);
    }

    #[test]
    fn composite_overlays_applies_max_height_and_focus_order_like_pi() {
        let lines = composite_overlays(
            vec!["..........".to_string()],
            &[
                RenderedOverlay {
                    lines: vec!["AAAA".to_string(), "DROP".to_string()],
                    options: OverlayOptions {
                        width: Some(SizeValue::Cells(4)),
                        max_height: Some(SizeValue::Cells(1)),
                        row: Some(SizeValue::Cells(0)),
                        col: Some(SizeValue::Cells(2)),
                        ..OverlayOptions::default()
                    },
                    focus_order: 10,
                },
                RenderedOverlay {
                    lines: vec!["BB".to_string()],
                    options: OverlayOptions {
                        width: Some(SizeValue::Cells(2)),
                        row: Some(SizeValue::Cells(0)),
                        col: Some(SizeValue::Cells(3)),
                        ..OverlayOptions::default()
                    },
                    focus_order: 20,
                },
            ],
            10,
            1,
        );

        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("A"));
        assert!(lines[0].contains("BB"));
        assert!(!lines[0].contains("DROP"));
        assert_eq!(visible_width(&lines[0]), 10);
    }

    #[test]
    fn apply_line_resets_normalizes_text_lines_and_preserves_image_lines() {
        let image_line = "\x1b_Ga=T,f=100;abc\x1b\\".to_string();
        let lines = apply_line_resets(vec!["ไทยกำ".to_string(), image_line.clone()]);

        assert_eq!(lines[0], "ไทยกํา\x1b[0m\x1b]8;;\x07");
        assert_eq!(lines[1], image_line);
    }

    #[test]
    fn extract_kitty_image_ids_matches_pi_single_line_rules() {
        assert_eq!(
            extract_kitty_image_ids("prefix \x1b_Ga=T,i=42,q=2;abc\x1b\\"),
            vec![42]
        );
        assert_eq!(
            extract_kitty_image_ids("\x1b_Ga=T;abc\x1b\\"),
            Vec::<u32>::new()
        );
        assert_eq!(
            extract_kitty_image_ids("\x1b_Ga=T,i=0;abc\x1b\\"),
            Vec::<u32>::new()
        );
        assert_eq!(
            extract_kitty_image_ids("\x1b_Ga=T,i=4294967296;abc\x1b\\"),
            Vec::<u32>::new()
        );
        assert_eq!(
            extract_kitty_image_ids("\x1b_Ga=T,i=7;one\x1b\\ later \x1b_Ga=T,i=8;two\x1b\\"),
            vec![7]
        );
    }

    #[test]
    fn collect_kitty_image_ids_deduplicates_ids_across_lines() {
        let ids = collect_kitty_image_ids(&[
            "\x1b_Ga=T,i=2;abc\x1b\\".to_string(),
            "plain".to_string(),
            "\x1b_Ga=T,i=2;dup\x1b\\".to_string(),
            "\x1b_Ga=T,i=9;def\x1b\\".to_string(),
        ]);

        assert_eq!(ids, vec![2, 9]);
    }
}

#[cfg(test)]
mod terminal_tests {
    use super::{
        bracketed_paste_sequence, clear_from_cursor_sequence, clear_line_sequence,
        clear_progress_sequence, clear_screen_sequence, disable_kitty_keyboard_protocol_sequence,
        disable_modify_other_keys_sequence, enable_bracketed_paste_sequence,
        enable_kitty_keyboard_protocol_sequence, enable_modify_other_keys_sequence,
        hide_cursor_sequence, is_apple_terminal_session, move_by_sequence,
        normalize_apple_terminal_input, normalize_terminal_input,
        query_kitty_keyboard_protocol_sequence, resolve_terminal_dimensions, set_title_sequence,
        show_cursor_sequence, start_progress_sequence, ProcessTerminal, Terminal,
        TerminalInputContext, APPLE_TERMINAL_SHIFT_ENTER_SEQUENCE, TERMINAL_PROGRESS_KEEPALIVE_MS,
    };

    #[test]
    fn terminal_control_sequences_match_pi_terminal_methods() {
        assert_eq!(move_by_sequence(3), Some("\x1b[3B".to_string()));
        assert_eq!(move_by_sequence(-2), Some("\x1b[2A".to_string()));
        assert_eq!(move_by_sequence(0), None);
        assert_eq!(hide_cursor_sequence(), "\x1b[?25l");
        assert_eq!(show_cursor_sequence(), "\x1b[?25h");
        assert_eq!(clear_line_sequence(), "\x1b[K");
        assert_eq!(clear_from_cursor_sequence(), "\x1b[J");
        assert_eq!(clear_screen_sequence(), "\x1b[2J\x1b[H");
        assert_eq!(set_title_sequence("pi"), "\x1b]0;pi\x07");
        assert_eq!(start_progress_sequence(), "\x1b]9;4;3\x07");
        assert_eq!(clear_progress_sequence(), "\x1b]9;4;0;\x07");
        assert_eq!(TERMINAL_PROGRESS_KEEPALIVE_MS, 1000);
    }

    #[test]
    fn terminal_keyboard_protocol_sequences_match_pi_start_stop() {
        assert_eq!(enable_bracketed_paste_sequence(), "\x1b[?2004h");
        assert_eq!(bracketed_paste_sequence(false), "\x1b[?2004l");
        assert_eq!(bracketed_paste_sequence(true), "\x1b[?2004h");
        assert_eq!(query_kitty_keyboard_protocol_sequence(), "\x1b[?u");
        assert_eq!(enable_kitty_keyboard_protocol_sequence(), "\x1b[>7u");
        assert_eq!(disable_kitty_keyboard_protocol_sequence(), "\x1b[<u");
        assert_eq!(enable_modify_other_keys_sequence(), "\x1b[>4;2m");
        assert_eq!(disable_modify_other_keys_sequence(), "\x1b[>4;0m");
    }

    #[test]
    fn normalizes_apple_terminal_shift_enter_like_pi() {
        assert_eq!(
            normalize_apple_terminal_input("\r", true, true),
            APPLE_TERMINAL_SHIFT_ENTER_SEQUENCE
        );
        assert_eq!(normalize_apple_terminal_input("\r", true, false), "\r");
        assert_eq!(normalize_apple_terminal_input("\r", false, true), "\r");
        assert_eq!(normalize_apple_terminal_input("x", true, true), "x");
    }

    #[test]
    fn detects_apple_terminal_session_like_pi() {
        assert!(is_apple_terminal_session("darwin", Some("Apple_Terminal")));
        assert!(!is_apple_terminal_session("linux", Some("Apple_Terminal")));
        assert!(!is_apple_terminal_session("darwin", Some("iTerm.app")));
        assert!(!is_apple_terminal_session("darwin", None));
    }

    #[test]
    fn normalizes_terminal_input_from_session_context_like_pi() {
        let apple_shift = TerminalInputContext {
            platform: "darwin",
            term_program: Some("Apple_Terminal"),
            shift_pressed: true,
        };
        assert_eq!(
            normalize_terminal_input("\r", apple_shift),
            APPLE_TERMINAL_SHIFT_ENTER_SEQUENCE
        );

        let apple_without_shift = TerminalInputContext {
            shift_pressed: false,
            ..apple_shift
        };
        assert_eq!(normalize_terminal_input("\r", apple_without_shift), "\r");

        let non_apple_shift = TerminalInputContext {
            platform: "darwin",
            term_program: Some("iTerm.app"),
            shift_pressed: true,
        };
        assert_eq!(normalize_terminal_input("\r", non_apple_shift), "\r");
    }

    #[test]
    fn terminal_dimensions_fall_back_to_env_then_defaults_like_process_terminal() {
        assert_eq!(
            resolve_terminal_dimensions(Some(100), Some(40), Some("123"), Some("45")),
            (100, 40)
        );
        assert_eq!(
            resolve_terminal_dimensions(None, None, Some("123"), Some("45")),
            (123, 45)
        );
        assert_eq!(
            resolve_terminal_dimensions(None, None, Some("bad"), Some("0")),
            (80, 24)
        );
    }

    #[test]
    fn process_terminal_writes_pi_control_sequences_and_tracks_dimensions() {
        let mut terminal = ProcessTerminal::new(Vec::<u8>::new(), 100, 40);

        terminal.start().expect("start");
        terminal.write("hello").expect("write");
        terminal.move_by(-2).expect("move up");
        terminal.hide_cursor().expect("hide cursor");
        terminal.clear_line().expect("clear line");
        terminal.set_title("pi").expect("title");
        terminal.set_progress(true).expect("progress start");
        terminal.set_progress(false).expect("progress clear");
        terminal.stop().expect("stop");

        assert_eq!(terminal.columns(), 100);
        assert_eq!(terminal.rows(), 40);
        assert!(!terminal.kitty_protocol_active());

        let output = String::from_utf8(terminal.into_inner()).expect("utf8 output");
        assert!(output.starts_with(enable_bracketed_paste_sequence()));
        assert!(output.contains(query_kitty_keyboard_protocol_sequence()));
        assert!(output.contains("hello"));
        assert!(output.contains("\x1b[2A"));
        assert!(output.contains(hide_cursor_sequence()));
        assert!(output.contains(clear_line_sequence()));
        assert!(output.contains(&set_title_sequence("pi")));
        assert!(output.contains(start_progress_sequence()));
        assert!(output.contains(clear_progress_sequence()));
        assert!(output.ends_with(show_cursor_sequence()));
    }

    #[test]
    fn process_terminal_stop_does_not_clear_inactive_progress_like_pi() {
        let mut terminal = ProcessTerminal::new(Vec::<u8>::new(), 100, 40);

        terminal.start().expect("start");
        terminal.stop().expect("stop");

        let output = String::from_utf8(terminal.into_inner()).expect("utf8 output");
        assert!(!output.contains(clear_progress_sequence()));
    }

    #[test]
    fn process_terminal_stop_does_not_duplicate_explicit_progress_clear_like_pi() {
        let mut terminal = ProcessTerminal::new(Vec::<u8>::new(), 100, 40);

        terminal.start().expect("start");
        terminal.set_progress(true).expect("progress start");
        terminal.set_progress(false).expect("progress clear");
        terminal.stop().expect("stop");

        let output = String::from_utf8(terminal.into_inner()).expect("utf8 output");
        assert_eq!(output.matches(clear_progress_sequence()).count(), 1);
    }

    #[test]
    fn process_terminal_enables_modify_other_keys_fallback_and_disables_on_stop_like_pi() {
        let mut terminal = ProcessTerminal::new(Vec::<u8>::new(), 100, 40);

        terminal.start().expect("start");
        assert!(terminal
            .enable_modify_other_keys_fallback()
            .expect("enable fallback"));
        assert!(terminal.modify_other_keys_active());
        terminal.stop().expect("stop");
        assert!(!terminal.modify_other_keys_active());

        let output = String::from_utf8(terminal.into_inner()).expect("utf8 output");
        assert!(output.contains(enable_modify_other_keys_sequence()));
        assert!(output.contains(disable_modify_other_keys_sequence()));
    }

    #[test]
    fn process_terminal_does_not_enable_modify_other_keys_when_kitty_is_active_like_pi() {
        let mut terminal = ProcessTerminal::new(Vec::<u8>::new(), 100, 40);
        terminal.start().expect("start");
        terminal.set_kitty_protocol_active(true);

        assert!(!terminal
            .enable_modify_other_keys_fallback()
            .expect("skip fallback"));
        assert!(!terminal.modify_other_keys_active());
        terminal.stop().expect("stop");

        let output = String::from_utf8(terminal.into_inner()).expect("utf8 output");
        assert!(!output.contains(enable_modify_other_keys_sequence()));
        assert!(output.contains(disable_kitty_keyboard_protocol_sequence()));
        assert!(!output.contains(disable_modify_other_keys_sequence()));
    }
}
