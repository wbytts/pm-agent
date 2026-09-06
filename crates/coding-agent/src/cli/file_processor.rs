use ai::ContentBlock;
use std::fs;
use std::path::{Path, PathBuf};

use crate::utils::base64::encode_base64;
use crate::utils::image_dimensions::format_resized_image_dimension_note;
use crate::utils::image_resize::resize_image;
use crate::utils::mime::detect_supported_image_mime_type_from_file;
use crate::utils::paths::{resolve_read_path, PathInputOptions};

#[derive(Debug, Clone, Default)]
pub struct ProcessFileOptions {
    pub cwd: Option<PathBuf>,
    pub auto_resize_images: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProcessedFiles {
    pub text: String,
    pub images: Vec<ContentBlock>,
}

pub fn process_file_arguments(
    file_args: &[String],
    options: ProcessFileOptions,
) -> Result<ProcessedFiles, String> {
    let cwd = options
        .cwd
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    let mut result = ProcessedFiles::default();

    for file_arg in file_args {
        let absolute_path = resolve_file_arg(file_arg, &cwd);
        if !absolute_path.exists() {
            return Err(format!("File not found: {}", absolute_path.display()));
        }
        let metadata = fs::metadata(&absolute_path)
            .map_err(|error| format!("Could not stat file {}: {error}", absolute_path.display()))?;
        if metadata.len() == 0 {
            continue;
        }

        let mime_type =
            detect_supported_image_mime_type_from_file(&absolute_path).map_err(|error| {
                format!(
                    "Could not inspect file {}: {error}",
                    absolute_path.display()
                )
            })?;
        if let Some(mime_type) = mime_type {
            let content = fs::read(&absolute_path).map_err(|error| {
                format!("Could not read file {}: {error}", absolute_path.display())
            })?;
            if options.auto_resize_images {
                let Some(resized) = resize_image(&content, mime_type) else {
                    result.text.push_str(&format!(
                        "<file name=\"{}\">[Image omitted: could not be resized below the inline image size limit.]</file>\n",
                        absolute_path.display()
                    ));
                    continue;
                };
                result.images.push(ContentBlock::Image {
                    mime_type: resized.mime_type,
                    data: resized.data,
                });
                if let Some(dimension_note) =
                    format_resized_image_dimension_note(resized.dimensions)
                {
                    result.text.push_str(&format!(
                        "<file name=\"{}\">{dimension_note}</file>\n",
                        absolute_path.display()
                    ));
                } else {
                    result.text.push_str(&format!(
                        "<file name=\"{}\"></file>\n",
                        absolute_path.display()
                    ));
                }
                continue;
            }
            result.images.push(ContentBlock::Image {
                mime_type: mime_type.to_string(),
                data: encode_base64(&content),
            });
            result.text.push_str(&format!(
                "<file name=\"{}\"></file>\n",
                absolute_path.display()
            ));
            continue;
        }

        let content = fs::read_to_string(&absolute_path)
            .map_err(|error| format!("Could not read file {}: {error}", absolute_path.display()))?;
        result.text.push_str(&format!(
            "<file name=\"{}\">\n{}\n</file>\n",
            absolute_path.display(),
            content
        ));
    }

    Ok(result)
}

fn resolve_file_arg(file_arg: &str, cwd: &Path) -> PathBuf {
    resolve_read_path(
        file_arg,
        cwd,
        Some(&PathInputOptions {
            trim: true,
            strip_at_prefix: true,
            normalize_unicode_spaces: true,
            ..PathInputOptions::default()
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn processes_text_files_into_file_tags() {
        let dir = temp_dir();
        fs::write(dir.join("prompt.txt"), "hello").expect("file should write");

        let processed = process_file_arguments(
            &["prompt.txt".to_string()],
            ProcessFileOptions {
                cwd: Some(dir.clone()),
                ..ProcessFileOptions::default()
            },
        )
        .expect("files should process");

        assert!(processed.text.contains("<file name="));
        assert!(processed.text.contains("hello"));
        assert!(processed.images.is_empty());
    }

    #[test]
    fn skips_empty_files() {
        let dir = temp_dir();
        fs::write(dir.join("empty.txt"), "").expect("file should write");

        let processed = process_file_arguments(
            &["empty.txt".to_string()],
            ProcessFileOptions {
                cwd: Some(dir),
                ..ProcessFileOptions::default()
            },
        )
        .expect("files should process");

        assert!(processed.text.is_empty());
        assert!(processed.images.is_empty());
    }

    #[test]
    fn processes_png_as_image_attachment() {
        let dir = temp_dir();
        let mut png = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
        png.extend_from_slice(&13u32.to_be_bytes());
        png.extend_from_slice(b"IHDR");
        png.extend_from_slice(&[0; 17]);
        png.extend_from_slice(&1u32.to_be_bytes());
        png.extend_from_slice(b"IDAT");
        fs::write(dir.join("image.png"), png).expect("file should write");

        let processed = process_file_arguments(
            &["image.png".to_string()],
            ProcessFileOptions {
                cwd: Some(dir),
                ..ProcessFileOptions::default()
            },
        )
        .expect("files should process");

        assert_eq!(processed.images.len(), 1);
        assert!(matches!(
            &processed.images[0],
            ContentBlock::Image { mime_type, .. } if mime_type == "image/png"
        ));
        assert!(processed.text.contains("</file>"));
    }

    #[test]
    fn auto_resize_omits_image_when_inline_limits_cannot_be_met_like_pi() {
        let dir = temp_dir();
        let mut png = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
        png.extend_from_slice(&13u32.to_be_bytes());
        png.extend_from_slice(b"IHDR");
        png.extend_from_slice(&3000u32.to_be_bytes());
        png.extend_from_slice(&3000u32.to_be_bytes());
        png.extend_from_slice(&[8, 2, 0, 0, 0]);
        png.extend_from_slice(&1u32.to_be_bytes());
        png.extend_from_slice(b"IDAT");
        fs::write(dir.join("huge.png"), png).expect("file should write");

        let processed = process_file_arguments(
            &["huge.png".to_string()],
            ProcessFileOptions {
                cwd: Some(dir),
                auto_resize_images: true,
            },
        )
        .expect("files should process");

        assert!(processed.images.is_empty());
        assert!(processed
            .text
            .contains("[Image omitted: could not be resized below the inline image size limit.]"));
    }

    #[test]
    fn reports_missing_file() {
        let error = process_file_arguments(
            &["missing.txt".to_string()],
            ProcessFileOptions {
                cwd: Some(temp_dir()),
                ..ProcessFileOptions::default()
            },
        )
        .expect_err("missing file should fail");

        assert!(error.contains("File not found"));
    }

    #[test]
    fn reads_macos_screenshot_ampm_path_variant_like_pi() {
        let dir = temp_dir();
        let actual = "Screenshot 2024-01-01 at 10.00.00\u{202f}AM.txt";
        fs::write(dir.join(actual), "screen").expect("file should write");

        let processed = process_file_arguments(
            &["Screenshot 2024-01-01 at 10.00.00 AM.txt".to_string()],
            ProcessFileOptions {
                cwd: Some(dir),
                ..ProcessFileOptions::default()
            },
        )
        .expect("screenshot variant should resolve");

        assert!(processed.text.contains("screen"));
    }

    #[test]
    fn reads_macos_curly_quote_path_variant_like_pi() {
        let dir = temp_dir();
        let actual = "Capture d\u{2019}ecran.txt";
        fs::write(dir.join(actual), "capture").expect("file should write");

        let processed = process_file_arguments(
            &["Capture d'ecran.txt".to_string()],
            ProcessFileOptions {
                cwd: Some(dir),
                ..ProcessFileOptions::default()
            },
        )
        .expect("curly quote variant should resolve");

        assert!(processed.text.contains("capture"));
    }

    fn temp_dir() -> PathBuf {
        let id = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("pm-agent-cli-file-test-{id}"));
        fs::create_dir_all(&dir).expect("dir should create");
        dir
    }
}
