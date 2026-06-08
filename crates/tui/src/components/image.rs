use super::Component;
use crate::{
    allocate_image_id, get_capabilities, get_cell_dimensions, get_image_dimensions, image_fallback,
    render_image, ImageDimensions, ImageProtocol, ImageRenderOptions,
};
use std::sync::Arc;

pub type ImageFallbackStyleFn = Arc<dyn Fn(&str) -> String + Send + Sync>;

#[derive(Clone)]
pub struct ImageTheme {
    pub fallback_color: ImageFallbackStyleFn,
}

impl Default for ImageTheme {
    fn default() -> Self {
        Self {
            fallback_color: Arc::new(str::to_string),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ImageOptions {
    pub max_width_cells: Option<u32>,
    pub max_height_cells: Option<u32>,
    pub filename: Option<String>,
    pub image_id: Option<u32>,
}

pub struct Image {
    base64_data: String,
    mime_type: String,
    dimensions: ImageDimensions,
    theme: ImageTheme,
    options: ImageOptions,
    image_id: Option<u32>,
    cached_lines: Option<Vec<String>>,
    cached_width: Option<usize>,
}

impl Image {
    pub fn new(
        base64_data: impl Into<String>,
        mime_type: impl Into<String>,
        theme: ImageTheme,
        options: ImageOptions,
        dimensions: Option<ImageDimensions>,
    ) -> Self {
        let base64_data = base64_data.into();
        let mime_type = mime_type.into();
        let dimensions = dimensions
            .or_else(|| get_image_dimensions(&base64_data, &mime_type))
            .unwrap_or(ImageDimensions {
                width_px: 800,
                height_px: 600,
            });
        let image_id = options.image_id;
        Self {
            base64_data,
            mime_type,
            dimensions,
            theme,
            options,
            image_id,
            cached_lines: None,
            cached_width: None,
        }
    }

    pub fn image_id(&self) -> Option<u32> {
        self.image_id
    }
}

impl Component for Image {
    fn render(&mut self, width: usize) -> Vec<String> {
        if self.cached_width == Some(width) {
            if let Some(lines) = &self.cached_lines {
                return lines.clone();
            }
        }

        let max_width = width
            .saturating_sub(2)
            .max(1)
            .min(self.options.max_width_cells.unwrap_or(60) as usize)
            as u32;
        let cell_dimensions = get_cell_dimensions();
        let default_max_height = ((max_width * cell_dimensions.width_px) as f64
            / cell_dimensions.height_px as f64)
            .ceil() as u32;
        let max_height = self
            .options
            .max_height_cells
            .unwrap_or(default_max_height.max(1));
        let capabilities = get_capabilities();

        let lines = if let Some(protocol) = capabilities.images {
            if protocol == ImageProtocol::Kitty && self.image_id.is_none() {
                self.image_id = Some(allocate_image_id());
            }
            let result = render_image(
                &self.base64_data,
                self.dimensions,
                ImageRenderOptions {
                    max_width_cells: Some(max_width),
                    max_height_cells: Some(max_height),
                    preserve_aspect_ratio: true,
                    image_id: self.image_id,
                    move_cursor: false,
                },
            );
            if let Some(result) = result {
                if result.image_id.is_some() {
                    self.image_id = result.image_id;
                }
                if protocol == ImageProtocol::Kitty {
                    let mut lines = vec![result.sequence];
                    lines.extend((1..result.rows).map(|_| String::new()));
                    lines
                } else {
                    let mut lines = Vec::new();
                    lines.extend((1..result.rows).map(|_| String::new()));
                    let row_offset = result.rows.saturating_sub(1);
                    let move_up = if row_offset > 0 {
                        format!("\x1b[{row_offset}A")
                    } else {
                        String::new()
                    };
                    lines.push(format!("{move_up}{}", result.sequence));
                    lines
                }
            } else {
                self.fallback_lines()
            }
        } else {
            self.fallback_lines()
        };

        self.cached_width = Some(width);
        self.cached_lines = Some(lines.clone());
        lines
    }

    fn invalidate(&mut self) {
        self.cached_lines = None;
        self.cached_width = None;
    }
}

impl Image {
    fn fallback_lines(&self) -> Vec<String> {
        vec![(self.theme.fallback_color)(&image_fallback(
            &self.mime_type,
            Some(self.dimensions),
            self.options.filename.as_deref(),
        ))]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{reset_capabilities_cache, set_capabilities, TerminalCapabilities};

    #[test]
    fn image_renders_fallback_without_protocol() {
        let _guard = crate::terminal_image::CAPABILITIES_TEST_LOCK
            .get_or_init(|| std::sync::Mutex::new(()))
            .lock()
            .expect("lock capabilities test");
        set_capabilities(TerminalCapabilities {
            images: None,
            true_color: true,
            hyperlinks: false,
        });
        let mut image = Image::new(
            "abc",
            "image/png",
            ImageTheme::default(),
            ImageOptions {
                filename: Some("a.png".to_string()),
                ..ImageOptions::default()
            },
            Some(ImageDimensions {
                width_px: 10,
                height_px: 20,
            }),
        );

        assert_eq!(image.render(80), vec!["[Image: a.png [image/png] 10x20]"]);
        reset_capabilities_cache();
    }

    #[test]
    fn image_renders_kitty_sequence_and_caches_id() {
        let _guard = crate::terminal_image::CAPABILITIES_TEST_LOCK
            .get_or_init(|| std::sync::Mutex::new(()))
            .lock()
            .expect("lock capabilities test");
        set_capabilities(TerminalCapabilities {
            images: Some(ImageProtocol::Kitty),
            true_color: true,
            hyperlinks: true,
        });
        let mut image = Image::new(
            "abc",
            "image/png",
            ImageTheme::default(),
            ImageOptions::default(),
            Some(ImageDimensions {
                width_px: 18,
                height_px: 18,
            }),
        );

        let lines = image.render(20);
        assert!(lines[0].starts_with("\x1b_G"));
        assert!(image.image_id().is_some());
        assert_eq!(image.render(20), lines);
        reset_capabilities_cache();
    }
}
