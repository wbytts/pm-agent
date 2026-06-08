pub fn get_exif_orientation(bytes: &[u8]) -> u16 {
    let tiff_offset = if bytes.len() >= 2 && bytes[0] == 0xff && bytes[1] == 0xd8 {
        find_jpeg_tiff_offset(bytes)
    } else if bytes.len() >= 12
        && bytes.get(0..4) == Some(b"RIFF")
        && bytes.get(8..12) == Some(b"WEBP")
    {
        find_webp_tiff_offset(bytes)
    } else {
        None
    };

    tiff_offset
        .map(|offset| read_orientation_from_tiff(bytes, offset))
        .unwrap_or(1)
}

fn read_orientation_from_tiff(bytes: &[u8], tiff_start: usize) -> u16 {
    if tiff_start + 8 > bytes.len() {
        return 1;
    }

    let little_endian = match bytes.get(tiff_start..tiff_start + 2) {
        Some(b"II") => true,
        Some(b"MM") => false,
        _ => return 1,
    };

    let ifd_offset = read_u32(bytes, tiff_start + 4, little_endian) as usize;
    let Some(ifd_start) = tiff_start.checked_add(ifd_offset) else {
        return 1;
    };
    if ifd_start + 2 > bytes.len() {
        return 1;
    }

    let entry_count = read_u16(bytes, ifd_start, little_endian) as usize;
    for index in 0..entry_count {
        let Some(entry_pos) = ifd_start
            .checked_add(2)
            .and_then(|start| start.checked_add(index * 12))
        else {
            return 1;
        };
        if entry_pos + 12 > bytes.len() {
            return 1;
        }

        if read_u16(bytes, entry_pos, little_endian) == 0x0112 {
            let value = read_u16(bytes, entry_pos + 8, little_endian);
            return (1..=8).contains(&value).then_some(value).unwrap_or(1);
        }
    }

    1
}

fn find_jpeg_tiff_offset(bytes: &[u8]) -> Option<usize> {
    let mut offset = 2usize;
    while offset < bytes.len().saturating_sub(1) {
        if bytes[offset] != 0xff {
            return None;
        }
        let marker = bytes[offset + 1];
        if marker == 0xff {
            offset += 1;
            continue;
        }

        if marker == 0xe1 {
            if offset + 4 >= bytes.len() {
                return None;
            }
            let segment_start = offset + 4;
            if segment_start + 6 > bytes.len() || !has_exif_header(bytes, segment_start) {
                return None;
            }
            return Some(segment_start + 6);
        }

        if offset + 4 > bytes.len() {
            return None;
        }
        let length = u16::from_be_bytes([bytes[offset + 2], bytes[offset + 3]]) as usize;
        offset = offset.saturating_add(2 + length);
    }

    None
}

fn find_webp_tiff_offset(bytes: &[u8]) -> Option<usize> {
    let mut offset = 12usize;
    while offset + 8 <= bytes.len() {
        let chunk_id = bytes.get(offset..offset + 4)?;
        let chunk_size = read_u32(bytes, offset + 4, true) as usize;
        let data_start = offset + 8;

        if chunk_id == b"EXIF" {
            if data_start + chunk_size > bytes.len() {
                return None;
            }
            let tiff_start = if chunk_size >= 6 && has_exif_header(bytes, data_start) {
                data_start + 6
            } else {
                data_start
            };
            return Some(tiff_start);
        }

        offset = data_start
            .checked_add(chunk_size)?
            .checked_add(chunk_size % 2)?;
    }

    None
}

fn has_exif_header(bytes: &[u8], offset: usize) -> bool {
    bytes.get(offset..offset + 6) == Some(b"Exif\0\0")
}

fn read_u16(bytes: &[u8], offset: usize, little_endian: bool) -> u16 {
    let Some(slice) = bytes.get(offset..offset + 2) else {
        return 0;
    };
    if little_endian {
        u16::from_le_bytes([slice[0], slice[1]])
    } else {
        u16::from_be_bytes([slice[0], slice[1]])
    }
}

fn read_u32(bytes: &[u8], offset: usize, little_endian: bool) -> u32 {
    let Some(slice) = bytes.get(offset..offset + 4) else {
        return 0;
    };
    if little_endian {
        u32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]])
    } else {
        u32::from_be_bytes([slice[0], slice[1], slice[2], slice[3]])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn jpeg_with_exif_orientation(orientation: u16, little_endian: bool) -> Vec<u8> {
        let mut tiff = Vec::new();
        if little_endian {
            tiff.extend_from_slice(b"II");
            tiff.extend_from_slice(&42u16.to_le_bytes());
            tiff.extend_from_slice(&8u32.to_le_bytes());
            tiff.extend_from_slice(&1u16.to_le_bytes());
            tiff.extend_from_slice(&0x0112u16.to_le_bytes());
            tiff.extend_from_slice(&3u16.to_le_bytes());
            tiff.extend_from_slice(&1u32.to_le_bytes());
            tiff.extend_from_slice(&orientation.to_le_bytes());
            tiff.extend_from_slice(&[0, 0]);
            tiff.extend_from_slice(&0u32.to_le_bytes());
        } else {
            tiff.extend_from_slice(b"MM");
            tiff.extend_from_slice(&42u16.to_be_bytes());
            tiff.extend_from_slice(&8u32.to_be_bytes());
            tiff.extend_from_slice(&1u16.to_be_bytes());
            tiff.extend_from_slice(&0x0112u16.to_be_bytes());
            tiff.extend_from_slice(&3u16.to_be_bytes());
            tiff.extend_from_slice(&1u32.to_be_bytes());
            tiff.extend_from_slice(&orientation.to_be_bytes());
            tiff.extend_from_slice(&[0, 0]);
            tiff.extend_from_slice(&0u32.to_be_bytes());
        }

        let segment_length = (2 + b"Exif\0\0".len() + tiff.len()) as u16;
        let mut jpeg = vec![0xff, 0xd8, 0xff, 0xe1];
        jpeg.extend_from_slice(&segment_length.to_be_bytes());
        jpeg.extend_from_slice(b"Exif\0\0");
        jpeg.extend_from_slice(&tiff);
        jpeg.extend_from_slice(&[0xff, 0xd9]);
        jpeg
    }

    fn webp_with_exif_orientation(orientation: u16, with_exif_prefix: bool) -> Vec<u8> {
        let mut tiff = Vec::new();
        tiff.extend_from_slice(b"II");
        tiff.extend_from_slice(&42u16.to_le_bytes());
        tiff.extend_from_slice(&8u32.to_le_bytes());
        tiff.extend_from_slice(&1u16.to_le_bytes());
        tiff.extend_from_slice(&0x0112u16.to_le_bytes());
        tiff.extend_from_slice(&3u16.to_le_bytes());
        tiff.extend_from_slice(&1u32.to_le_bytes());
        tiff.extend_from_slice(&orientation.to_le_bytes());
        tiff.extend_from_slice(&[0, 0]);
        tiff.extend_from_slice(&0u32.to_le_bytes());

        let mut exif_data = Vec::new();
        if with_exif_prefix {
            exif_data.extend_from_slice(b"Exif\0\0");
        }
        exif_data.extend_from_slice(&tiff);

        let mut webp = Vec::new();
        webp.extend_from_slice(b"RIFF");
        let riff_size = 4 + 8 + exif_data.len() + (exif_data.len() % 2);
        webp.extend_from_slice(&(riff_size as u32).to_le_bytes());
        webp.extend_from_slice(b"WEBP");
        webp.extend_from_slice(b"EXIF");
        webp.extend_from_slice(&(exif_data.len() as u32).to_le_bytes());
        webp.extend_from_slice(&exif_data);
        if exif_data.len() % 2 == 1 {
            webp.push(0);
        }
        webp
    }

    #[test]
    fn reads_jpeg_exif_orientation_like_pi() {
        assert_eq!(
            get_exif_orientation(&jpeg_with_exif_orientation(6, true)),
            6
        );
        assert_eq!(
            get_exif_orientation(&jpeg_with_exif_orientation(8, false)),
            8
        );
    }

    #[test]
    fn reads_webp_exif_orientation_like_pi() {
        assert_eq!(
            get_exif_orientation(&webp_with_exif_orientation(3, true)),
            3
        );
        assert_eq!(
            get_exif_orientation(&webp_with_exif_orientation(5, false)),
            5
        );
    }

    #[test]
    fn defaults_to_one_for_missing_or_invalid_orientation_like_pi() {
        assert_eq!(get_exif_orientation(b"not an image"), 1);
        assert_eq!(
            get_exif_orientation(&jpeg_with_exif_orientation(9, true)),
            1
        );
    }
}
