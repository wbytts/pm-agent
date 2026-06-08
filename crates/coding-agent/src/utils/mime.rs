use std::fs::File;
use std::io::Read;
use std::path::Path;

const IMAGE_TYPE_SNIFF_BYTES: usize = 4100;
const PNG_SIGNATURE: &[u8] = &[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];

pub fn detect_supported_image_mime_type(buffer: &[u8]) -> Option<&'static str> {
    if starts_with(buffer, &[0xff, 0xd8, 0xff]) {
        return (buffer.get(3) != Some(&0xf7)).then_some("image/jpeg");
    }
    if starts_with(buffer, PNG_SIGNATURE) {
        return (is_png(buffer) && !is_animated_png(buffer)).then_some("image/png");
    }
    if starts_with_ascii(buffer, 0, "GIF") {
        return Some("image/gif");
    }
    if starts_with_ascii(buffer, 0, "RIFF") && starts_with_ascii(buffer, 8, "WEBP") {
        return Some("image/webp");
    }
    None
}

pub fn detect_supported_image_mime_type_from_file(
    path: impl AsRef<Path>,
) -> std::io::Result<Option<&'static str>> {
    let mut file = File::open(path)?;
    let mut buffer = vec![0; IMAGE_TYPE_SNIFF_BYTES];
    let bytes_read = file.read(&mut buffer)?;
    buffer.truncate(bytes_read);
    Ok(detect_supported_image_mime_type(&buffer))
}

fn is_png(buffer: &[u8]) -> bool {
    buffer.len() >= 16
        && read_u32_be(buffer, PNG_SIGNATURE.len()) == 13
        && starts_with_ascii(buffer, 12, "IHDR")
}

fn is_animated_png(buffer: &[u8]) -> bool {
    let mut offset = PNG_SIGNATURE.len();
    while offset + 8 <= buffer.len() {
        let chunk_length = read_u32_be(buffer, offset) as usize;
        let chunk_type_offset = offset + 4;
        if starts_with_ascii(buffer, chunk_type_offset, "acTL") {
            return true;
        }
        if starts_with_ascii(buffer, chunk_type_offset, "IDAT") {
            return false;
        }

        let Some(next_offset) = offset.checked_add(8 + chunk_length + 4) else {
            return false;
        };
        if next_offset <= offset || next_offset > buffer.len() {
            return false;
        }
        offset = next_offset;
    }
    false
}

fn read_u32_be(buffer: &[u8], offset: usize) -> u32 {
    ((buffer.get(offset).copied().unwrap_or(0) as u32) << 24)
        | ((buffer.get(offset + 1).copied().unwrap_or(0) as u32) << 16)
        | ((buffer.get(offset + 2).copied().unwrap_or(0) as u32) << 8)
        | buffer.get(offset + 3).copied().unwrap_or(0) as u32
}

fn starts_with(buffer: &[u8], bytes: &[u8]) -> bool {
    buffer.len() >= bytes.len() && buffer.iter().zip(bytes).all(|(left, right)| left == right)
}

fn starts_with_ascii(buffer: &[u8], offset: usize, text: &str) -> bool {
    buffer
        .get(offset..offset + text.len())
        .is_some_and(|slice| slice == text.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_common_supported_image_types() {
        assert_eq!(
            detect_supported_image_mime_type(&[0xff, 0xd8, 0xff, 0xe0]),
            Some("image/jpeg")
        );
        assert_eq!(
            detect_supported_image_mime_type(b"GIF89a"),
            Some("image/gif")
        );
        assert_eq!(
            detect_supported_image_mime_type(b"RIFFxxxxWEBP"),
            Some("image/webp")
        );
    }

    #[test]
    fn detects_static_png_and_rejects_apng() {
        let mut png = PNG_SIGNATURE.to_vec();
        png.extend_from_slice(&13u32.to_be_bytes());
        png.extend_from_slice(b"IHDR");
        png.extend_from_slice(&[0; 17]);
        png.extend_from_slice(&1u32.to_be_bytes());
        png.extend_from_slice(b"IDAT");
        assert_eq!(detect_supported_image_mime_type(&png), Some("image/png"));

        let mut apng = PNG_SIGNATURE.to_vec();
        apng.extend_from_slice(&13u32.to_be_bytes());
        apng.extend_from_slice(b"IHDR");
        apng.extend_from_slice(&[0; 17]);
        apng.extend_from_slice(&1u32.to_be_bytes());
        apng.extend_from_slice(b"acTL");
        assert_eq!(detect_supported_image_mime_type(&apng), None);
    }
}
