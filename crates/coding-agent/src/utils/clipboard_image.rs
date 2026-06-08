use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

const SUPPORTED_IMAGE_MIME_TYPES: [&str; 4] =
    ["image/png", "image/jpeg", "image/webp", "image/gif"];
const DEFAULT_LIST_TIMEOUT_MS: u64 = 1000;
const DEFAULT_READ_TIMEOUT_MS: u64 = 3000;
const DEFAULT_POWERSHELL_TIMEOUT_MS: u64 = 5000;
const DEFAULT_MAX_BUFFER_BYTES: usize = 50 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipboardImage {
    pub bytes: Vec<u8>,
    pub mime_type: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ClipboardImageCommandOutput {
    pub stdout: Vec<u8>,
    pub ok: bool,
}

impl ClipboardImageCommandOutput {
    pub fn success(stdout: Vec<u8>) -> Self {
        Self { stdout, ok: true }
    }

    pub fn failure() -> Self {
        Self {
            stdout: Vec::new(),
            ok: false,
        }
    }
}

pub trait ClipboardImageRunner {
    fn run(
        &mut self,
        command: &str,
        args: &[String],
        timeout_ms: u64,
        max_buffer_bytes: usize,
    ) -> ClipboardImageCommandOutput;
    fn read_file(&mut self, path: &Path) -> std::io::Result<Vec<u8>>;
    fn read_to_string(&mut self, path: &Path) -> std::io::Result<String>;
    fn write_file(&mut self, path: &Path, bytes: &[u8]) -> std::io::Result<()>;
    fn remove_file(&mut self, path: &Path) -> std::io::Result<()>;
    fn next_temp_png_path(&mut self) -> PathBuf;
    fn native_image(&mut self) -> Option<Vec<u8>>;
}

pub struct SystemClipboardImageRunner;

impl ClipboardImageRunner for SystemClipboardImageRunner {
    fn run(
        &mut self,
        command: &str,
        args: &[String],
        _timeout_ms: u64,
        max_buffer_bytes: usize,
    ) -> ClipboardImageCommandOutput {
        let output = match Command::new(command).args(args).output() {
            Ok(output) => output,
            Err(_) => return ClipboardImageCommandOutput::failure(),
        };
        if !output.status.success() || output.stdout.len() > max_buffer_bytes {
            return ClipboardImageCommandOutput::failure();
        }
        ClipboardImageCommandOutput::success(output.stdout)
    }

    fn read_file(&mut self, path: &Path) -> std::io::Result<Vec<u8>> {
        fs::read(path)
    }

    fn read_to_string(&mut self, path: &Path) -> std::io::Result<String> {
        fs::read_to_string(path)
    }

    fn write_file(&mut self, path: &Path, bytes: &[u8]) -> std::io::Result<()> {
        fs::write(path, bytes)
    }

    fn remove_file(&mut self, path: &Path) -> std::io::Result<()> {
        fs::remove_file(path)
    }

    fn next_temp_png_path(&mut self) -> PathBuf {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|value| value.as_nanos())
            .unwrap_or_default();
        std::env::temp_dir().join(format!("pi-wsl-clip-{}-{now}.png", std::process::id()))
    }

    fn native_image(&mut self) -> Option<Vec<u8>> {
        None
    }
}

pub fn is_wayland_session<K, V>(env: impl IntoIterator<Item = (K, V)>) -> bool
where
    K: AsRef<str>,
    V: AsRef<str>,
{
    env_contains_wayland(env)
}

pub fn is_wsl_environment<K, V>(
    env: impl IntoIterator<Item = (K, V)>,
    proc_version: Option<&str>,
) -> bool
where
    K: AsRef<str>,
    V: AsRef<str>,
{
    if env.into_iter().any(|(key, value)| {
        let key = key.as_ref();
        let value = value.as_ref();
        (key == "WSL_DISTRO_NAME" || key == "WSLENV") && !value.is_empty()
    }) {
        return true;
    }

    proc_version
        .map(|value| {
            let lower = value.to_ascii_lowercase();
            lower.contains("microsoft") || lower.contains("wsl")
        })
        .unwrap_or(false)
}

fn env_contains_wayland<K, V>(env: impl IntoIterator<Item = (K, V)>) -> bool
where
    K: AsRef<str>,
    V: AsRef<str>,
{
    env.into_iter().any(|(key, value)| {
        let key = key.as_ref();
        let value = value.as_ref();
        (key == "WAYLAND_DISPLAY" && !value.is_empty())
            || (key == "XDG_SESSION_TYPE" && value == "wayland")
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipboardImageReadBackend {
    WlPaste,
    Xclip,
    PowerShell,
    Native,
}

pub fn clipboard_image_read_plan<K, V>(
    platform: &str,
    env: impl IntoIterator<Item = (K, V)>,
    is_wsl: bool,
) -> Vec<ClipboardImageReadBackend>
where
    K: AsRef<str>,
    V: AsRef<str>,
{
    let env = env
        .into_iter()
        .map(|(key, value)| (key.as_ref().to_string(), value.as_ref().to_string()))
        .collect::<Vec<_>>();
    if env
        .iter()
        .any(|(key, value)| key == "TERMUX_VERSION" && !value.is_empty())
    {
        return Vec::new();
    }

    if platform != "linux" {
        return vec![ClipboardImageReadBackend::Native];
    }

    let wayland = env_contains_wayland(
        env.iter()
            .map(|(key, value)| (key.as_str(), value.as_str())),
    );
    let mut plan = Vec::new();
    if wayland || is_wsl {
        plan.push(ClipboardImageReadBackend::WlPaste);
        plan.push(ClipboardImageReadBackend::Xclip);
    }
    if is_wsl {
        plan.push(ClipboardImageReadBackend::PowerShell);
    }
    if !wayland {
        plan.push(ClipboardImageReadBackend::Native);
    }
    plan
}

pub fn read_clipboard_image_with_runner<K, V>(
    platform: &str,
    env: impl IntoIterator<Item = (K, V)>,
    is_wsl: bool,
    runner: &mut impl ClipboardImageRunner,
) -> Option<ClipboardImage>
where
    K: AsRef<str>,
    V: AsRef<str>,
{
    let env = env
        .into_iter()
        .map(|(key, value)| (key.as_ref().to_string(), value.as_ref().to_string()))
        .collect::<Vec<_>>();

    let mut image = None;
    for backend in clipboard_image_read_plan(
        platform,
        env.iter()
            .map(|(key, value)| (key.as_str(), value.as_str())),
        is_wsl,
    ) {
        image = match backend {
            ClipboardImageReadBackend::WlPaste => read_clipboard_image_via_wl_paste(runner),
            ClipboardImageReadBackend::Xclip => read_clipboard_image_via_xclip(runner),
            ClipboardImageReadBackend::PowerShell => read_clipboard_image_via_powershell(runner),
            ClipboardImageReadBackend::Native => read_clipboard_image_via_native_clipboard(runner),
        };
        if image.is_some() {
            break;
        }
    }

    let image = image?;
    if is_supported_image_mime_type(&image.mime_type) {
        return Some(image);
    }

    convert_unsupported_image_to_png(&image.bytes, &image.mime_type).map(|bytes| ClipboardImage {
        bytes,
        mime_type: String::from("image/png"),
    })
}

pub fn read_clipboard_image<K, V>(
    platform: &str,
    env: impl IntoIterator<Item = (K, V)>,
    runner: &mut impl ClipboardImageRunner,
) -> Option<ClipboardImage>
where
    K: AsRef<str>,
    V: AsRef<str>,
{
    let env = env
        .into_iter()
        .map(|(key, value)| (key.as_ref().to_string(), value.as_ref().to_string()))
        .collect::<Vec<_>>();
    let proc_version = runner.read_to_string(Path::new("/proc/version")).ok();
    let is_wsl = is_wsl_environment(
        env.iter()
            .map(|(key, value)| (key.as_str(), value.as_str())),
        proc_version.as_deref(),
    );
    read_clipboard_image_with_runner(
        platform,
        env.iter()
            .map(|(key, value)| (key.as_str(), value.as_str())),
        is_wsl,
        runner,
    )
}

pub fn write_clipboard_image_for_editor_insert<K, V>(
    platform: &str,
    env: impl IntoIterator<Item = (K, V)>,
    is_wsl: bool,
    temp_dir: impl AsRef<Path>,
    id: &str,
    runner: &mut impl ClipboardImageRunner,
) -> Option<PathBuf>
where
    K: AsRef<str>,
    V: AsRef<str>,
{
    let image = read_clipboard_image_with_runner(platform, env, is_wsl, runner)?;
    let ext = extension_for_image_mime_type(&image.mime_type).unwrap_or("png");
    let file_path = temp_dir.as_ref().join(format!("pi-clipboard-{id}.{ext}"));

    runner.write_file(&file_path, &image.bytes).ok()?;
    Some(file_path)
}

pub fn write_clipboard_image_for_editor_insert_auto<K, V>(
    platform: &str,
    env: impl IntoIterator<Item = (K, V)>,
    temp_dir: impl AsRef<Path>,
    id: &str,
    runner: &mut impl ClipboardImageRunner,
) -> Option<PathBuf>
where
    K: AsRef<str>,
    V: AsRef<str>,
{
    let image = read_clipboard_image(platform, env, runner)?;
    let ext = extension_for_image_mime_type(&image.mime_type).unwrap_or("png");
    let file_path = temp_dir.as_ref().join(format!("pi-clipboard-{id}.{ext}"));

    runner.write_file(&file_path, &image.bytes).ok()?;
    Some(file_path)
}

pub fn extension_for_image_mime_type(mime_type: &str) -> Option<&'static str> {
    match base_mime_type(mime_type).as_str() {
        "image/png" => Some("png"),
        "image/jpeg" => Some("jpg"),
        "image/webp" => Some("webp"),
        "image/gif" => Some("gif"),
        _ => None,
    }
}

pub fn select_preferred_image_mime_type<'a>(
    mime_types: impl IntoIterator<Item = &'a str>,
) -> Option<String> {
    let normalized = mime_types
        .into_iter()
        .map(str::trim)
        .filter(|mime_type| !mime_type.is_empty())
        .map(|raw| (raw.to_string(), base_mime_type(raw)))
        .collect::<Vec<_>>();

    for preferred in ["image/png", "image/jpeg", "image/webp", "image/gif"] {
        if let Some((raw, _)) = normalized.iter().find(|(_, base)| base == preferred) {
            return Some(raw.clone());
        }
    }

    normalized
        .into_iter()
        .find(|(_, base)| base.starts_with("image/"))
        .map(|(raw, _)| raw)
}

fn base_mime_type(mime_type: &str) -> String {
    mime_type
        .split(';')
        .next()
        .unwrap_or(mime_type)
        .trim()
        .to_ascii_lowercase()
}

fn is_supported_image_mime_type(mime_type: &str) -> bool {
    let base = base_mime_type(mime_type);
    SUPPORTED_IMAGE_MIME_TYPES
        .iter()
        .any(|supported| *supported == base)
}

fn convert_unsupported_image_to_png(bytes: &[u8], mime_type: &str) -> Option<Vec<u8>> {
    match base_mime_type(mime_type).as_str() {
        "image/bmp" | "image/x-ms-bmp" => bmp_to_png(bytes),
        _ => None,
    }
}

fn bmp_to_png(bytes: &[u8]) -> Option<Vec<u8>> {
    if bytes.len() < 54 || &bytes[0..2] != b"BM" {
        return None;
    }

    let pixel_offset = read_u32_le(bytes, 10)? as usize;
    let dib_header_size = read_u32_le(bytes, 14)?;
    if dib_header_size < 40 {
        return None;
    }

    let width = read_i32_le(bytes, 18)?;
    let height = read_i32_le(bytes, 22)?;
    let planes = read_u16_le(bytes, 26)?;
    let bits_per_pixel = read_u16_le(bytes, 28)?;
    let compression = read_u32_le(bytes, 30)?;
    if width <= 0 || height == 0 || planes != 1 || compression != 0 {
        return None;
    }
    if bits_per_pixel != 24 && bits_per_pixel != 32 {
        return None;
    }

    let width = width as usize;
    let height_abs = height.unsigned_abs() as usize;
    let bytes_per_pixel = usize::from(bits_per_pixel / 8);
    let row_stride = (width * bytes_per_pixel).div_ceil(4) * 4;
    let pixel_data_len = row_stride.checked_mul(height_abs)?;
    if pixel_offset.checked_add(pixel_data_len)? > bytes.len() {
        return None;
    }

    let mut rgba = Vec::with_capacity(width.checked_mul(height_abs)?.checked_mul(4)?);
    for output_y in 0..height_abs {
        let source_y = if height > 0 {
            height_abs - 1 - output_y
        } else {
            output_y
        };
        let row_start = pixel_offset + source_y * row_stride;
        for x in 0..width {
            let pixel_start = row_start + x * bytes_per_pixel;
            let blue = bytes[pixel_start];
            let green = bytes[pixel_start + 1];
            let red = bytes[pixel_start + 2];
            let alpha = if bytes_per_pixel == 4 {
                bytes[pixel_start + 3]
            } else {
                0xff
            };
            rgba.extend_from_slice(&[red, green, blue, alpha]);
        }
    }

    rgba_to_png(width as u32, height_abs as u32, &rgba)
}

fn rgba_to_png(width: u32, height: u32, rgba: &[u8]) -> Option<Vec<u8>> {
    let width_usize = width as usize;
    let height_usize = height as usize;
    if rgba.len() != width_usize.checked_mul(height_usize)?.checked_mul(4)? {
        return None;
    }

    let mut raw = Vec::with_capacity(height_usize.checked_mul(width_usize.checked_mul(4)? + 1)?);
    for y in 0..height_usize {
        raw.push(0);
        let row_start = y * width_usize * 4;
        raw.extend_from_slice(&rgba[row_start..row_start + width_usize * 4]);
    }

    let compressed = zlib_store_blocks(&raw)?;
    let mut png = Vec::new();
    png.extend_from_slice(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]);

    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    write_png_chunk(&mut png, b"IHDR", &ihdr);
    write_png_chunk(&mut png, b"IDAT", &compressed);
    write_png_chunk(&mut png, b"IEND", &[]);

    Some(png)
}

fn zlib_store_blocks(data: &[u8]) -> Option<Vec<u8>> {
    let mut output = vec![0x78, 0x01];
    let mut remaining = data;

    while !remaining.is_empty() {
        let chunk_len = remaining.len().min(u16::MAX as usize);
        let is_final = chunk_len == remaining.len();
        output.push(if is_final { 0x01 } else { 0x00 });
        let len = chunk_len as u16;
        output.extend_from_slice(&len.to_le_bytes());
        output.extend_from_slice(&(!len).to_le_bytes());
        output.extend_from_slice(&remaining[..chunk_len]);
        remaining = &remaining[chunk_len..];
    }

    if data.is_empty() {
        output.push(0x01);
        output.extend_from_slice(&0u16.to_le_bytes());
        output.extend_from_slice(&(!0u16).to_le_bytes());
    }

    output.extend_from_slice(&adler32(data).to_be_bytes());
    Some(output)
}

fn write_png_chunk(output: &mut Vec<u8>, chunk_type: &[u8; 4], data: &[u8]) {
    output.extend_from_slice(&(data.len() as u32).to_be_bytes());
    output.extend_from_slice(chunk_type);
    output.extend_from_slice(data);

    let mut crc_input = Vec::with_capacity(4 + data.len());
    crc_input.extend_from_slice(chunk_type);
    crc_input.extend_from_slice(data);
    output.extend_from_slice(&crc32(&crc_input).to_be_bytes());
}

fn adler32(data: &[u8]) -> u32 {
    const MOD_ADLER: u32 = 65_521;
    let mut a = 1u32;
    let mut b = 0u32;
    for byte in data {
        a = (a + u32::from(*byte)) % MOD_ADLER;
        b = (b + a) % MOD_ADLER;
    }
    (b << 16) | a
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for byte in data {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xedb8_8320 & mask);
        }
    }
    !crc
}

fn read_u16_le(bytes: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_le_bytes(
        bytes.get(offset..offset + 2)?.try_into().ok()?,
    ))
}

fn read_u32_le(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        bytes.get(offset..offset + 4)?.try_into().ok()?,
    ))
}

fn read_i32_le(bytes: &[u8], offset: usize) -> Option<i32> {
    Some(i32::from_le_bytes(
        bytes.get(offset..offset + 4)?.try_into().ok()?,
    ))
}

fn read_clipboard_image_via_wl_paste(
    runner: &mut impl ClipboardImageRunner,
) -> Option<ClipboardImage> {
    let list = runner.run(
        "wl-paste",
        &[String::from("--list-types")],
        DEFAULT_LIST_TIMEOUT_MS,
        DEFAULT_MAX_BUFFER_BYTES,
    );
    if !list.ok {
        return None;
    }

    let stdout = String::from_utf8_lossy(&list.stdout);
    let selected_type = select_preferred_image_mime_type(stdout.lines())?;
    let data = runner.run(
        "wl-paste",
        &[
            String::from("--type"),
            selected_type.clone(),
            String::from("--no-newline"),
        ],
        DEFAULT_READ_TIMEOUT_MS,
        DEFAULT_MAX_BUFFER_BYTES,
    );
    if !data.ok || data.stdout.is_empty() {
        return None;
    }

    Some(ClipboardImage {
        bytes: data.stdout,
        mime_type: base_mime_type(&selected_type),
    })
}

fn read_clipboard_image_via_xclip(
    runner: &mut impl ClipboardImageRunner,
) -> Option<ClipboardImage> {
    let targets = runner.run(
        "xclip",
        &[
            String::from("-selection"),
            String::from("clipboard"),
            String::from("-t"),
            String::from("TARGETS"),
            String::from("-o"),
        ],
        DEFAULT_LIST_TIMEOUT_MS,
        DEFAULT_MAX_BUFFER_BYTES,
    );

    let candidate_types = if targets.ok {
        String::from_utf8_lossy(&targets.stdout)
            .lines()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };

    let preferred = select_preferred_image_mime_type(candidate_types.iter().map(String::as_str));
    let mut try_types = Vec::new();
    if let Some(preferred) = preferred {
        try_types.push(preferred);
    }
    try_types.extend(
        SUPPORTED_IMAGE_MIME_TYPES
            .iter()
            .map(|mime_type| (*mime_type).to_string()),
    );

    for mime_type in try_types {
        let data = runner.run(
            "xclip",
            &[
                String::from("-selection"),
                String::from("clipboard"),
                String::from("-t"),
                mime_type.clone(),
                String::from("-o"),
            ],
            DEFAULT_READ_TIMEOUT_MS,
            DEFAULT_MAX_BUFFER_BYTES,
        );
        if data.ok && !data.stdout.is_empty() {
            return Some(ClipboardImage {
                bytes: data.stdout,
                mime_type: base_mime_type(&mime_type),
            });
        }
    }

    None
}

fn read_clipboard_image_via_powershell(
    runner: &mut impl ClipboardImageRunner,
) -> Option<ClipboardImage> {
    let tmp_file = runner.next_temp_png_path();
    let wsl_path = tmp_file.to_string_lossy().to_string();
    let win_path_result = runner.run(
        "wslpath",
        &[String::from("-w"), wsl_path],
        DEFAULT_LIST_TIMEOUT_MS,
        DEFAULT_MAX_BUFFER_BYTES,
    );
    if !win_path_result.ok {
        let _ = runner.remove_file(&tmp_file);
        return None;
    }

    let win_path = String::from_utf8_lossy(&win_path_result.stdout)
        .trim()
        .to_string();
    if win_path.is_empty() {
        let _ = runner.remove_file(&tmp_file);
        return None;
    }

    let ps_quoted_win_path = win_path.replace('\'', "''");
    let ps_script = [
        "Add-Type -AssemblyName System.Windows.Forms".to_string(),
        "Add-Type -AssemblyName System.Drawing".to_string(),
        format!("$path = '{ps_quoted_win_path}'"),
        "$img = [System.Windows.Forms.Clipboard]::GetImage()".to_string(),
        "if ($img) { $img.Save($path, [System.Drawing.Imaging.ImageFormat]::Png); Write-Output 'ok' } else { Write-Output 'empty' }".to_string(),
    ]
    .join("; ");

    let result = runner.run(
        "powershell.exe",
        &[
            String::from("-NoProfile"),
            String::from("-Command"),
            ps_script,
        ],
        DEFAULT_POWERSHELL_TIMEOUT_MS,
        DEFAULT_MAX_BUFFER_BYTES,
    );
    let output = String::from_utf8_lossy(&result.stdout).trim().to_string();
    let image = if result.ok && output == "ok" {
        runner.read_file(&tmp_file).ok().and_then(|bytes| {
            if bytes.is_empty() {
                None
            } else {
                Some(ClipboardImage {
                    bytes,
                    mime_type: String::from("image/png"),
                })
            }
        })
    } else {
        None
    };

    let _ = runner.remove_file(&tmp_file);
    image
}

fn read_clipboard_image_via_native_clipboard(
    runner: &mut impl ClipboardImageRunner,
) -> Option<ClipboardImage> {
    let bytes = runner.native_image()?;
    if bytes.is_empty() {
        return None;
    }
    Some(ClipboardImage {
        bytes,
        mime_type: String::from("image/png"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};

    #[derive(Default)]
    struct FakeClipboardImageRunner {
        calls: Vec<(String, Vec<String>)>,
        native_image: Option<Vec<u8>>,
        temp_path: PathBuf,
        temp_bytes: Vec<u8>,
        removed_paths: Vec<PathBuf>,
    }

    impl ClipboardImageRunner for FakeClipboardImageRunner {
        fn run(
            &mut self,
            command: &str,
            args: &[String],
            _timeout_ms: u64,
            _max_buffer_bytes: usize,
        ) -> ClipboardImageCommandOutput {
            self.calls.push((command.to_string(), args.to_vec()));
            match (command, args.first().map(String::as_str)) {
                ("wl-paste", Some("--list-types")) => {
                    ClipboardImageCommandOutput::success(b"text/plain\nimage/png\n".to_vec())
                }
                ("wl-paste", Some("--type")) => ClipboardImageCommandOutput::success(vec![1, 2, 3]),
                _ => ClipboardImageCommandOutput::failure(),
            }
        }

        fn read_file(&mut self, _path: &Path) -> std::io::Result<Vec<u8>> {
            Ok(self.temp_bytes.clone())
        }

        fn read_to_string(&mut self, _path: &Path) -> std::io::Result<String> {
            Ok(String::new())
        }

        fn write_file(&mut self, _path: &Path, _bytes: &[u8]) -> std::io::Result<()> {
            Ok(())
        }

        fn remove_file(&mut self, path: &Path) -> std::io::Result<()> {
            self.removed_paths.push(path.to_path_buf());
            Ok(())
        }

        fn next_temp_png_path(&mut self) -> PathBuf {
            self.temp_path.clone()
        }

        fn native_image(&mut self) -> Option<Vec<u8>> {
            self.native_image.clone()
        }
    }

    #[test]
    fn detects_wayland_session_like_pi_clipboard_image() {
        assert!(is_wayland_session(HashMap::from([(
            "WAYLAND_DISPLAY",
            "wayland-1"
        )])));
        assert!(is_wayland_session(HashMap::from([(
            "XDG_SESSION_TYPE",
            "wayland"
        )])));
        assert!(!is_wayland_session(HashMap::from([(
            "XDG_SESSION_TYPE",
            "x11"
        )])));
    }

    #[test]
    fn detects_wsl_environment_like_pi_clipboard_image() {
        assert!(is_wsl_environment(
            HashMap::from([("WSL_DISTRO_NAME", "Ubuntu")]),
            None
        ));
        assert!(is_wsl_environment(
            HashMap::from([("WSLENV", "WT_SESSION/u")]),
            None
        ));
        assert!(is_wsl_environment(
            HashMap::<&str, &str>::new(),
            Some("Linux version 5.15.90.1-microsoft-standard-WSL2")
        ));
        assert!(is_wsl_environment(
            HashMap::<&str, &str>::new(),
            Some("Linux version 6.6.36.6-1 wsl")
        ));
        assert!(!is_wsl_environment(
            HashMap::<&str, &str>::new(),
            Some("Linux version 6.6.36 x86_64 GNU/Linux")
        ));
        assert!(!is_wsl_environment(HashMap::<&str, &str>::new(), None));
    }

    #[test]
    fn maps_image_mime_type_extensions_like_pi_clipboard_image() {
        assert_eq!(
            extension_for_image_mime_type(" image/png; charset=binary "),
            Some("png")
        );
        assert_eq!(extension_for_image_mime_type("IMAGE/JPEG"), Some("jpg"));
        assert_eq!(extension_for_image_mime_type("image/webp"), Some("webp"));
        assert_eq!(extension_for_image_mime_type("image/gif"), Some("gif"));
        assert_eq!(extension_for_image_mime_type("image/bmp"), None);
    }

    #[test]
    fn plans_clipboard_image_read_backends_like_pi_clipboard_image() {
        assert_eq!(
            clipboard_image_read_plan(
                "linux",
                HashMap::from([("WAYLAND_DISPLAY", "wayland-1")]),
                false,
            ),
            vec![
                ClipboardImageReadBackend::WlPaste,
                ClipboardImageReadBackend::Xclip,
            ]
        );
        assert_eq!(
            clipboard_image_read_plan("linux", HashMap::from([("WSLENV", "1")]), true),
            vec![
                ClipboardImageReadBackend::WlPaste,
                ClipboardImageReadBackend::Xclip,
                ClipboardImageReadBackend::PowerShell,
                ClipboardImageReadBackend::Native,
            ]
        );
        assert_eq!(
            clipboard_image_read_plan("linux", HashMap::<&str, &str>::new(), false),
            vec![ClipboardImageReadBackend::Native]
        );
        assert_eq!(
            clipboard_image_read_plan("darwin", HashMap::<&str, &str>::new(), false),
            vec![ClipboardImageReadBackend::Native]
        );
        assert!(clipboard_image_read_plan(
            "linux",
            HashMap::from([("TERMUX_VERSION", "1")]),
            false
        )
        .is_empty());
    }

    #[test]
    fn reads_wayland_clipboard_image_via_wl_paste_like_pi() {
        let mut runner = FakeClipboardImageRunner::default();
        let image = read_clipboard_image_with_runner(
            "linux",
            HashMap::from([("WAYLAND_DISPLAY", "wayland-1")]),
            false,
            &mut runner,
        )
        .expect("wayland image");

        assert_eq!(image.mime_type, "image/png");
        assert_eq!(image.bytes, vec![1, 2, 3]);
        assert_eq!(
            runner.calls,
            vec![
                ("wl-paste".to_string(), vec!["--list-types".to_string()]),
                (
                    "wl-paste".to_string(),
                    vec![
                        "--type".to_string(),
                        "image/png".to_string(),
                        "--no-newline".to_string(),
                    ]
                ),
            ]
        );
    }

    #[test]
    fn falls_back_to_xclip_when_wl_paste_is_missing_like_pi() {
        struct XclipRunner {
            calls: Vec<(String, Vec<String>)>,
        }

        impl ClipboardImageRunner for XclipRunner {
            fn run(
                &mut self,
                command: &str,
                args: &[String],
                _timeout_ms: u64,
                _max_buffer_bytes: usize,
            ) -> ClipboardImageCommandOutput {
                self.calls.push((command.to_string(), args.to_vec()));
                if command == "wl-paste" {
                    return ClipboardImageCommandOutput::failure();
                }
                if command == "xclip" && args.iter().any(|arg| arg == "TARGETS") {
                    return ClipboardImageCommandOutput::success(b"image/png\n".to_vec());
                }
                if command == "xclip" && args.iter().any(|arg| arg == "image/png") {
                    return ClipboardImageCommandOutput::success(vec![9, 8]);
                }
                ClipboardImageCommandOutput::failure()
            }

            fn read_file(&mut self, _path: &Path) -> std::io::Result<Vec<u8>> {
                Ok(Vec::new())
            }

            fn read_to_string(&mut self, _path: &Path) -> std::io::Result<String> {
                Ok(String::new())
            }

            fn write_file(&mut self, _path: &Path, _bytes: &[u8]) -> std::io::Result<()> {
                Ok(())
            }

            fn remove_file(&mut self, _path: &Path) -> std::io::Result<()> {
                Ok(())
            }

            fn next_temp_png_path(&mut self) -> PathBuf {
                PathBuf::from("/tmp/unused.png")
            }

            fn native_image(&mut self) -> Option<Vec<u8>> {
                None
            }
        }

        let mut runner = XclipRunner { calls: Vec::new() };
        let image = read_clipboard_image_with_runner(
            "linux",
            HashMap::from([("XDG_SESSION_TYPE", "wayland")]),
            false,
            &mut runner,
        )
        .expect("xclip image");

        assert_eq!(image.mime_type, "image/png");
        assert_eq!(image.bytes, vec![9, 8]);
    }

    #[test]
    fn reads_wsl_clipboard_image_via_powershell_like_pi() {
        struct WslRunner {
            calls: Vec<(String, Vec<String>)>,
            temp_path: PathBuf,
            removed_paths: Vec<PathBuf>,
        }

        impl ClipboardImageRunner for WslRunner {
            fn run(
                &mut self,
                command: &str,
                args: &[String],
                _timeout_ms: u64,
                _max_buffer_bytes: usize,
            ) -> ClipboardImageCommandOutput {
                self.calls.push((command.to_string(), args.to_vec()));
                match command {
                    "wl-paste" | "xclip" => ClipboardImageCommandOutput::failure(),
                    "wslpath" => ClipboardImageCommandOutput::success(
                        b"C:\\Users\\O'Hare\\clip.png\n".to_vec(),
                    ),
                    "powershell.exe" => {
                        assert!(args[2].contains("$path = 'C:\\Users\\O''Hare\\clip.png'"));
                        ClipboardImageCommandOutput::success(b"ok\n".to_vec())
                    }
                    _ => ClipboardImageCommandOutput::failure(),
                }
            }

            fn read_file(&mut self, path: &Path) -> std::io::Result<Vec<u8>> {
                assert_eq!(path, self.temp_path.as_path());
                Ok(vec![4, 5, 6])
            }

            fn read_to_string(&mut self, _path: &Path) -> std::io::Result<String> {
                Ok(String::new())
            }

            fn write_file(&mut self, _path: &Path, _bytes: &[u8]) -> std::io::Result<()> {
                Ok(())
            }

            fn remove_file(&mut self, path: &Path) -> std::io::Result<()> {
                self.removed_paths.push(path.to_path_buf());
                Ok(())
            }

            fn next_temp_png_path(&mut self) -> PathBuf {
                self.temp_path.clone()
            }

            fn native_image(&mut self) -> Option<Vec<u8>> {
                panic!("native clipboard should not be called before PowerShell on WSL");
            }
        }

        let temp_path = PathBuf::from("/tmp/pi-wsl-clip-test.png");
        let mut runner = WslRunner {
            calls: Vec::new(),
            temp_path: temp_path.clone(),
            removed_paths: Vec::new(),
        };
        let image = read_clipboard_image_with_runner(
            "linux",
            HashMap::from([("WSL_DISTRO_NAME", "Ubuntu")]),
            true,
            &mut runner,
        )
        .expect("powershell image");

        assert_eq!(image.mime_type, "image/png");
        assert_eq!(image.bytes, vec![4, 5, 6]);
        assert_eq!(runner.removed_paths, vec![temp_path]);
    }

    #[test]
    fn read_clipboard_image_detects_wsl_before_planning_like_pi() {
        struct AutoWslRunner {
            temp_path: PathBuf,
        }

        impl ClipboardImageRunner for AutoWslRunner {
            fn run(
                &mut self,
                command: &str,
                _args: &[String],
                _timeout_ms: u64,
                _max_buffer_bytes: usize,
            ) -> ClipboardImageCommandOutput {
                match command {
                    "wl-paste" | "xclip" => ClipboardImageCommandOutput::failure(),
                    "wslpath" => ClipboardImageCommandOutput::success(b"C:\\clip.png\n".to_vec()),
                    "powershell.exe" => ClipboardImageCommandOutput::success(b"ok\n".to_vec()),
                    _ => ClipboardImageCommandOutput::failure(),
                }
            }

            fn read_file(&mut self, path: &Path) -> std::io::Result<Vec<u8>> {
                assert_eq!(path, self.temp_path.as_path());
                Ok(vec![7, 7, 7])
            }

            fn read_to_string(&mut self, _path: &Path) -> std::io::Result<String> {
                Ok(String::new())
            }

            fn write_file(&mut self, _path: &Path, _bytes: &[u8]) -> std::io::Result<()> {
                Ok(())
            }

            fn remove_file(&mut self, _path: &Path) -> std::io::Result<()> {
                Ok(())
            }

            fn next_temp_png_path(&mut self) -> PathBuf {
                self.temp_path.clone()
            }

            fn native_image(&mut self) -> Option<Vec<u8>> {
                panic!("native clipboard should not be used before WSL PowerShell fallback");
            }
        }

        let mut runner = AutoWslRunner {
            temp_path: PathBuf::from("/tmp/pi-wsl-auto.png"),
        };
        let image = read_clipboard_image(
            "linux",
            HashMap::from([("WSL_DISTRO_NAME", "Ubuntu")]),
            &mut runner,
        )
        .expect("wsl image");

        assert_eq!(image.mime_type, "image/png");
        assert_eq!(image.bytes, vec![7, 7, 7]);
    }

    #[test]
    fn converts_bmp_clipboard_image_to_png_like_pi_wslg() {
        struct BmpRunner;

        impl ClipboardImageRunner for BmpRunner {
            fn run(
                &mut self,
                command: &str,
                args: &[String],
                _timeout_ms: u64,
                _max_buffer_bytes: usize,
            ) -> ClipboardImageCommandOutput {
                match (command, args.first().map(String::as_str)) {
                    ("wl-paste", Some("--list-types")) => {
                        ClipboardImageCommandOutput::success(b"image/bmp\n".to_vec())
                    }
                    ("wl-paste", Some("--type")) if args.iter().any(|arg| arg == "image/bmp") => {
                        ClipboardImageCommandOutput::success(tiny_bmp_1x1_red_24bpp())
                    }
                    _ => ClipboardImageCommandOutput::failure(),
                }
            }

            fn read_file(&mut self, _path: &Path) -> std::io::Result<Vec<u8>> {
                Ok(Vec::new())
            }

            fn read_to_string(&mut self, _path: &Path) -> std::io::Result<String> {
                Ok(String::new())
            }

            fn write_file(&mut self, _path: &Path, _bytes: &[u8]) -> std::io::Result<()> {
                Ok(())
            }

            fn remove_file(&mut self, _path: &Path) -> std::io::Result<()> {
                Ok(())
            }

            fn next_temp_png_path(&mut self) -> PathBuf {
                PathBuf::from("/tmp/unused.png")
            }

            fn native_image(&mut self) -> Option<Vec<u8>> {
                None
            }
        }

        let mut runner = BmpRunner;
        let image = read_clipboard_image_with_runner(
            "linux",
            HashMap::from([("WAYLAND_DISPLAY", "wayland-0")]),
            false,
            &mut runner,
        )
        .expect("converted image");

        assert_eq!(image.mime_type, "image/png");
        assert_eq!(
            &image.bytes[..8],
            &[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]
        );
        let dimensions = crate::utils::image_dimensions::get_png_dimensions(&image.bytes).unwrap();
        assert_eq!(dimensions.width_px, 1);
        assert_eq!(dimensions.height_px, 1);
    }

    #[test]
    fn writes_clipboard_image_to_temp_file_for_editor_insert_like_pi() {
        struct PasteRunner {
            dir: PathBuf,
            written: Vec<(PathBuf, Vec<u8>)>,
        }

        impl ClipboardImageRunner for PasteRunner {
            fn run(
                &mut self,
                _command: &str,
                _args: &[String],
                _timeout_ms: u64,
                _max_buffer_bytes: usize,
            ) -> ClipboardImageCommandOutput {
                ClipboardImageCommandOutput::failure()
            }

            fn read_file(&mut self, _path: &Path) -> std::io::Result<Vec<u8>> {
                Ok(Vec::new())
            }

            fn read_to_string(&mut self, _path: &Path) -> std::io::Result<String> {
                Ok(String::new())
            }

            fn remove_file(&mut self, _path: &Path) -> std::io::Result<()> {
                Ok(())
            }

            fn next_temp_png_path(&mut self) -> PathBuf {
                self.dir.join("unused.png")
            }

            fn native_image(&mut self) -> Option<Vec<u8>> {
                Some(vec![0x89, b'P', b'N', b'G'])
            }

            fn write_file(&mut self, path: &Path, bytes: &[u8]) -> std::io::Result<()> {
                self.written.push((path.to_path_buf(), bytes.to_vec()));
                Ok(())
            }
        }

        let dir = temp_dir("clipboard-paste-plan");
        let mut runner = PasteRunner {
            dir: dir.clone(),
            written: Vec::new(),
        };
        let path = write_clipboard_image_for_editor_insert(
            "darwin",
            HashMap::<&str, &str>::new(),
            false,
            &dir,
            "fixed-id",
            &mut runner,
        )
        .expect("paste path");

        assert_eq!(path, dir.join("pi-clipboard-fixed-id.png"));
        assert_eq!(runner.written, vec![(path, vec![0x89, b'P', b'N', b'G'])]);
    }

    #[test]
    fn editor_insert_write_detects_wsl_like_pi_read_clipboard_image() {
        struct AutoWslPasteRunner {
            temp_path: PathBuf,
            written: Vec<(PathBuf, Vec<u8>)>,
        }

        impl ClipboardImageRunner for AutoWslPasteRunner {
            fn run(
                &mut self,
                command: &str,
                _args: &[String],
                _timeout_ms: u64,
                _max_buffer_bytes: usize,
            ) -> ClipboardImageCommandOutput {
                match command {
                    "wl-paste" | "xclip" => ClipboardImageCommandOutput::failure(),
                    "wslpath" => ClipboardImageCommandOutput::success(b"C:\\clip.png\n".to_vec()),
                    "powershell.exe" => ClipboardImageCommandOutput::success(b"ok\n".to_vec()),
                    _ => ClipboardImageCommandOutput::failure(),
                }
            }

            fn read_file(&mut self, path: &Path) -> std::io::Result<Vec<u8>> {
                assert_eq!(path, self.temp_path.as_path());
                Ok(vec![8, 8, 8])
            }

            fn read_to_string(&mut self, _path: &Path) -> std::io::Result<String> {
                Ok(String::new())
            }

            fn write_file(&mut self, path: &Path, bytes: &[u8]) -> std::io::Result<()> {
                self.written.push((path.to_path_buf(), bytes.to_vec()));
                Ok(())
            }

            fn remove_file(&mut self, _path: &Path) -> std::io::Result<()> {
                Ok(())
            }

            fn next_temp_png_path(&mut self) -> PathBuf {
                self.temp_path.clone()
            }

            fn native_image(&mut self) -> Option<Vec<u8>> {
                panic!("native clipboard should not be used for WSL paste image");
            }
        }

        let dir = temp_dir("clipboard-paste-auto-wsl");
        let mut runner = AutoWslPasteRunner {
            temp_path: PathBuf::from("/tmp/pi-wsl-auto-paste.png"),
            written: Vec::new(),
        };
        let path = write_clipboard_image_for_editor_insert_auto(
            "linux",
            HashMap::from([("WSL_DISTRO_NAME", "Ubuntu")]),
            &dir,
            "auto-wsl",
            &mut runner,
        )
        .expect("paste path");

        assert_eq!(path, dir.join("pi-clipboard-auto-wsl.png"));
        assert_eq!(runner.written, vec![(path, vec![8, 8, 8])]);
    }

    #[test]
    fn clipboard_image_editor_insert_returns_none_when_no_image_like_pi() {
        let dir = temp_dir("clipboard-paste-empty");
        let mut runner = FakeClipboardImageRunner::default();

        let path = write_clipboard_image_for_editor_insert(
            "darwin",
            HashMap::<&str, &str>::new(),
            false,
            &dir,
            "empty",
            &mut runner,
        );

        assert_eq!(path, None);
    }

    #[test]
    fn selects_preferred_image_mime_type_like_pi_clipboard_image() {
        assert_eq!(
            select_preferred_image_mime_type([" text/plain ", "image/webp", "image/png"]),
            Some("image/png".to_string())
        );
        assert_eq!(
            select_preferred_image_mime_type(["image/bmp; format=dib", "text/plain"]),
            Some("image/bmp; format=dib".to_string())
        );
        assert_eq!(select_preferred_image_mime_type(["text/plain", ""]), None);
    }

    fn tiny_bmp_1x1_red_24bpp() -> Vec<u8> {
        let mut buffer = vec![0; 58];

        buffer[0..2].copy_from_slice(b"BM");
        let file_size = buffer.len() as u32;
        buffer[2..6].copy_from_slice(&file_size.to_le_bytes());
        buffer[10..14].copy_from_slice(&54u32.to_le_bytes());
        buffer[14..18].copy_from_slice(&40u32.to_le_bytes());
        buffer[18..22].copy_from_slice(&1i32.to_le_bytes());
        buffer[22..26].copy_from_slice(&1i32.to_le_bytes());
        buffer[26..28].copy_from_slice(&1u16.to_le_bytes());
        buffer[28..30].copy_from_slice(&24u16.to_le_bytes());
        buffer[34..38].copy_from_slice(&4u32.to_le_bytes());

        buffer[54] = 0x00;
        buffer[55] = 0x00;
        buffer[56] = 0xff;
        buffer[57] = 0x00;

        buffer
    }

    fn temp_dir(label: &str) -> PathBuf {
        let id = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("pm-agent-{label}-{id}"));
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir
    }
}
