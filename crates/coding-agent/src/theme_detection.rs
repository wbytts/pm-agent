#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalTheme {
    Dark,
    Light,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalThemeDetectionSource {
    TerminalBackground,
    ColorFgBg,
    Fallback,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalThemeDetectionConfidence {
    High,
    Low,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalThemeDetection {
    pub theme: TerminalTheme,
    pub source: TerminalThemeDetectionSource,
    pub detail: String,
    pub confidence: TerminalThemeDetectionConfidence,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RgbColor {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

pub fn detect_terminal_background_from_colorfgbg(
    colorfgbg: Option<&str>,
) -> TerminalThemeDetection {
    if let Some(index) = colorfgbg.and_then(colorfgbg_background_index) {
        return TerminalThemeDetection {
            theme: if ansi_color_luminance(index) >= 0.5 {
                TerminalTheme::Light
            } else {
                TerminalTheme::Dark
            },
            source: TerminalThemeDetectionSource::ColorFgBg,
            detail: format!("background color index {index}"),
            confidence: TerminalThemeDetectionConfidence::High,
        };
    }

    TerminalThemeDetection {
        theme: TerminalTheme::Dark,
        source: TerminalThemeDetectionSource::Fallback,
        detail: "no terminal background hint found".to_string(),
        confidence: TerminalThemeDetectionConfidence::Low,
    }
}

pub fn parse_osc11_background_color(data: &str) -> Option<RgbColor> {
    let value = data
        .strip_prefix("\x1b]11;")?
        .strip_suffix('\x07')
        .or_else(|| data.strip_prefix("\x1b]11;")?.strip_suffix("\x1b\\"))?
        .trim();

    if let Some(hex) = value.strip_prefix('#') {
        return parse_osc_hex_color(hex);
    }

    let rgb_value = value
        .strip_prefix("rgb:")
        .or_else(|| value.strip_prefix("RGB:"))
        .or_else(|| value.strip_prefix("rgba:"))
        .or_else(|| value.strip_prefix("RGBA:"))
        .unwrap_or(value);
    let mut channels = rgb_value.split('/');
    let r = parse_osc_hex_channel(channels.next()?)?;
    let g = parse_osc_hex_channel(channels.next()?)?;
    let b = parse_osc_hex_channel(channels.next()?)?;
    Some(RgbColor { r, g, b })
}

pub fn get_theme_for_rgb_color(rgb: RgbColor) -> TerminalTheme {
    if rgb_luminance(rgb) >= 0.5 {
        TerminalTheme::Light
    } else {
        TerminalTheme::Dark
    }
}

fn colorfgbg_background_index(colorfgbg: &str) -> Option<u8> {
    colorfgbg
        .split(';')
        .rev()
        .filter_map(|part| part.trim().parse::<u16>().ok())
        .find(|value| *value <= 255)
        .map(|value| value as u8)
}

fn parse_osc_hex_color(hex: &str) -> Option<RgbColor> {
    if hex.len() == 6 && hex.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return Some(RgbColor {
            r: u8::from_str_radix(&hex[0..2], 16).ok()?,
            g: u8::from_str_radix(&hex[2..4], 16).ok()?,
            b: u8::from_str_radix(&hex[4..6], 16).ok()?,
        });
    }
    if hex.len() == 12 && hex.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return Some(RgbColor {
            r: parse_osc_hex_channel(&hex[0..4])?,
            g: parse_osc_hex_channel(&hex[4..8])?,
            b: parse_osc_hex_channel(&hex[8..12])?,
        });
    }
    None
}

fn parse_osc_hex_channel(channel: &str) -> Option<u8> {
    if channel.is_empty() || !channel.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return None;
    }
    let max = 16_u32.checked_pow(channel.len() as u32)?.checked_sub(1)?;
    if max == 0 {
        return None;
    }
    let value = u32::from_str_radix(channel, 16).ok()?;
    Some(((value as f64 / max as f64) * 255.0).round() as u8)
}

fn ansi_color_luminance(index: u8) -> f64 {
    rgb_luminance(ansi256_rgb(index))
}

fn rgb_luminance(rgb: RgbColor) -> f64 {
    fn to_linear(channel: u8) -> f64 {
        let value = channel as f64 / 255.0;
        if value <= 0.03928 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    }
    0.2126 * to_linear(rgb.r) + 0.7152 * to_linear(rgb.g) + 0.0722 * to_linear(rgb.b)
}

fn ansi256_rgb(index: u8) -> RgbColor {
    const BASIC: [RgbColor; 16] = [
        RgbColor { r: 0, g: 0, b: 0 },
        RgbColor { r: 128, g: 0, b: 0 },
        RgbColor { r: 0, g: 128, b: 0 },
        RgbColor {
            r: 128,
            g: 128,
            b: 0,
        },
        RgbColor { r: 0, g: 0, b: 128 },
        RgbColor {
            r: 128,
            g: 0,
            b: 128,
        },
        RgbColor {
            r: 0,
            g: 128,
            b: 128,
        },
        RgbColor {
            r: 192,
            g: 192,
            b: 192,
        },
        RgbColor {
            r: 128,
            g: 128,
            b: 128,
        },
        RgbColor { r: 255, g: 0, b: 0 },
        RgbColor { r: 0, g: 255, b: 0 },
        RgbColor {
            r: 255,
            g: 255,
            b: 0,
        },
        RgbColor { r: 0, g: 0, b: 255 },
        RgbColor {
            r: 255,
            g: 0,
            b: 255,
        },
        RgbColor {
            r: 0,
            g: 255,
            b: 255,
        },
        RgbColor {
            r: 255,
            g: 255,
            b: 255,
        },
    ];

    match index {
        0..=15 => BASIC[index as usize],
        16..=231 => {
            let value = index - 16;
            let r = value / 36;
            let g = (value % 36) / 6;
            let b = value % 6;
            RgbColor {
                r: ansi_cube_channel(r),
                g: ansi_cube_channel(g),
                b: ansi_cube_channel(b),
            }
        }
        232..=255 => {
            let value = 8 + (index - 232) * 10;
            RgbColor {
                r: value,
                g: value,
                b: value,
            }
        }
    }
}

fn ansi_cube_channel(index: u8) -> u8 {
    if index == 0 {
        0
    } else {
        55 + index * 40
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_terminal_background_from_colorfgbg_like_pi() {
        assert_eq!(
            detect_terminal_background_from_colorfgbg(Some("0;15")),
            TerminalThemeDetection {
                theme: TerminalTheme::Light,
                source: TerminalThemeDetectionSource::ColorFgBg,
                detail: "background color index 15".to_string(),
                confidence: TerminalThemeDetectionConfidence::High,
            }
        );
        assert_eq!(
            detect_terminal_background_from_colorfgbg(Some("15;0")),
            TerminalThemeDetection {
                theme: TerminalTheme::Dark,
                source: TerminalThemeDetectionSource::ColorFgBg,
                detail: "background color index 0".to_string(),
                confidence: TerminalThemeDetectionConfidence::High,
            }
        );
        assert_eq!(
            detect_terminal_background_from_colorfgbg(Some("0;7;15")).theme,
            TerminalTheme::Light
        );
    }

    #[test]
    fn terminal_background_detection_defaults_to_dark_without_hints_like_pi() {
        assert_eq!(
            detect_terminal_background_from_colorfgbg(None),
            TerminalThemeDetection {
                theme: TerminalTheme::Dark,
                source: TerminalThemeDetectionSource::Fallback,
                detail: "no terminal background hint found".to_string(),
                confidence: TerminalThemeDetectionConfidence::Low,
            }
        );
        assert_eq!(
            detect_terminal_background_from_colorfgbg(Some("invalid")),
            TerminalThemeDetection {
                theme: TerminalTheme::Dark,
                source: TerminalThemeDetectionSource::Fallback,
                detail: "no terminal background hint found".to_string(),
                confidence: TerminalThemeDetectionConfidence::Low,
            }
        );
    }

    #[test]
    fn parses_osc11_background_color_like_pi() {
        assert_eq!(
            parse_osc11_background_color("\x1b]11;rgb:0000/8000/ffff\x07"),
            Some(RgbColor {
                r: 0,
                g: 128,
                b: 255,
            })
        );
        assert_eq!(
            parse_osc11_background_color("\x1b]11;#ffffff\x1b\\"),
            Some(RgbColor {
                r: 255,
                g: 255,
                b: 255,
            })
        );
        assert_eq!(
            parse_osc11_background_color("\x1b]11;#000000\x07"),
            Some(RgbColor { r: 0, g: 0, b: 0 })
        );
    }

    #[test]
    fn rejects_invalid_osc11_background_color_like_pi() {
        assert_eq!(parse_osc11_background_color("11;#ffffff"), None);
        assert_eq!(parse_osc11_background_color("\x1b]11;#ffff\x07"), None);
        assert_eq!(
            parse_osc11_background_color("\x1b]11;rgb:xx/00/00\x07"),
            None
        );
    }

    #[test]
    fn classifies_rgb_background_by_luminance_like_pi() {
        assert_eq!(
            get_theme_for_rgb_color(RgbColor { r: 8, g: 8, b: 8 }),
            TerminalTheme::Dark
        );
        assert_eq!(
            get_theme_for_rgb_color(RgbColor {
                r: 250,
                g: 250,
                b: 250,
            }),
            TerminalTheme::Light
        );
    }
}
