#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImageDimensions {
    pub width_px: u32,
    pub height_px: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResizedImageDimensions {
    pub original_width_px: u32,
    pub original_height_px: u32,
    pub width_px: u32,
    pub height_px: u32,
    pub was_resized: bool,
}

pub fn detect_image_dimensions(bytes: &[u8], mime_type: &str) -> Option<ImageDimensions> {
    match mime_type {
        "image/png" => get_png_dimensions(bytes),
        "image/jpeg" => get_jpeg_dimensions(bytes),
        "image/gif" => get_gif_dimensions(bytes),
        "image/webp" => get_webp_dimensions(bytes),
        _ => None,
    }
}

pub fn format_image_dimensions_note(dimensions: ImageDimensions) -> String {
    format!(
        "[Image dimensions: {}x{}]",
        dimensions.width_px, dimensions.height_px
    )
}

pub fn format_resized_image_dimension_note(dimensions: ResizedImageDimensions) -> Option<String> {
    if !dimensions.was_resized {
        return None;
    }

    let scale = dimensions.original_width_px as f64 / dimensions.width_px as f64;
    Some(format!(
        "[Image: original {}x{}, displayed at {}x{}. Multiply coordinates by {:.2} to map to original image.]",
        dimensions.original_width_px,
        dimensions.original_height_px,
        dimensions.width_px,
        dimensions.height_px,
        scale,
    ))
}

pub fn get_png_dimensions(bytes: &[u8]) -> Option<ImageDimensions> {
    if bytes.len() < 24 || bytes.get(0..4)? != [0x89, b'P', b'N', b'G'] {
        return None;
    }
    Some(ImageDimensions {
        width_px: u32::from_be_bytes(bytes.get(16..20)?.try_into().ok()?),
        height_px: u32::from_be_bytes(bytes.get(20..24)?.try_into().ok()?),
    })
}

pub fn get_jpeg_dimensions(bytes: &[u8]) -> Option<ImageDimensions> {
    if bytes.len() < 2 || bytes[0] != 0xff || bytes[1] != 0xd8 {
        return None;
    }
    let mut offset = 2;
    while offset < bytes.len().saturating_sub(9) {
        if bytes[offset] != 0xff {
            offset += 1;
            continue;
        }
        let marker = bytes[offset + 1];
        if (0xc0..=0xc2).contains(&marker) {
            return Some(ImageDimensions {
                height_px: u16::from_be_bytes(bytes.get(offset + 5..offset + 7)?.try_into().ok()?)
                    as u32,
                width_px: u16::from_be_bytes(bytes.get(offset + 7..offset + 9)?.try_into().ok()?)
                    as u32,
            });
        }
        let length =
            u16::from_be_bytes(bytes.get(offset + 2..offset + 4)?.try_into().ok()?) as usize;
        if length < 2 {
            return None;
        }
        offset += 2 + length;
    }
    None
}

pub fn get_gif_dimensions(bytes: &[u8]) -> Option<ImageDimensions> {
    if bytes.len() < 10 || !matches!(bytes.get(0..6)?, b"GIF87a" | b"GIF89a") {
        return None;
    }
    Some(ImageDimensions {
        width_px: u16::from_le_bytes(bytes.get(6..8)?.try_into().ok()?) as u32,
        height_px: u16::from_le_bytes(bytes.get(8..10)?.try_into().ok()?) as u32,
    })
}

pub fn get_webp_dimensions(bytes: &[u8]) -> Option<ImageDimensions> {
    if bytes.len() < 30 || bytes.get(0..4)? != b"RIFF" || bytes.get(8..12)? != b"WEBP" {
        return None;
    }
    match bytes.get(12..16)? {
        b"VP8 " => Some(ImageDimensions {
            width_px: (u16::from_le_bytes(bytes.get(26..28)?.try_into().ok()?) & 0x3fff) as u32,
            height_px: (u16::from_le_bytes(bytes.get(28..30)?.try_into().ok()?) & 0x3fff) as u32,
        }),
        b"VP8L" => {
            let bits = u32::from_le_bytes(bytes.get(21..25)?.try_into().ok()?);
            Some(ImageDimensions {
                width_px: (bits & 0x3fff) + 1,
                height_px: ((bits >> 14) & 0x3fff) + 1,
            })
        }
        b"VP8X" => Some(ImageDimensions {
            width_px: (bytes[24] as u32 | ((bytes[25] as u32) << 8) | ((bytes[26] as u32) << 16))
                + 1,
            height_px: (bytes[27] as u32 | ((bytes[28] as u32) << 8) | ((bytes[29] as u32) << 16))
                + 1,
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_png_dimensions_from_ihdr() {
        let mut png = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
        png.extend_from_slice(&13u32.to_be_bytes());
        png.extend_from_slice(b"IHDR");
        png.extend_from_slice(&640u32.to_be_bytes());
        png.extend_from_slice(&480u32.to_be_bytes());

        assert_eq!(
            get_png_dimensions(&png),
            Some(ImageDimensions {
                width_px: 640,
                height_px: 480
            })
        );
    }

    #[test]
    fn formats_dimension_note() {
        assert_eq!(
            format_image_dimensions_note(ImageDimensions {
                width_px: 2,
                height_px: 3
            }),
            "[Image dimensions: 2x3]"
        );
    }

    #[test]
    fn formats_resized_image_dimension_note_like_pi() {
        assert_eq!(
            format_resized_image_dimension_note(ResizedImageDimensions {
                original_width_px: 100,
                original_height_px: 100,
                width_px: 100,
                height_px: 100,
                was_resized: false,
            }),
            None
        );

        assert_eq!(
            format_resized_image_dimension_note(ResizedImageDimensions {
                original_width_px: 2000,
                original_height_px: 1000,
                width_px: 1000,
                height_px: 500,
                was_resized: true,
            }),
            Some("[Image: original 2000x1000, displayed at 1000x500. Multiply coordinates by 2.00 to map to original image.]".to_string())
        );
    }
}
