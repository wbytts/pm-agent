use crate::components::Component;
use crate::{
    apply_line_resets, collect_kitty_image_ids, composite_overlays, extract_ansi_code,
    extract_cursor_position, is_image_line, is_key_release, matches_key, normalize_terminal_output,
    set_cell_dimensions, visible_width, CellDimensions, CursorPosition, OverlayOptions,
    RenderedOverlay,
};
use ratatui::backend::CrosstermBackend;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::Widget;
use ratatui::{Frame, Terminal};
use std::fmt;
use std::io::{self, Write};

/// 构造基于 crossterm backend 的 ratatui Terminal，供真实终端主循环复用。
pub fn create_ratatui_terminal<W>(writer: W) -> io::Result<Terminal<CrosstermBackend<W>>>
where
    W: Write,
{
    Terminal::new(CrosstermBackend::new(writer))
}

pub trait RatatuiInputComponent: Component {
    fn handle_input(&mut self, data: &str);

    fn wants_key_release(&self) -> bool {
        false
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputListenerResult {
    consume: bool,
    data: Option<String>,
}

impl InputListenerResult {
    pub fn pass() -> Self {
        Self {
            consume: false,
            data: None,
        }
    }

    pub fn consume() -> Self {
        Self {
            consume: true,
            data: None,
        }
    }

    pub fn replace(data: impl Into<String>) -> Self {
        Self {
            consume: false,
            data: Some(data.into()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FocusHandle {
    id: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenderRequest {
    pub force: bool,
}

pub struct RatatuiComponent<C> {
    component: C,
}

impl<C> RatatuiComponent<C> {
    pub fn new(component: C) -> Self {
        Self { component }
    }

    pub fn into_inner(self) -> C {
        self.component
    }
}

impl<C> RatatuiComponent<C>
where
    C: Component,
{
    /// 通过 ratatui 的 Frame 渲染组件，便于接入 Terminal::draw 主循环。
    pub fn render_frame(&mut self, frame: &mut Frame<'_>, area: Rect) {
        frame.render_widget(self, area);
    }
}

impl<C> Widget for RatatuiComponent<C>
where
    C: Component,
{
    fn render(mut self, area: Rect, buffer: &mut Buffer) {
        render_component_to_buffer(&mut self.component, area, buffer);
    }
}

impl<C> Widget for &mut RatatuiComponent<C>
where
    C: Component,
{
    fn render(self, area: Rect, buffer: &mut Buffer) {
        render_component_to_buffer(&mut self.component, area, buffer);
    }
}

pub fn render_component_to_buffer<C>(component: &mut C, area: Rect, buffer: &mut Buffer)
where
    C: Component + ?Sized,
{
    if area.width == 0 || area.height == 0 {
        return;
    }

    for (row, line) in component
        .render(area.width as usize)
        .into_iter()
        .take(area.height as usize)
        .enumerate()
    {
        write_line_to_buffer(
            buffer,
            area.x,
            area.y + row as u16,
            &line,
            area.width as usize,
        );
    }
}

struct RatatuiOverlay {
    component: OverlayComponent,
    options: OverlayOptions,
    visible: Option<Box<dyn Fn(usize, usize) -> bool>>,
    hidden: bool,
    focus_order: usize,
    id: usize,
    pre_focus: Option<FocusTarget>,
}

impl RatatuiOverlay {
    fn is_visible_at(&self, width: usize, height: usize) -> bool {
        if self.hidden {
            return false;
        }
        self.visible
            .as_ref()
            .map(|visible| visible(width, height))
            .unwrap_or(true)
    }
}

enum OverlayComponent {
    Plain(Box<dyn Component>),
    Input(Box<dyn RatatuiInputComponent>),
}

impl OverlayComponent {
    fn render(&mut self, width: usize) -> Vec<String> {
        match self {
            Self::Plain(component) => component.render(width),
            Self::Input(component) => component.render(width),
        }
    }

    fn invalidate(&mut self) {
        match self {
            Self::Plain(component) => component.invalidate(),
            Self::Input(component) => component.invalidate(),
        }
    }

    fn handle_input(&mut self, data: &str) -> bool {
        match self {
            Self::Plain(_) => false,
            Self::Input(component) => {
                if is_key_release(data) && !component.wants_key_release() {
                    return false;
                }
                component.handle_input(data);
                true
            }
        }
    }

    fn is_input(&self) -> bool {
        matches!(self, Self::Input(_))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OverlayHandle {
    id: usize,
}

impl OverlayHandle {
    pub fn hide(self, tui: &mut RatatuiTui) -> bool {
        tui.hide_overlay_by_id(self.id)
    }

    pub fn set_hidden(self, tui: &mut RatatuiTui, hidden: bool) -> bool {
        tui.set_overlay_hidden_by_id(self.id, hidden)
    }

    pub fn is_hidden(self, tui: &RatatuiTui) -> bool {
        tui.overlay_by_id(self.id)
            .map(|overlay| overlay.hidden)
            .unwrap_or(true)
    }

    pub fn is_present(self, tui: &RatatuiTui) -> bool {
        tui.overlay_by_id(self.id).is_some()
    }

    pub fn focus(self, tui: &mut RatatuiTui) -> bool {
        tui.focus_overlay_by_id(self.id)
    }

    pub fn unfocus(self, tui: &mut RatatuiTui) -> bool {
        tui.unfocus_overlay_by_id(self.id)
    }

    pub fn is_focused(self, tui: &RatatuiTui) -> bool {
        tui.focused_target == Some(FocusTarget::Overlay(self.id))
    }
}

pub struct RatatuiTui {
    children: Vec<Box<dyn Component>>,
    input_children: Vec<RatatuiInputChild>,
    overlays: Vec<RatatuiOverlay>,
    input_listeners: Vec<Box<dyn FnMut(&str) -> InputListenerResult>>,
    debug_handler: Option<Box<dyn FnMut()>>,
    focused_component: Option<usize>,
    focused_target: Option<FocusTarget>,
    focus_order_counter: usize,
    next_overlay_id: usize,
    next_component_id: usize,
    last_frame_size: Option<(usize, usize)>,
    previous_lines: Vec<String>,
    previous_kitty_image_ids: Vec<u32>,
    previous_width: isize,
    previous_height: isize,
    cursor_row: usize,
    hardware_cursor_row: usize,
    hardware_cursor_position: Option<CursorPosition>,
    max_lines_rendered: usize,
    previous_viewport_top: usize,
    full_redraw_count: usize,
    clear_on_shrink: bool,
    show_hardware_cursor: bool,
    render_request: Option<RenderRequest>,
    stopped: bool,
}

pub type Tui = RatatuiTui;
pub type TUI = RatatuiTui;

struct RatatuiInputChild {
    component: Box<dyn RatatuiInputComponent>,
    id: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FocusTarget {
    Child(usize),
    Overlay(usize),
}

impl RatatuiTui {
    pub fn new() -> Self {
        Self::new_with_environment(
            std::env::var("PI_HARDWARE_CURSOR").ok().as_deref(),
            std::env::var("PI_CLEAR_ON_SHRINK").ok().as_deref(),
        )
    }

    fn new_with_environment(
        show_hardware_cursor: Option<&str>,
        clear_on_shrink: Option<&str>,
    ) -> Self {
        Self {
            children: Vec::new(),
            input_children: Vec::new(),
            overlays: Vec::new(),
            input_listeners: Vec::new(),
            debug_handler: None,
            focused_component: None,
            focused_target: None,
            focus_order_counter: 0,
            next_overlay_id: 0,
            next_component_id: 0,
            last_frame_size: None,
            previous_lines: Vec::new(),
            previous_kitty_image_ids: Vec::new(),
            previous_width: 0,
            previous_height: 0,
            cursor_row: 0,
            hardware_cursor_row: 0,
            hardware_cursor_position: None,
            max_lines_rendered: 0,
            previous_viewport_top: 0,
            full_redraw_count: 0,
            clear_on_shrink: clear_on_shrink == Some("1"),
            show_hardware_cursor: show_hardware_cursor == Some("1"),
            render_request: None,
            stopped: true,
        }
    }

    #[cfg(test)]
    pub fn new_with_environment_for_test(
        show_hardware_cursor: Option<&str>,
        clear_on_shrink: Option<&str>,
    ) -> Self {
        Self::new_with_environment(show_hardware_cursor, clear_on_shrink)
    }

    pub fn add_child<C>(&mut self, component: C)
    where
        C: Component + 'static,
    {
        self.children.push(Box::new(component));
    }

    pub fn add_focusable_child<C>(&mut self, component: C) -> FocusHandle
    where
        C: RatatuiInputComponent + 'static,
    {
        self.next_component_id += 1;
        let id = self.next_component_id;
        self.input_children.push(RatatuiInputChild {
            component: Box::new(component),
            id,
        });
        FocusHandle { id }
    }

    pub fn clear(&mut self) {
        self.children.clear();
        self.input_children.clear();
        self.focused_component = None;
        self.focused_target = None;
    }

    pub fn invalidate(&mut self) {
        for child in &mut self.children {
            child.invalidate();
        }
        for child in &mut self.input_children {
            child.component.invalidate();
        }
        for overlay in &mut self.overlays {
            overlay.component.invalidate();
        }
    }

    pub fn set_focus(&mut self, focus: Option<FocusHandle>) {
        self.focused_component = focus.and_then(|handle| {
            self.input_children
                .iter()
                .any(|child| child.id == handle.id)
                .then_some(handle.id)
        });
        self.focused_target = self.focused_component.map(FocusTarget::Child);
    }

    pub fn focused(&self) -> Option<FocusHandle> {
        self.focused_component.map(|id| FocusHandle { id })
    }

    pub fn start(&mut self) {
        self.stopped = false;
        self.request_render(false);
    }

    pub fn stop(&mut self) {
        self.stopped = true;
        self.render_request = None;
    }

    pub fn is_stopped(&self) -> bool {
        self.stopped
    }

    pub fn request_render(&mut self, force: bool) {
        if self.stopped {
            return;
        }
        if force {
            self.reset_render_state_for_full_redraw();
            self.render_request = Some(RenderRequest { force: true });
            return;
        }
        if self.render_request.is_none() {
            self.render_request = Some(RenderRequest { force: false });
        }
    }

    pub fn render_requested(&self) -> bool {
        self.render_request.is_some()
    }

    pub fn take_render_request(&mut self) -> Option<RenderRequest> {
        self.render_request.take()
    }

    pub fn full_redraws(&self) -> usize {
        self.full_redraw_count
    }

    pub fn get_clear_on_shrink(&self) -> bool {
        self.clear_on_shrink
    }

    pub fn set_clear_on_shrink(&mut self, enabled: bool) {
        self.clear_on_shrink = enabled;
    }

    pub fn get_show_hardware_cursor(&self) -> bool {
        self.show_hardware_cursor
    }

    pub fn set_show_hardware_cursor(&mut self, enabled: bool) {
        if self.show_hardware_cursor == enabled {
            return;
        }
        self.show_hardware_cursor = enabled;
        self.request_render(false);
    }

    pub fn add_input_listener<F>(&mut self, listener: F) -> usize
    where
        F: FnMut(&str) -> InputListenerResult + 'static,
    {
        self.input_listeners.push(Box::new(listener));
        self.input_listeners.len() - 1
    }

    pub fn remove_input_listener(&mut self, index: usize) -> bool {
        if index >= self.input_listeners.len() {
            return false;
        }
        drop(self.input_listeners.remove(index));
        true
    }

    pub fn set_debug_handler<F>(&mut self, handler: F)
    where
        F: FnMut() + 'static,
    {
        self.debug_handler = Some(Box::new(handler));
    }

    pub fn clear_debug_handler(&mut self) {
        self.debug_handler = None;
    }

    pub fn handle_input(&mut self, data: &str) -> bool {
        let mut current = data.to_string();
        for listener in &mut self.input_listeners {
            let result = listener(&current);
            if result.consume {
                return false;
            }
            if let Some(data) = result.data {
                current = data;
            }
            if current.is_empty() {
                return false;
            }
        }

        if self.consume_cell_size_response(&current) {
            return false;
        }

        if matches_key(&current, "shift+ctrl+d") {
            if let Some(handler) = &mut self.debug_handler {
                handler();
                return false;
            }
        }

        if let Some(FocusTarget::Overlay(overlay_id)) = self.focused_target {
            let visible = self.overlays.iter().any(|overlay| {
                overlay.id == overlay_id && self.is_overlay_visible_for_input(overlay)
            });
            if !visible {
                self.restore_focus_after_overlay(overlay_id);
            }
        }

        let Some(focused_target) = self.focused_target else {
            return false;
        };
        match focused_target {
            FocusTarget::Child(focused_id) => {
                let Some(child) = self
                    .input_children
                    .iter_mut()
                    .find(|child| child.id == focused_id)
                else {
                    self.focused_component = None;
                    self.focused_target = None;
                    return false;
                };
                if is_key_release(&current) && !child.component.wants_key_release() {
                    return false;
                }
                child.component.handle_input(&current);
                self.request_render(false);
                true
            }
            FocusTarget::Overlay(overlay_id) => {
                let Some(index) = self
                    .overlays
                    .iter()
                    .position(|overlay| overlay.id == overlay_id)
                else {
                    self.restore_focus_after_overlay(overlay_id);
                    return false;
                };
                let overlay = &mut self.overlays[index];
                if !overlay.component.handle_input(&current) {
                    self.restore_focus_after_overlay(overlay_id);
                    return false;
                }
                self.request_render(false);
                true
            }
        }
    }

    fn consume_cell_size_response(&mut self, data: &str) -> bool {
        let Some(body) = data
            .strip_prefix("\x1b[6;")
            .and_then(|value| value.strip_suffix('t'))
        else {
            return false;
        };
        let Some((height, width)) = body.split_once(';') else {
            return false;
        };
        let (Ok(height_px), Ok(width_px)) = (height.parse::<u32>(), width.parse::<u32>()) else {
            return true;
        };
        if height_px == 0 || width_px == 0 {
            return true;
        }

        set_cell_dimensions(CellDimensions {
            width_px,
            height_px,
        });
        self.invalidate();
        self.request_render(false);
        true
    }

    pub fn show_overlay<C>(&mut self, component: C, options: OverlayOptions) -> OverlayHandle
    where
        C: Component + 'static,
    {
        self.show_overlay_entry(
            OverlayComponent::Plain(Box::new(component)),
            options,
            None::<fn(usize, usize) -> bool>,
            None,
        )
    }

    pub fn show_overlay_when<C, F>(
        &mut self,
        component: C,
        options: OverlayOptions,
        visible: F,
    ) -> OverlayHandle
    where
        C: Component + 'static,
        F: Fn(usize, usize) -> bool + 'static,
    {
        self.show_overlay_entry(
            OverlayComponent::Plain(Box::new(component)),
            options,
            Some(visible),
            None,
        )
    }

    fn show_overlay_entry<F>(
        &mut self,
        component: OverlayComponent,
        options: OverlayOptions,
        visible: Option<F>,
        pre_focus: Option<FocusTarget>,
    ) -> OverlayHandle
    where
        F: Fn(usize, usize) -> bool + 'static,
    {
        self.focus_order_counter += 1;
        self.next_overlay_id += 1;
        let id = self.next_overlay_id;
        self.overlays.push(RatatuiOverlay {
            component,
            options,
            visible: visible.map(|visible| Box::new(visible) as Box<dyn Fn(usize, usize) -> bool>),
            hidden: false,
            focus_order: self.focus_order_counter,
            id,
            pre_focus,
        });
        self.request_render(false);
        OverlayHandle { id }
    }

    pub fn show_focusable_overlay<C>(
        &mut self,
        component: C,
        options: OverlayOptions,
    ) -> OverlayHandle
    where
        C: RatatuiInputComponent + 'static,
    {
        self.show_focusable_overlay_entry(
            OverlayComponent::Input(Box::new(component)),
            options,
            None::<fn(usize, usize) -> bool>,
        )
    }

    pub fn show_focusable_overlay_when<C, F>(
        &mut self,
        component: C,
        options: OverlayOptions,
        visible: F,
    ) -> OverlayHandle
    where
        C: RatatuiInputComponent + 'static,
        F: Fn(usize, usize) -> bool + 'static,
    {
        self.show_focusable_overlay_entry(
            OverlayComponent::Input(Box::new(component)),
            options,
            Some(visible),
        )
    }

    fn show_focusable_overlay_entry<F>(
        &mut self,
        component: OverlayComponent,
        options: OverlayOptions,
        visible: Option<F>,
    ) -> OverlayHandle
    where
        F: Fn(usize, usize) -> bool + 'static,
    {
        self.focus_order_counter += 1;
        self.next_overlay_id += 1;
        let id = self.next_overlay_id;
        let pre_focus = self.focused_target;
        let captures_focus = !options.non_capturing;
        self.overlays.push(RatatuiOverlay {
            component,
            options,
            visible: visible.map(|visible| Box::new(visible) as Box<dyn Fn(usize, usize) -> bool>),
            hidden: false,
            focus_order: self.focus_order_counter,
            id,
            pre_focus,
        });
        if captures_focus {
            self.focused_target = Some(FocusTarget::Overlay(id));
            self.focused_component = None;
        }
        self.request_render(false);
        OverlayHandle { id }
    }

    pub fn hide_overlay(&mut self) -> Option<()> {
        let overlay = self.overlays.pop()?;
        if self.focused_target == Some(FocusTarget::Overlay(overlay.id)) {
            self.restore_focus_for_removed_or_hidden_overlay(overlay.id, overlay.pre_focus);
        }
        self.request_render(false);
        Some(())
    }

    pub fn set_overlay_hidden(&mut self, index: usize, hidden: bool) -> bool {
        let Some(overlay) = self.overlays.get_mut(index) else {
            return false;
        };
        if overlay.hidden == hidden {
            return true;
        }
        overlay.hidden = hidden;
        self.request_render(false);
        true
    }

    pub fn hide_overlay_by_id(&mut self, id: usize) -> bool {
        let Some(index) = self.overlays.iter().position(|overlay| overlay.id == id) else {
            return false;
        };
        let overlay = self.overlays.remove(index);
        if self.focused_target == Some(FocusTarget::Overlay(id)) {
            self.restore_focus_for_removed_or_hidden_overlay(id, overlay.pre_focus);
        }
        self.request_render(false);
        true
    }

    pub fn set_overlay_hidden_by_id(&mut self, id: usize, hidden: bool) -> bool {
        let Some(index) = self.overlays.iter().position(|overlay| overlay.id == id) else {
            return false;
        };
        if self.overlays[index].hidden == hidden {
            return true;
        }
        self.overlays[index].hidden = hidden;
        if hidden {
            if self.focused_target == Some(FocusTarget::Overlay(id)) {
                let pre_focus = self.overlays[index].pre_focus;
                self.restore_focus_for_removed_or_hidden_overlay(id, pre_focus);
            }
        } else if self.overlays[index].component.is_input() {
            self.focus_order_counter += 1;
            self.overlays[index].focus_order = self.focus_order_counter;
            self.apply_focus_target(Some(FocusTarget::Overlay(id)));
        }
        self.request_render(false);
        true
    }

    fn overlay_by_id(&self, id: usize) -> Option<&RatatuiOverlay> {
        self.overlays.iter().find(|overlay| overlay.id == id)
    }

    pub fn has_overlay(&self) -> bool {
        self.overlays.iter().any(|overlay| !overlay.hidden)
    }

    fn is_overlay_visible_for_input(&self, overlay: &RatatuiOverlay) -> bool {
        self.last_frame_size
            .map(|(width, height)| overlay.is_visible_at(width, height))
            .unwrap_or(!overlay.hidden)
    }

    pub fn focus_overlay_by_id(&mut self, id: usize) -> bool {
        let Some(index) = self.overlays.iter().position(|overlay| {
            overlay.id == id && !overlay.hidden && overlay.component.is_input()
        }) else {
            return false;
        };
        self.focus_order_counter += 1;
        self.overlays[index].focus_order = self.focus_order_counter;
        self.apply_focus_target(Some(FocusTarget::Overlay(id)));
        self.request_render(false);
        true
    }

    pub fn unfocus_overlay_by_id(&mut self, id: usize) -> bool {
        if self.focused_target != Some(FocusTarget::Overlay(id)) {
            return false;
        }
        let pre_focus = self.overlay_by_id(id).and_then(|overlay| overlay.pre_focus);
        self.restore_focus_for_removed_or_hidden_overlay(id, pre_focus);
        self.request_render(false);
        true
    }

    fn restore_focus_after_overlay(&mut self, id: usize) {
        let pre_focus = self.overlay_by_id(id).and_then(|overlay| overlay.pre_focus);
        self.restore_focus_for_removed_or_hidden_overlay(id, pre_focus);
    }

    fn restore_focus_for_removed_or_hidden_overlay(
        &mut self,
        current_id: usize,
        pre_focus: Option<FocusTarget>,
    ) {
        let fallback = self
            .topmost_visible_capturing_overlay_except(current_id)
            .map(FocusTarget::Overlay)
            .or(pre_focus);
        self.apply_focus_target(fallback);
    }

    fn topmost_visible_capturing_overlay_except(&self, current_id: usize) -> Option<usize> {
        self.overlays
            .iter()
            .rev()
            .find(|overlay| {
                overlay.id != current_id
                    && !overlay.hidden
                    && !overlay.options.non_capturing
                    && overlay.component.is_input()
            })
            .map(|overlay| overlay.id)
    }

    fn apply_focus_target(&mut self, target: Option<FocusTarget>) {
        match target {
            Some(FocusTarget::Child(id))
                if self.input_children.iter().any(|child| child.id == id) =>
            {
                self.focused_component = Some(id);
                self.focused_target = Some(FocusTarget::Child(id));
            }
            Some(FocusTarget::Overlay(id))
                if self.overlays.iter().any(|overlay| {
                    overlay.id == id && !overlay.hidden && overlay.component.is_input()
                }) =>
            {
                self.focused_component = None;
                self.focused_target = Some(FocusTarget::Overlay(id));
            }
            _ => {
                self.focused_component = None;
                self.focused_target = None;
            }
        }
    }

    /// 通过 ratatui 的 Frame 渲染 TUI 树，覆盖基础组件和 overlay 合成路径。
    pub fn render_frame(&mut self, frame: &mut Frame<'_>, area: Rect) {
        frame.render_widget(&mut *self, area);
        if self.show_hardware_cursor {
            self.position_ratatui_cursor(frame, area);
        }
    }

    #[cfg(test)]
    pub fn render_lines_for_test(&mut self, width: usize, height: usize) -> Vec<String> {
        self.render_lines(width, height)
    }

    #[cfg(test)]
    pub fn last_frame_size_for_test(&self) -> Option<(usize, usize)> {
        self.last_frame_size
    }

    #[cfg(test)]
    pub fn render_state_for_test(&self) -> RatatuiRenderState {
        RatatuiRenderState {
            previous_lines: self.previous_lines.clone(),
            previous_kitty_image_ids: self.previous_kitty_image_ids.clone(),
            previous_width: self.previous_width,
            previous_height: self.previous_height,
            cursor_row: self.cursor_row,
            hardware_cursor_row: self.hardware_cursor_row,
            hardware_cursor_position: self.hardware_cursor_position,
            max_lines_rendered: self.max_lines_rendered,
            previous_viewport_top: self.previous_viewport_top,
        }
    }

    #[cfg(test)]
    pub fn render_plan_for_test(&mut self, width: usize, height: usize) -> RatatuiRenderPlan {
        self.render_plan(width, height)
    }

    #[cfg(test)]
    pub fn render_plan_result_for_test(
        &mut self,
        width: usize,
        height: usize,
    ) -> Result<RatatuiRenderPlan, RatatuiRenderError> {
        self.render_plan_result(width, height)
    }

    #[cfg(test)]
    pub fn full_redraws_for_test(&self) -> usize {
        self.full_redraws()
    }

    fn reset_render_state_for_full_redraw(&mut self) {
        self.last_frame_size = None;
        self.previous_lines.clear();
        self.previous_kitty_image_ids.clear();
        self.previous_width = -1;
        self.previous_height = -1;
        self.cursor_row = 0;
        self.hardware_cursor_row = 0;
        self.hardware_cursor_position = None;
        self.max_lines_rendered = 0;
        self.previous_viewport_top = 0;
    }

    fn render_lines(&mut self, width: usize, height: usize) -> Vec<String> {
        let current = self.render_current_lines(width, height);
        let rendered = current.lines;
        self.commit_render_state(
            width,
            height,
            &rendered,
            current.hardware_cursor_position,
            None,
            false,
        );
        rendered
    }

    pub fn render_plan(&mut self, width: usize, height: usize) -> RatatuiRenderPlan {
        self.render_plan_result(width, height)
            .expect("render plan failed")
    }

    pub fn render_plan_result(
        &mut self,
        width: usize,
        height: usize,
    ) -> Result<RatatuiRenderPlan, RatatuiRenderError> {
        let current = self.render_current_lines(width, height);
        let rendered = current.lines;
        let width_changed = self.previous_width != 0 && self.previous_width != width as isize;
        let height_changed = self.previous_height != 0 && self.previous_height != height as isize;
        let previous_viewport_top = if height_changed {
            let previous_buffer_length = if self.previous_height > 0 {
                self.previous_viewport_top
                    .saturating_add(self.previous_height as usize)
            } else {
                height
            };
            previous_buffer_length.saturating_sub(height)
        } else {
            self.previous_viewport_top
        };
        let change_range = changed_line_range(&self.previous_lines, &rendered);
        let first_changed = change_range.first;
        let render_end = change_range
            .last
            .zip(rendered.len().checked_sub(1))
            .map(|(last_changed, last_line)| last_changed.min(last_line));
        let should_clear_on_shrink = self.clear_on_shrink
            && rendered.len() < self.max_lines_rendered
            && self.overlays.is_empty();

        let kind = if self.previous_lines.is_empty() && !width_changed && !height_changed {
            self.full_redraw_count += 1;
            RatatuiRenderPlanKind::Full { clear: false }
        } else if width_changed || height_changed {
            self.full_redraw_count += 1;
            RatatuiRenderPlanKind::Full { clear: true }
        } else if should_clear_on_shrink {
            self.full_redraw_count += 1;
            RatatuiRenderPlanKind::Full { clear: true }
        } else if first_changed.is_none() {
            RatatuiRenderPlanKind::Unchanged
        } else if first_changed.is_some_and(|first| first >= rendered.len()) {
            let target_row = rendered.len().saturating_sub(1);
            let extra_lines = self.previous_lines.len().saturating_sub(rendered.len());
            if target_row < previous_viewport_top || extra_lines > height {
                self.full_redraw_count += 1;
                RatatuiRenderPlanKind::Full { clear: true }
            } else {
                RatatuiRenderPlanKind::DeletedLines
            }
        } else if first_changed.is_some_and(|first| first < previous_viewport_top) {
            self.full_redraw_count += 1;
            RatatuiRenderPlanKind::Full { clear: true }
        } else {
            RatatuiRenderPlanKind::Differential
        };

        let final_cursor_row =
            final_cursor_row_for_plan(&kind, render_end, self.previous_lines.len(), rendered.len());
        let hardware_cursor_position = current.hardware_cursor_position;
        let hardware_cursor_row = hardware_cursor_position
            .map(|position| position.row)
            .or(final_cursor_row);
        validate_changed_lines_fit_width(&rendered, width, change_range.first, render_end)?;

        let reset_max_lines_rendered = matches!(kind, RatatuiRenderPlanKind::Full { clear: true });
        self.commit_render_state(
            width,
            height,
            &rendered,
            hardware_cursor_position,
            hardware_cursor_row,
            reset_max_lines_rendered,
        );

        Ok(RatatuiRenderPlan {
            kind,
            first_changed: change_range.first,
            last_changed: change_range.last,
            append_start: change_range.append_start,
            render_end,
            final_cursor_row,
            hardware_cursor_row,
            hardware_cursor_position,
            lines: rendered,
        })
    }

    fn render_current_lines(&mut self, width: usize, height: usize) -> CurrentRender {
        let mut lines = Vec::new();
        self.last_frame_size = Some((width, height));
        for child in &mut self.children {
            lines.extend(child.render(width));
        }
        for child in &mut self.input_children {
            lines.extend(child.component.render(width));
        }

        let overlays = self
            .overlays
            .iter_mut()
            .filter(|overlay| overlay.is_visible_at(width, height))
            .map(|overlay| {
                let overlay_width =
                    crate::resolve_overlay_layout(Some(&overlay.options), 0, width, height).width;
                RenderedOverlay {
                    lines: overlay.component.render(overlay_width),
                    options: overlay.options.clone(),
                    focus_order: overlay.focus_order,
                }
            })
            .collect::<Vec<_>>();

        let mut rendered = composite_overlays(lines, &overlays, width, height);
        let cursor_position = extract_cursor_position(&mut rendered, height);
        rendered = apply_line_resets(rendered);

        CurrentRender {
            lines: rendered,
            hardware_cursor_position: cursor_position,
        }
    }

    fn commit_render_state(
        &mut self,
        width: usize,
        height: usize,
        rendered: &[String],
        hardware_cursor_position: Option<CursorPosition>,
        hardware_cursor_row: Option<usize>,
        reset_max_lines_rendered: bool,
    ) {
        self.previous_lines = rendered.to_vec();
        self.previous_kitty_image_ids = collect_kitty_image_ids(rendered);
        self.previous_width = width as isize;
        self.previous_height = height as isize;
        self.cursor_row = rendered.len().saturating_sub(1);
        self.hardware_cursor_row = hardware_cursor_row.unwrap_or(self.cursor_row);
        self.hardware_cursor_position = hardware_cursor_position;
        self.max_lines_rendered = if reset_max_lines_rendered {
            rendered.len()
        } else {
            self.max_lines_rendered.max(rendered.len())
        };
        self.previous_viewport_top = height.max(rendered.len()).saturating_sub(height);
    }

    fn position_ratatui_cursor(&self, frame: &mut Frame<'_>, area: Rect) {
        let Some(position) = self.hardware_cursor_position else {
            return;
        };
        let viewport_top = self.previous_viewport_top;
        let viewport_bottom = viewport_top.saturating_add(area.height as usize);
        if position.row < viewport_top || position.row >= viewport_bottom {
            return;
        }

        let visible_row = position.row - viewport_top;
        let x_offset = position.col.min(u16::MAX as usize) as u16;
        let y_offset = visible_row.min(u16::MAX as usize) as u16;
        frame.set_cursor_position((
            area.x.saturating_add(x_offset),
            area.y.saturating_add(y_offset),
        ));
    }
}

struct CurrentRender {
    lines: Vec<String>,
    hardware_cursor_position: Option<CursorPosition>,
}

fn changed_line_range(previous: &[String], current: &[String]) -> ChangedLineRange {
    let max_lines = previous.len().max(current.len());
    let mut first = None;
    let mut last = None;

    for index in 0..max_lines {
        let previous_line = previous.get(index).map(String::as_str).unwrap_or("");
        let current_line = current.get(index).map(String::as_str).unwrap_or("");
        if previous_line != current_line {
            first.get_or_insert(index);
            last = Some(index);
        }
    }

    let appended_lines = current.len() > previous.len();
    if appended_lines {
        if first.is_none() {
            first = Some(previous.len());
        }
        last = current.len().checked_sub(1);
    }
    let append_start = appended_lines && first == Some(previous.len()) && first.unwrap_or(0) > 0;

    ChangedLineRange {
        first,
        last,
        append_start,
    }
}

fn final_cursor_row_for_plan(
    kind: &RatatuiRenderPlanKind,
    render_end: Option<usize>,
    previous_len: usize,
    current_len: usize,
) -> Option<usize> {
    match kind {
        RatatuiRenderPlanKind::Differential => {
            let mut final_cursor_row = render_end?;
            if previous_len > current_len && final_cursor_row < current_len.saturating_sub(1) {
                final_cursor_row = current_len.saturating_sub(1);
            }
            Some(final_cursor_row)
        }
        RatatuiRenderPlanKind::DeletedLines => Some(current_len.saturating_sub(1)),
        RatatuiRenderPlanKind::Full { .. } => Some(current_len.saturating_sub(1)),
        RatatuiRenderPlanKind::Unchanged => None,
    }
}

fn validate_changed_lines_fit_width(
    lines: &[String],
    terminal_width: usize,
    first_changed: Option<usize>,
    render_end: Option<usize>,
) -> Result<(), RatatuiRenderError> {
    let Some(first_changed) = first_changed else {
        return Ok(());
    };
    let Some(render_end) = render_end else {
        return Ok(());
    };

    for line_index in first_changed..=render_end {
        let Some(line) = lines.get(line_index) else {
            continue;
        };
        if is_image_line(line) {
            continue;
        }
        let line_width = visible_width(line);
        if line_width > terminal_width {
            return Err(RatatuiRenderError {
                line_index,
                line_width,
                terminal_width,
            });
        }
    }

    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ChangedLineRange {
    first: Option<usize>,
    last: Option<usize>,
    append_start: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RatatuiRenderPlanKind {
    Full { clear: bool },
    Differential,
    DeletedLines,
    Unchanged,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RatatuiRenderPlan {
    pub kind: RatatuiRenderPlanKind,
    pub first_changed: Option<usize>,
    pub last_changed: Option<usize>,
    pub append_start: bool,
    pub render_end: Option<usize>,
    pub final_cursor_row: Option<usize>,
    pub hardware_cursor_row: Option<usize>,
    pub hardware_cursor_position: Option<CursorPosition>,
    pub lines: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RatatuiRenderError {
    pub line_index: usize,
    pub line_width: usize,
    pub terminal_width: usize,
}

impl fmt::Display for RatatuiRenderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "Rendered line {} exceeds terminal width ({} > {}). Use visible_width() to measure and truncate_to_width() to truncate lines.",
            self.line_index, self.line_width, self.terminal_width
        )
    }
}

impl std::error::Error for RatatuiRenderError {}

#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RatatuiRenderState {
    pub previous_lines: Vec<String>,
    pub previous_kitty_image_ids: Vec<u32>,
    pub previous_width: isize,
    pub previous_height: isize,
    pub cursor_row: usize,
    pub hardware_cursor_row: usize,
    pub hardware_cursor_position: Option<CursorPosition>,
    pub max_lines_rendered: usize,
    pub previous_viewport_top: usize,
}

impl Default for RatatuiTui {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for &mut RatatuiTui {
    fn render(self, area: Rect, buffer: &mut Buffer) {
        if area.width == 0 || area.height == 0 {
            return;
        }

        for (row, line) in self
            .render_lines(area.width as usize, area.height as usize)
            .into_iter()
            .take(area.height as usize)
            .enumerate()
        {
            write_line_to_buffer(
                buffer,
                area.x,
                area.y + row as u16,
                &line,
                area.width as usize,
            );
        }
    }
}

fn write_line_to_buffer(buffer: &mut Buffer, x: u16, y: u16, line: &str, width: usize) {
    let normalized = normalize_terminal_output(line);
    let mut style = Style::default();
    let mut segment = String::new();
    let mut current_x = x;
    let mut index = 0;

    while index < normalized.len() && current_x.saturating_sub(x) as usize <= width {
        if let Some(ansi) = extract_ansi_code(&normalized, index) {
            write_styled_segment(buffer, &mut current_x, x, y, &segment, width, style);
            segment.clear();
            apply_sgr_to_style(&ansi.code, &mut style);
            index += ansi.length;
            continue;
        }

        let Some(ch) = normalized[index..].chars().next() else {
            break;
        };
        segment.push(ch);
        index += ch.len_utf8();
    }

    write_styled_segment(buffer, &mut current_x, x, y, &segment, width, style);
}

fn write_styled_segment(
    buffer: &mut Buffer,
    current_x: &mut u16,
    start_x: u16,
    y: u16,
    segment: &str,
    width: usize,
    style: Style,
) {
    if segment.is_empty() {
        return;
    }

    let used_width = current_x.saturating_sub(start_x) as usize;
    let Some(remaining_width) = width.checked_sub(used_width) else {
        return;
    };
    if remaining_width == 0 {
        return;
    }

    let (next_x, _) = buffer.set_stringn(*current_x, y, segment, remaining_width, style);
    *current_x = next_x;
}

fn apply_sgr_to_style(sequence: &str, style: &mut Style) {
    let Some(params) = sequence
        .strip_prefix("\x1b[")
        .and_then(|value| value.strip_suffix('m'))
    else {
        return;
    };

    let parts = params.split(';').collect::<Vec<_>>();
    let mut index = 0;
    while index < parts.len() {
        let Some(code) = parse_sgr_number(parts[index]) else {
            index += 1;
            continue;
        };

        match code {
            0 => *style = Style::reset(),
            1 => *style = (*style).add_modifier(Modifier::BOLD),
            2 => *style = (*style).add_modifier(Modifier::DIM),
            3 => *style = (*style).add_modifier(Modifier::ITALIC),
            4 => *style = (*style).add_modifier(Modifier::UNDERLINED),
            5 => *style = (*style).add_modifier(Modifier::SLOW_BLINK),
            6 => *style = (*style).add_modifier(Modifier::RAPID_BLINK),
            7 => *style = (*style).add_modifier(Modifier::REVERSED),
            8 => *style = (*style).add_modifier(Modifier::HIDDEN),
            9 => *style = (*style).add_modifier(Modifier::CROSSED_OUT),
            22 => *style = (*style).remove_modifier(Modifier::BOLD | Modifier::DIM),
            23 => *style = (*style).remove_modifier(Modifier::ITALIC),
            24 => *style = (*style).remove_modifier(Modifier::UNDERLINED),
            25 => *style = (*style).remove_modifier(Modifier::SLOW_BLINK | Modifier::RAPID_BLINK),
            27 => *style = (*style).remove_modifier(Modifier::REVERSED),
            28 => *style = (*style).remove_modifier(Modifier::HIDDEN),
            29 => *style = (*style).remove_modifier(Modifier::CROSSED_OUT),
            30..=37 => *style = (*style).fg(ansi_color(code - 30, false)),
            39 => *style = (*style).fg(Color::Reset),
            40..=47 => *style = (*style).bg(ansi_color(code - 40, false)),
            49 => *style = (*style).bg(Color::Reset),
            90..=97 => *style = (*style).fg(ansi_color(code - 90, true)),
            100..=107 => *style = (*style).bg(ansi_color(code - 100, true)),
            38 | 48 => {
                if let Some((color, consumed)) = parse_extended_color(&parts, index + 1) {
                    if code == 38 {
                        *style = (*style).fg(color);
                    } else {
                        *style = (*style).bg(color);
                    }
                    index += consumed;
                }
            }
            _ => {}
        }

        index += 1;
    }
}

fn parse_sgr_number(value: &str) -> Option<u16> {
    if value.is_empty() {
        Some(0)
    } else {
        value.parse().ok()
    }
}

fn parse_extended_color(parts: &[&str], start: usize) -> Option<(Color, usize)> {
    match parse_sgr_number(*parts.get(start)?)? {
        5 => {
            let index = parse_sgr_number(*parts.get(start + 1)?)?;
            let index = u8::try_from(index).ok()?;
            Some((Color::Indexed(index), 2))
        }
        2 => {
            let red = u8::try_from(parse_sgr_number(*parts.get(start + 1)?)?).ok()?;
            let green = u8::try_from(parse_sgr_number(*parts.get(start + 2)?)?).ok()?;
            let blue = u8::try_from(parse_sgr_number(*parts.get(start + 3)?)?).ok()?;
            Some((Color::Rgb(red, green, blue), 4))
        }
        _ => None,
    }
}

fn ansi_color(index: u16, bright: bool) -> Color {
    match (bright, index) {
        (false, 0) => Color::Black,
        (false, 1) => Color::Red,
        (false, 2) => Color::Green,
        (false, 3) => Color::Yellow,
        (false, 4) => Color::Blue,
        (false, 5) => Color::Magenta,
        (false, 6) => Color::Cyan,
        (false, 7) => Color::Gray,
        (true, 0) => Color::DarkGray,
        (true, 1) => Color::LightRed,
        (true, 2) => Color::LightGreen,
        (true, 3) => Color::LightYellow,
        (true, 4) => Color::LightBlue,
        (true, 5) => Color::LightMagenta,
        (true, 6) => Color::LightCyan,
        (true, 7) => Color::White,
        _ => Color::Reset,
    }
}
