use super::dynamic_border::BorderStyleFn;
use super::loader::{ColorFn, LoaderIndicatorOptions};
use super::{CancellableLoader, Component, DynamicBorder, Loader, Spacer, Text};
use crate::KeybindingsManager;

pub struct BorderedLoaderOptions {
    pub cancellable: bool,
    pub cancel_hint: Option<String>,
}

impl Default for BorderedLoaderOptions {
    fn default() -> Self {
        Self {
            cancellable: true,
            cancel_hint: None,
        }
    }
}

enum InnerLoader {
    Cancellable(CancellableLoader),
    Plain(Loader),
}

pub struct BorderedLoader {
    border_style: BorderStyleFn,
    loader: InnerLoader,
    cancel_hint: Option<String>,
}

impl BorderedLoader {
    pub fn new(
        spinner_color_fn: ColorFn,
        message_color_fn: ColorFn,
        message: impl Into<String>,
        options: BorderedLoaderOptions,
    ) -> Self {
        Self::with_indicator(
            spinner_color_fn,
            message_color_fn,
            message,
            options,
            None,
            None,
        )
    }

    pub fn with_indicator(
        spinner_color_fn: ColorFn,
        message_color_fn: ColorFn,
        message: impl Into<String>,
        options: BorderedLoaderOptions,
        indicator: Option<LoaderIndicatorOptions>,
        border_style: Option<BorderStyleFn>,
    ) -> Self {
        let loader = if options.cancellable {
            InnerLoader::Cancellable(CancellableLoader::new(
                spinner_color_fn,
                message_color_fn,
                message,
                indicator,
            ))
        } else {
            InnerLoader::Plain(Loader::new(
                spinner_color_fn,
                message_color_fn,
                message,
                indicator,
            ))
        };

        Self {
            border_style: border_style.unwrap_or_else(|| std::sync::Arc::new(str::to_string)),
            loader,
            cancel_hint: options.cancellable.then_some(options.cancel_hint).flatten(),
        }
    }

    pub fn handle_input(&mut self, data: &str, keybindings: &KeybindingsManager) {
        if let InnerLoader::Cancellable(loader) = &mut self.loader {
            loader.handle_input(data, keybindings);
        }
    }

    pub fn aborted(&self) -> bool {
        match &self.loader {
            InnerLoader::Cancellable(loader) => loader.aborted(),
            InnerLoader::Plain(_) => false,
        }
    }

    pub fn dispose(&mut self) {
        match &mut self.loader {
            InnerLoader::Cancellable(loader) => loader.dispose(),
            InnerLoader::Plain(loader) => loader.stop(),
        }
    }
}

impl Component for BorderedLoader {
    fn render(&mut self, width: usize) -> Vec<String> {
        let mut lines = Vec::new();
        lines.extend(DynamicBorder::new(self.border_style.clone()).render(width));
        lines.extend(match &mut self.loader {
            InnerLoader::Cancellable(loader) => loader.render(width),
            InnerLoader::Plain(loader) => loader.render(width),
        });
        if let Some(cancel_hint) = &self.cancel_hint {
            lines.extend(Spacer::new(1).render(width));
            lines.extend(Text::new(cancel_hint, 1, 0).render(width));
        }
        lines.extend(Spacer::new(1).render(width));
        lines.extend(DynamicBorder::new(self.border_style.clone()).render(width));
        lines
    }

    fn invalidate(&mut self) {
        match &mut self.loader {
            InnerLoader::Cancellable(loader) => loader.invalidate(),
            InnerLoader::Plain(loader) => loader.invalidate(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{BorderedLoader, BorderedLoaderOptions};
    use crate::components::Component;
    use std::sync::Arc;

    #[test]
    fn bordered_loader_renders_borders_loader_and_cancel_hint_when_cancellable() {
        let identity = Arc::new(str::to_string);
        let mut loader = BorderedLoader::new(
            identity.clone(),
            identity,
            "Working",
            BorderedLoaderOptions {
                cancellable: true,
                cancel_hint: Some("escape cancel".to_string()),
            },
        );

        let lines = loader.render(24);

        assert_eq!(
            lines.first().map(String::as_str),
            Some("────────────────────────")
        );
        assert!(lines.iter().any(|line| line.contains("Working")));
        assert!(lines.iter().any(|line| line.contains("escape cancel")));
        assert_eq!(
            lines.last().map(String::as_str),
            Some("────────────────────────")
        );
    }

    #[test]
    fn bordered_loader_omits_cancel_hint_when_not_cancellable() {
        let identity = Arc::new(str::to_string);
        let mut loader = BorderedLoader::new(
            identity.clone(),
            identity,
            "Creating gist...",
            BorderedLoaderOptions {
                cancellable: false,
                cancel_hint: Some("escape cancel".to_string()),
            },
        );

        let lines = loader.render(24);

        assert!(lines.iter().any(|line| line.contains("Creating gist...")));
        assert!(!lines.iter().any(|line| line.contains("escape cancel")));
        assert_eq!(
            lines.first().map(String::as_str),
            Some("────────────────────────")
        );
        assert_eq!(
            lines.last().map(String::as_str),
            Some("────────────────────────")
        );
    }

    #[test]
    fn bordered_loader_forwards_cancel_input_only_when_cancellable() {
        let identity = Arc::new(str::to_string);
        let mut cancellable = BorderedLoader::new(
            identity.clone(),
            identity.clone(),
            "Working",
            BorderedLoaderOptions {
                cancellable: true,
                cancel_hint: None,
            },
        );
        let mut plain = BorderedLoader::new(
            identity.clone(),
            identity,
            "Working",
            BorderedLoaderOptions {
                cancellable: false,
                cancel_hint: None,
            },
        );

        cancellable.handle_input("\x1b", &crate::KeybindingsManager::default());
        plain.handle_input("\x1b", &crate::KeybindingsManager::default());

        assert!(cancellable.aborted());
        assert!(!plain.aborted());
    }
}
