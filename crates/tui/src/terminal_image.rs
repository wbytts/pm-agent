use std::env;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, OnceLock};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageProtocol {
    Kitty,
    ITerm2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TerminalCapabilities {
    pub images: Option<ImageProtocol>,
    pub true_color: bool,
    pub hyperlinks: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CellDimensions {
    pub width_px: u32,
    pub height_px: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImageDimensions {
    pub width_px: u32,
    pub height_px: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImageRenderOptions {
    pub max_width_cells: Option<u32>,
    pub max_height_cells: Option<u32>,
    pub preserve_aspect_ratio: bool,
    pub image_id: Option<u32>,
    pub move_cursor: bool,
}

impl Default for ImageRenderOptions {
    fn default() -> Self {
        Self {
            max_width_cells: None,
            max_height_cells: None,
            preserve_aspect_ratio: true,
            image_id: None,
            move_cursor: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImageCellSize {
    pub columns: u32,
    pub rows: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageRenderResult {
    pub sequence: String,
    pub rows: u32,
    pub image_id: Option<u32>,
}

static CAPABILITIES: OnceLock<Mutex<Option<TerminalCapabilities>>> = OnceLock::new();
static CELL_DIMENSIONS: OnceLock<Mutex<CellDimensions>> = OnceLock::new();
static NEXT_IMAGE_ID: AtomicU32 = AtomicU32::new(1);
#[cfg(test)]
pub(crate) static CAPABILITIES_TEST_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

const KITTY_PREFIX: &str = "\x1b_G";
const ITERM2_PREFIX: &str = "\x1b]1337;File=";

pub fn get_cell_dimensions() -> CellDimensions {
    *cell_dimensions_store()
        .lock()
        .expect("lock terminal cell dimensions")
}

pub fn set_cell_dimensions(dimensions: CellDimensions) {
    *cell_dimensions_store()
        .lock()
        .expect("lock terminal cell dimensions") = dimensions;
}

pub fn detect_capabilities() -> TerminalCapabilities {
    let term_program = env::var("TERM_PROGRAM").unwrap_or_default().to_lowercase();
    let term = env::var("TERM").unwrap_or_default().to_lowercase();
    let color_term = env::var("COLORTERM").unwrap_or_default().to_lowercase();
    let has_true_color_hint = color_term == "truecolor" || color_term == "24bit";

    let in_tmux_or_screen =
        env::var("TMUX").is_ok() || term.starts_with("tmux") || term.starts_with("screen");
    if in_tmux_or_screen {
        return TerminalCapabilities {
            images: None,
            true_color: has_true_color_hint,
            hyperlinks: false,
        };
    }

    if env::var("KITTY_WINDOW_ID").is_ok() || term_program == "kitty" {
        return TerminalCapabilities {
            images: Some(ImageProtocol::Kitty),
            true_color: true,
            hyperlinks: true,
        };
    }
    if term_program == "ghostty"
        || term.contains("ghostty")
        || env::var("GHOSTTY_RESOURCES_DIR").is_ok()
    {
        return TerminalCapabilities {
            images: Some(ImageProtocol::Kitty),
            true_color: true,
            hyperlinks: true,
        };
    }
    if env::var("WEZTERM_PANE").is_ok() || term_program == "wezterm" {
        return TerminalCapabilities {
            images: Some(ImageProtocol::Kitty),
            true_color: true,
            hyperlinks: true,
        };
    }
    if env::var("ITERM_SESSION_ID").is_ok() || term_program == "iterm.app" {
        return TerminalCapabilities {
            images: Some(ImageProtocol::ITerm2),
            true_color: true,
            hyperlinks: true,
        };
    }
    if term_program == "vscode" || term_program == "alacritty" {
        return TerminalCapabilities {
            images: None,
            true_color: true,
            hyperlinks: true,
        };
    }

    TerminalCapabilities {
        images: None,
        true_color: has_true_color_hint || env::var("WT_SESSION").is_ok(),
        hyperlinks: false,
    }
}

pub fn get_capabilities() -> TerminalCapabilities {
    let mut cached = capabilities_store()
        .lock()
        .expect("lock terminal capabilities");
    if cached.is_none() {
        *cached = Some(detect_capabilities());
    }
    cached.expect("terminal capabilities initialized")
}

pub fn reset_capabilities_cache() {
    *capabilities_store()
        .lock()
        .expect("lock terminal capabilities") = None;
}

pub fn set_capabilities(capabilities: TerminalCapabilities) {
    *capabilities_store()
        .lock()
        .expect("lock terminal capabilities") = Some(capabilities);
}

pub fn is_image_line(line: &str) -> bool {
    line.starts_with(KITTY_PREFIX)
        || line.starts_with(ITERM2_PREFIX)
        || line.contains(KITTY_PREFIX)
        || line.contains(ITERM2_PREFIX)
}

pub fn allocate_image_id() -> u32 {
    NEXT_IMAGE_ID.fetch_add(1, Ordering::Relaxed).max(1)
}

pub fn encode_kitty(
    base64_data: &str,
    columns: Option<u32>,
    rows: Option<u32>,
    image_id: Option<u32>,
    move_cursor: bool,
) -> String {
    const CHUNK_SIZE: usize = 4096;

    let mut params = vec!["a=T".to_string(), "f=100".to_string(), "q=2".to_string()];
    if !move_cursor {
        params.push("C=1".to_string());
    }
    if let Some(columns) = columns {
        params.push(format!("c={columns}"));
    }
    if let Some(rows) = rows {
        params.push(format!("r={rows}"));
    }
    if let Some(image_id) = image_id.filter(|id| *id > 0) {
        params.push(format!("i={image_id}"));
    }

    if base64_data.len() <= CHUNK_SIZE {
        return format!("\x1b_G{};{}\x1b\\", params.join(","), base64_data);
    }

    let mut chunks = Vec::new();
    let mut offset = 0;
    let mut is_first = true;
    while offset < base64_data.len() {
        let end = (offset + CHUNK_SIZE).min(base64_data.len());
        let chunk = &base64_data[offset..end];
        let is_last = end >= base64_data.len();
        if is_first {
            chunks.push(format!("\x1b_G{},m=1;{}\x1b\\", params.join(","), chunk));
            is_first = false;
        } else if is_last {
            chunks.push(format!("\x1b_Gm=0;{chunk}\x1b\\"));
        } else {
            chunks.push(format!("\x1b_Gm=1;{chunk}\x1b\\"));
        }
        offset = end;
    }
    chunks.join("")
}

pub fn delete_kitty_image(image_id: u32) -> String {
    format!("\x1b_Ga=d,d=I,i={image_id},q=2\x1b\\")
}

pub fn delete_all_kitty_images() -> String {
    "\x1b_Ga=d,d=A,q=2\x1b\\".to_string()
}

pub fn encode_iterm2(
    base64_data: &str,
    width: Option<&str>,
    height: Option<&str>,
    name: Option<&str>,
    preserve_aspect_ratio: bool,
    inline: bool,
) -> String {
    let mut params = vec![format!("inline={}", u8::from(inline))];
    if let Some(width) = width {
        params.push(format!("width={width}"));
    }
    if let Some(height) = height {
        params.push(format!("height={height}"));
    }
    if let Some(name) = name {
        params.push(format!("name={}", encode_base64(name.as_bytes())));
    }
    if !preserve_aspect_ratio {
        params.push("preserveAspectRatio=0".to_string());
    }
    format!("\x1b]1337;File={}:{}\x07", params.join(";"), base64_data)
}

pub fn calculate_image_cell_size(
    image_dimensions: ImageDimensions,
    max_width_cells: u32,
    max_height_cells: Option<u32>,
    cell_dimensions: CellDimensions,
) -> ImageCellSize {
    let max_width = max_width_cells.max(1);
    let max_height = max_height_cells.map(|height| height.max(1));
    let image_width = image_dimensions.width_px.max(1) as f64;
    let image_height = image_dimensions.height_px.max(1) as f64;
    let width_scale = (max_width as f64 * cell_dimensions.width_px as f64) / image_width;
    let height_scale = max_height.map_or(width_scale, |height| {
        (height as f64 * cell_dimensions.height_px as f64) / image_height
    });
    let scale = width_scale.min(height_scale);
    let scaled_width_px = image_width * scale;
    let scaled_height_px = image_height * scale;
    let columns = (scaled_width_px / cell_dimensions.width_px as f64).ceil() as u32;
    let rows = (scaled_height_px / cell_dimensions.height_px as f64).ceil() as u32;

    ImageCellSize {
        columns: columns.max(1).min(max_width),
        rows: rows.max(1).min(max_height.unwrap_or(u32::MAX)),
    }
}

pub fn calculate_image_rows(
    image_dimensions: ImageDimensions,
    target_width_cells: u32,
    cell_dimensions: CellDimensions,
) -> u32 {
    calculate_image_cell_size(image_dimensions, target_width_cells, None, cell_dimensions).rows
}

pub fn get_png_dimensions(base64_data: &str) -> Option<ImageDimensions> {
    let buffer = decode_base64(base64_data)?;
    if buffer.len() < 24 || buffer.get(0..4)? != [0x89, b'P', b'N', b'G'] {
        return None;
    }
    Some(ImageDimensions {
        width_px: u32::from_be_bytes(buffer.get(16..20)?.try_into().ok()?),
        height_px: u32::from_be_bytes(buffer.get(20..24)?.try_into().ok()?),
    })
}

pub fn get_jpeg_dimensions(base64_data: &str) -> Option<ImageDimensions> {
    let buffer = decode_base64(base64_data)?;
    if buffer.len() < 2 || buffer[0] != 0xff || buffer[1] != 0xd8 {
        return None;
    }
    let mut offset = 2;
    while offset < buffer.len().saturating_sub(9) {
        if buffer[offset] != 0xff {
            offset += 1;
            continue;
        }
        let marker = buffer[offset + 1];
        if (0xc0..=0xc2).contains(&marker) {
            return Some(ImageDimensions {
                height_px: u16::from_be_bytes(buffer.get(offset + 5..offset + 7)?.try_into().ok()?)
                    as u32,
                width_px: u16::from_be_bytes(buffer.get(offset + 7..offset + 9)?.try_into().ok()?)
                    as u32,
            });
        }
        let length =
            u16::from_be_bytes(buffer.get(offset + 2..offset + 4)?.try_into().ok()?) as usize;
        if length < 2 {
            return None;
        }
        offset += 2 + length;
    }
    None
}

pub fn get_gif_dimensions(base64_data: &str) -> Option<ImageDimensions> {
    let buffer = decode_base64(base64_data)?;
    if buffer.len() < 10 || !matches!(buffer.get(0..6)?, b"GIF87a" | b"GIF89a") {
        return None;
    }
    Some(ImageDimensions {
        width_px: u16::from_le_bytes(buffer.get(6..8)?.try_into().ok()?) as u32,
        height_px: u16::from_le_bytes(buffer.get(8..10)?.try_into().ok()?) as u32,
    })
}

pub fn get_webp_dimensions(base64_data: &str) -> Option<ImageDimensions> {
    let buffer = decode_base64(base64_data)?;
    if buffer.len() < 30 || buffer.get(0..4)? != b"RIFF" || buffer.get(8..12)? != b"WEBP" {
        return None;
    }
    match buffer.get(12..16)? {
        b"VP8 " => Some(ImageDimensions {
            width_px: (u16::from_le_bytes(buffer.get(26..28)?.try_into().ok()?) & 0x3fff) as u32,
            height_px: (u16::from_le_bytes(buffer.get(28..30)?.try_into().ok()?) & 0x3fff) as u32,
        }),
        b"VP8L" => {
            let bits = u32::from_le_bytes(buffer.get(21..25)?.try_into().ok()?);
            Some(ImageDimensions {
                width_px: (bits & 0x3fff) + 1,
                height_px: ((bits >> 14) & 0x3fff) + 1,
            })
        }
        b"VP8X" => Some(ImageDimensions {
            width_px: (buffer[24] as u32
                | ((buffer[25] as u32) << 8)
                | ((buffer[26] as u32) << 16))
                + 1,
            height_px: (buffer[27] as u32
                | ((buffer[28] as u32) << 8)
                | ((buffer[29] as u32) << 16))
                + 1,
        }),
        _ => None,
    }
}

pub fn get_image_dimensions(base64_data: &str, mime_type: &str) -> Option<ImageDimensions> {
    match mime_type {
        "image/png" => get_png_dimensions(base64_data),
        "image/jpeg" => get_jpeg_dimensions(base64_data),
        "image/gif" => get_gif_dimensions(base64_data),
        "image/webp" => get_webp_dimensions(base64_data),
        _ => None,
    }
}

pub fn render_image(
    base64_data: &str,
    image_dimensions: ImageDimensions,
    options: ImageRenderOptions,
) -> Option<ImageRenderResult> {
    let capabilities = get_capabilities();
    let protocol = capabilities.images?;
    let max_width = options.max_width_cells.unwrap_or(80);
    let size = calculate_image_cell_size(
        image_dimensions,
        max_width,
        options.max_height_cells,
        get_cell_dimensions(),
    );

    match protocol {
        ImageProtocol::Kitty => Some(ImageRenderResult {
            sequence: encode_kitty(
                base64_data,
                Some(size.columns),
                Some(size.rows),
                options.image_id,
                options.move_cursor,
            ),
            rows: size.rows,
            image_id: options.image_id,
        }),
        ImageProtocol::ITerm2 => Some(ImageRenderResult {
            sequence: encode_iterm2(
                base64_data,
                Some(&size.columns.to_string()),
                Some("auto"),
                None,
                options.preserve_aspect_ratio,
                true,
            ),
            rows: size.rows,
            image_id: None,
        }),
    }
}

pub fn hyperlink(text: &str, url: &str) -> String {
    format!("\x1b]8;;{url}\x1b\\{text}\x1b]8;;\x1b\\")
}

pub fn image_fallback(
    mime_type: &str,
    dimensions: Option<ImageDimensions>,
    filename: Option<&str>,
) -> String {
    let mut parts = Vec::new();
    if let Some(filename) = filename {
        parts.push(filename.to_string());
    }
    parts.push(format!("[{mime_type}]"));
    if let Some(dimensions) = dimensions {
        parts.push(format!("{}x{}", dimensions.width_px, dimensions.height_px));
    }
    format!("[Image: {}]", parts.join(" "))
}

fn capabilities_store() -> &'static Mutex<Option<TerminalCapabilities>> {
    CAPABILITIES.get_or_init(|| Mutex::new(None))
}

fn cell_dimensions_store() -> &'static Mutex<CellDimensions> {
    CELL_DIMENSIONS.get_or_init(|| {
        Mutex::new(CellDimensions {
            width_px: 9,
            height_px: 18,
        })
    })
}

fn decode_base64(input: &str) -> Option<Vec<u8>> {
    let mut output = Vec::new();
    let mut buffer = 0u32;
    let mut bits = 0u8;

    for byte in input.bytes().filter(|byte| !byte.is_ascii_whitespace()) {
        if byte == b'=' {
            break;
        }
        let value = base64_value(byte)? as u32;
        buffer = (buffer << 6) | value;
        bits += 6;
        while bits >= 8 {
            bits -= 8;
            output.push(((buffer >> bits) & 0xff) as u8);
        }
    }

    Some(output)
}

fn encode_base64(input: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut output = String::new();
    for chunk in input.chunks(3) {
        let b0 = chunk[0];
        let b1 = *chunk.get(1).unwrap_or(&0);
        let b2 = *chunk.get(2).unwrap_or(&0);
        output.push(TABLE[(b0 >> 2) as usize] as char);
        output.push(TABLE[(((b0 & 0x03) << 4) | (b1 >> 4)) as usize] as char);
        if chunk.len() > 1 {
            output.push(TABLE[(((b1 & 0x0f) << 2) | (b2 >> 6)) as usize] as char);
        } else {
            output.push('=');
        }
        if chunk.len() > 2 {
            output.push(TABLE[(b2 & 0x3f) as usize] as char);
        } else {
            output.push('=');
        }
    }
    output
}

fn base64_value(byte: u8) -> Option<u8> {
    match byte {
        b'A'..=b'Z' => Some(byte - b'A'),
        b'a'..=b'z' => Some(byte - b'a' + 26),
        b'0'..=b'9' => Some(byte - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_kitty_and_iterm_sequences() {
        let kitty = encode_kitty("abc", Some(2), Some(3), Some(42), false);
        assert_eq!(kitty, "\x1b_Ga=T,f=100,q=2,C=1,c=2,r=3,i=42;abc\x1b\\");
        assert!(!encode_kitty("abc", None, None, Some(0), true).contains("i=0"));
        let iterm = encode_iterm2("abc", Some("10"), Some("auto"), Some("x.png"), true, true);
        assert!(iterm.starts_with("\x1b]1337;File=inline=1;width=10;height=auto;name=eC5wbmc=:abc"));
    }

    #[test]
    fn calculates_cell_size_and_fallback() {
        let size = calculate_image_cell_size(
            ImageDimensions {
                width_px: 180,
                height_px: 180,
            },
            10,
            None,
            CellDimensions {
                width_px: 9,
                height_px: 18,
            },
        );
        assert_eq!(
            size,
            ImageCellSize {
                columns: 10,
                rows: 5
            }
        );
        assert_eq!(
            image_fallback(
                "image/png",
                Some(ImageDimensions {
                    width_px: 12,
                    height_px: 8
                }),
                Some("a.png")
            ),
            "[Image: a.png [image/png] 12x8]"
        );
    }

    #[test]
    fn reads_png_dimensions() {
        let one_by_one_png = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJ";
        assert_eq!(
            get_png_dimensions(one_by_one_png),
            Some(ImageDimensions {
                width_px: 1,
                height_px: 1
            })
        );
    }

    #[test]
    fn renders_image_for_cached_capabilities() {
        let _guard = CAPABILITIES_TEST_LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .expect("lock capabilities test");
        set_cell_dimensions(CellDimensions {
            width_px: 9,
            height_px: 18,
        });
        set_capabilities(TerminalCapabilities {
            images: Some(ImageProtocol::Kitty),
            true_color: true,
            hyperlinks: true,
        });
        let result = render_image(
            "abc",
            ImageDimensions {
                width_px: 18,
                height_px: 18,
            },
            ImageRenderOptions {
                max_width_cells: Some(4),
                max_height_cells: None,
                preserve_aspect_ratio: true,
                image_id: Some(7),
                move_cursor: false,
            },
        )
        .expect("render image");
        assert_eq!(result.rows, 2);
        assert!(result.sequence.contains("i=7"));
        reset_capabilities_cache();
    }
}
