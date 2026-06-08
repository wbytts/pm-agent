use crate::utils::base64::encode_base64;
use crate::utils::image_dimensions::{
    detect_image_dimensions, ResizedImageDimensions, DEFAULT_IMAGE_MAX_DIMENSION,
};

pub const DEFAULT_MAX_INLINE_IMAGE_BASE64_BYTES: usize = 4_718_592;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResizedImage {
    pub data: String,
    pub mime_type: String,
    pub dimensions: ResizedImageDimensions,
}

pub fn resize_image(input_bytes: &[u8], mime_type: &str) -> Option<ResizedImage> {
    let dimensions = detect_image_dimensions(input_bytes, mime_type)?;
    let input_base64_size = input_bytes.len().div_ceil(3) * 4;
    if dimensions.width_px <= DEFAULT_IMAGE_MAX_DIMENSION
        && dimensions.height_px <= DEFAULT_IMAGE_MAX_DIMENSION
        && input_base64_size < DEFAULT_MAX_INLINE_IMAGE_BASE64_BYTES
    {
        return Some(ResizedImage {
            data: encode_base64(input_bytes),
            mime_type: mime_type.to_string(),
            dimensions: ResizedImageDimensions {
                original_width_px: dimensions.width_px,
                original_height_px: dimensions.height_px,
                width_px: dimensions.width_px,
                height_px: dimensions.height_px,
                was_resized: false,
            },
        });
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png_header(width: u32, height: u32) -> Vec<u8> {
        let mut png = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
        png.extend_from_slice(&13u32.to_be_bytes());
        png.extend_from_slice(b"IHDR");
        png.extend_from_slice(&width.to_be_bytes());
        png.extend_from_slice(&height.to_be_bytes());
        png.extend_from_slice(&[8, 2, 0, 0, 0]);
        png
    }

    #[test]
    fn passes_through_images_within_pi_resize_limits() {
        let png = png_header(16, 8);

        let resized = resize_image(&png, "image/png").expect("image should pass through");

        assert_eq!(resized.mime_type, "image/png");
        assert_eq!(resized.data, encode_base64(&png));
        assert_eq!(
            resized.dimensions,
            ResizedImageDimensions {
                original_width_px: 16,
                original_height_px: 8,
                width_px: 16,
                height_px: 8,
                was_resized: false,
            }
        );
    }

    #[test]
    fn returns_none_when_image_would_need_resampling() {
        assert_eq!(resize_image(&png_header(3000, 1000), "image/png"), None);
    }
}
