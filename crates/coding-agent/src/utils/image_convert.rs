#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConvertedPngImage {
    pub data: String,
    pub mime_type: String,
}

pub fn convert_to_png(
    base64_data: &str,
    mime_type: &str,
) -> Result<Option<ConvertedPngImage>, String> {
    if mime_type == "image/png" {
        return Ok(Some(ConvertedPngImage {
            data: base64_data.to_string(),
            mime_type: mime_type.to_string(),
        }));
    }

    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::base64::encode_base64;

    #[test]
    fn convert_to_png_passes_png_through_like_pi_image_convert() {
        let png = b"png bytes";
        let converted = convert_to_png(&encode_base64(png), "image/png")
            .expect("conversion should not fail")
            .expect("png should pass through");

        assert_eq!(converted.data, encode_base64(png));
        assert_eq!(converted.mime_type, "image/png");
    }

    #[test]
    fn convert_to_png_returns_none_for_non_png_without_photon_like_pi_image_convert() {
        assert_eq!(
            convert_to_png("abc", "image/jpeg").expect("conversion should not fail"),
            None
        );
    }
}
