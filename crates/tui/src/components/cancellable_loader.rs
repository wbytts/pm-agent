use super::loader::{ColorFn, LoaderIndicatorOptions};
use super::{Component, Loader};
use crate::KeybindingsManager;

pub struct CancellableLoader {
    loader: Loader,
    aborted: bool,
    on_abort: Option<Box<dyn FnMut() + Send>>,
}

impl CancellableLoader {
    pub fn new(
        spinner_color_fn: ColorFn,
        message_color_fn: ColorFn,
        message: impl Into<String>,
        indicator: Option<LoaderIndicatorOptions>,
    ) -> Self {
        Self {
            loader: Loader::new(spinner_color_fn, message_color_fn, message, indicator),
            aborted: false,
            on_abort: None,
        }
    }

    pub fn set_on_abort<F>(&mut self, on_abort: F)
    where
        F: FnMut() + Send + 'static,
    {
        self.on_abort = Some(Box::new(on_abort));
    }

    pub fn aborted(&self) -> bool {
        self.aborted
    }

    pub fn handle_input(&mut self, data: &str, keybindings: &KeybindingsManager) {
        if keybindings.matches(data, "tui.select.cancel") {
            self.aborted = true;
            if let Some(on_abort) = self.on_abort.as_mut() {
                on_abort();
            }
        }
    }

    pub fn dispose(&mut self) {
        self.loader.stop();
    }
}

impl Component for CancellableLoader {
    fn render(&mut self, width: usize) -> Vec<String> {
        self.loader.render(width)
    }

    fn invalidate(&mut self) {
        self.loader.invalidate();
    }
}
