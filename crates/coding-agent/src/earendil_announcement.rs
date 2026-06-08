pub const BLOG_URL: &str = "https://mariozechner.at/posts/2026-04-08-ive-sold-out/";
pub const IMAGE_FILENAME: &str = "clankolas.png";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EarendilAnnouncementLine {
    Border,
    AccentTitle(String),
    Blank,
    Muted(String),
    Link(String),
    Image {
        base64_data: String,
        mime_type: String,
        filename: String,
        max_width_cells: u32,
    },
}

pub struct EarendilAnnouncement<F>
where
    F: FnMut() -> Option<String>,
{
    image_loader: F,
    attempted_image_load: bool,
    cached_image_base64: Option<String>,
    cached_lines: Option<Vec<EarendilAnnouncementLine>>,
}

impl<F> EarendilAnnouncement<F>
where
    F: FnMut() -> Option<String>,
{
    pub fn new(image_loader: F) -> Self {
        Self {
            image_loader,
            attempted_image_load: false,
            cached_image_base64: None,
            cached_lines: None,
        }
    }

    pub fn render(&mut self) -> Vec<EarendilAnnouncementLine> {
        if let Some(lines) = &self.cached_lines {
            return lines.clone();
        }

        let image_base64 = self.load_image_base64();
        let mut lines = vec![
            EarendilAnnouncementLine::Border,
            EarendilAnnouncementLine::AccentTitle("pi has joined Earendil".to_string()),
            EarendilAnnouncementLine::Blank,
            EarendilAnnouncementLine::Muted("Read the blog post:".to_string()),
            EarendilAnnouncementLine::Link(BLOG_URL.to_string()),
            EarendilAnnouncementLine::Blank,
        ];

        if let Some(base64_data) = image_base64 {
            lines.push(EarendilAnnouncementLine::Image {
                base64_data,
                mime_type: "image/png".to_string(),
                filename: IMAGE_FILENAME.to_string(),
                max_width_cells: 56,
            });
            lines.push(EarendilAnnouncementLine::Blank);
        }

        lines.push(EarendilAnnouncementLine::Border);
        self.cached_lines = Some(lines.clone());
        lines
    }

    fn load_image_base64(&mut self) -> Option<String> {
        if self.attempted_image_load {
            return self.cached_image_base64.clone();
        }

        self.attempted_image_load = true;
        self.cached_image_base64 = (self.image_loader)();
        self.cached_image_base64.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn earendil_announcement_renders_static_lines_without_image() {
        let mut attempts = 0;
        let mut announcement = EarendilAnnouncement::new(|| {
            attempts += 1;
            None
        });

        let lines = announcement.render();
        let second = announcement.render();

        assert_eq!(attempts, 1);
        assert_eq!(lines, second);
        assert_eq!(
            lines,
            vec![
                EarendilAnnouncementLine::Border,
                EarendilAnnouncementLine::AccentTitle("pi has joined Earendil".to_string()),
                EarendilAnnouncementLine::Blank,
                EarendilAnnouncementLine::Muted("Read the blog post:".to_string()),
                EarendilAnnouncementLine::Link(BLOG_URL.to_string()),
                EarendilAnnouncementLine::Blank,
                EarendilAnnouncementLine::Border,
            ]
        );
    }

    #[test]
    fn earendil_announcement_renders_image_when_loader_succeeds() {
        let mut announcement = EarendilAnnouncement::new(|| Some("base64-png".to_string()));

        let lines = announcement.render();

        assert!(lines.contains(&EarendilAnnouncementLine::Image {
            base64_data: "base64-png".to_string(),
            mime_type: "image/png".to_string(),
            filename: IMAGE_FILENAME.to_string(),
            max_width_cells: 56,
        }));
        assert_eq!(
            lines.last(),
            Some(&EarendilAnnouncementLine::Border),
            "bottom border stays last"
        );
        assert_eq!(
            lines
                .iter()
                .filter(|line| matches!(line, EarendilAnnouncementLine::Blank))
                .count(),
            3
        );
    }

    #[test]
    fn earendil_announcement_caches_successful_image_load() {
        let mut attempts = 0;
        let mut announcement = EarendilAnnouncement::new(|| {
            attempts += 1;
            Some(format!("image-{attempts}"))
        });

        let first = announcement.render();
        let second = announcement.render();

        assert_eq!(attempts, 1);
        assert_eq!(first, second);
        assert!(second.contains(&EarendilAnnouncementLine::Image {
            base64_data: "image-1".to_string(),
            mime_type: "image/png".to_string(),
            filename: IMAGE_FILENAME.to_string(),
            max_width_cells: 56,
        }));
    }
}
