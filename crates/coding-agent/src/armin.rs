pub const ARMIN_WIDTH: usize = 31;
pub const ARMIN_HEIGHT: usize = 36;
pub const ARMIN_DISPLAY_HEIGHT: usize = ARMIN_HEIGHT.div_ceil(2);

const ARMIN_BITS: &[u8] = &[
    0xff, 0xff, 0xff, 0x7f, 0xff, 0xf0, 0xff, 0x7f, 0xff, 0xed, 0xff, 0x7f, 0xff, 0xdb, 0xff, 0x7f,
    0xff, 0xb7, 0xff, 0x7f, 0xff, 0x77, 0xfe, 0x7f, 0x3f, 0xf8, 0xfe, 0x7f, 0xdf, 0xff, 0xfe, 0x7f,
    0xdf, 0x3f, 0xfc, 0x7f, 0x9f, 0xc3, 0xfb, 0x7f, 0x6f, 0xfc, 0xf4, 0x7f, 0xf7, 0x0f, 0xf7, 0x7f,
    0xf7, 0xff, 0xf7, 0x7f, 0xf7, 0xff, 0xe3, 0x7f, 0xf7, 0x07, 0xe8, 0x7f, 0xef, 0xf8, 0x67, 0x70,
    0x0f, 0xff, 0xbb, 0x6f, 0xf1, 0x00, 0xd0, 0x5b, 0xfd, 0x3f, 0xec, 0x53, 0xc1, 0xff, 0xef, 0x57,
    0x9f, 0xfd, 0xee, 0x5f, 0x9f, 0xfc, 0xae, 0x5f, 0x1f, 0x78, 0xac, 0x5f, 0x3f, 0x00, 0x50, 0x6c,
    0x7f, 0x00, 0xdc, 0x77, 0xff, 0xc0, 0x3f, 0x78, 0xff, 0x01, 0xf8, 0x7f, 0xff, 0x03, 0x9c, 0x78,
    0xff, 0x07, 0x8c, 0x7c, 0xff, 0x0f, 0xce, 0x78, 0xff, 0xff, 0xcf, 0x7f, 0xff, 0xff, 0xcf, 0x78,
    0xff, 0xff, 0xdf, 0x78, 0xff, 0xff, 0xdf, 0x7d, 0xff, 0xff, 0x3f, 0x7e, 0xff, 0xff, 0xff, 0x7f,
];

const BYTES_PER_ROW: usize = ARMIN_WIDTH.div_ceil(8);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArminEffect {
    Typewriter,
    Scanline,
    Fade {
        positions: Vec<(usize, usize)>,
    },
    Crt,
    Dissolve {
        positions: Vec<(usize, usize)>,
        noise: Vec<Vec<char>>,
    },
}

pub struct ArminAnimation {
    effect: ArminEffect,
    final_grid: Vec<Vec<char>>,
    current_grid: Vec<Vec<char>>,
    cursor: usize,
    positions: Vec<(usize, usize)>,
    expansion: usize,
}

impl ArminAnimation {
    pub fn new(effect: ArminEffect) -> Self {
        let positions = match &effect {
            ArminEffect::Fade { positions } | ArminEffect::Dissolve { positions, .. } => {
                positions.clone()
            }
            ArminEffect::Typewriter | ArminEffect::Scanline | ArminEffect::Crt => Vec::new(),
        };
        let current_grid = match &effect {
            ArminEffect::Dissolve { noise, .. } => normalize_grid(noise),
            ArminEffect::Typewriter
            | ArminEffect::Scanline
            | ArminEffect::Fade { .. }
            | ArminEffect::Crt => empty_grid(),
        };
        Self {
            effect,
            final_grid: armin_final_grid(),
            current_grid,
            cursor: 0,
            positions,
            expansion: 0,
        }
    }

    pub fn current_grid(&self) -> &Vec<Vec<char>> {
        &self.current_grid
    }

    pub fn visible_cell_count(&self) -> usize {
        self.current_grid
            .iter()
            .flatten()
            .filter(|cell| **cell != ' ')
            .count()
    }

    pub fn revealed_cell_count(&self) -> usize {
        match self.effect {
            ArminEffect::Typewriter => self.cursor.min(ARMIN_WIDTH * ARMIN_DISPLAY_HEIGHT),
            ArminEffect::Scanline => self.cursor.min(ARMIN_DISPLAY_HEIGHT) * ARMIN_WIDTH,
            ArminEffect::Fade { .. } | ArminEffect::Dissolve { .. } => {
                self.cursor.min(self.positions.len())
            }
            ArminEffect::Crt => {
                let mid_row = ARMIN_DISPLAY_HEIGHT / 2;
                if self.expansion == 0 {
                    return 0;
                }
                let radius = self.expansion - 1;
                let top = mid_row.saturating_sub(radius);
                let bottom = (mid_row + radius).min(ARMIN_DISPLAY_HEIGHT - 1);
                (bottom - top + 1) * ARMIN_WIDTH
            }
        }
    }

    pub fn tick(&mut self) -> bool {
        match self.effect {
            ArminEffect::Typewriter => self.tick_typewriter(),
            ArminEffect::Scanline => self.tick_scanline(),
            ArminEffect::Fade { .. } => self.tick_positions(15),
            ArminEffect::Crt => self.tick_crt(),
            ArminEffect::Dissolve { .. } => self.tick_positions(20),
        }
    }

    pub fn render(&self, width: usize) -> Vec<String> {
        let padding = 1usize;
        let available_width = width.saturating_sub(padding);
        let mut lines = self
            .current_grid
            .iter()
            .map(|row| {
                let clipped = row.iter().take(available_width).collect::<String>();
                padded_line(&clipped, width, padding)
            })
            .collect::<Vec<_>>();
        lines.push(padded_line("ARMIN SAYS HI", width, padding));
        lines
    }

    fn tick_typewriter(&mut self) -> bool {
        let pixels_per_frame = 3;
        for _ in 0..pixels_per_frame {
            let row = self.cursor / ARMIN_WIDTH;
            let x = self.cursor % ARMIN_WIDTH;
            if row >= ARMIN_DISPLAY_HEIGHT {
                return true;
            }
            self.current_grid[row][x] = self.final_grid[row][x];
            self.cursor += 1;
        }
        false
    }

    fn tick_scanline(&mut self) -> bool {
        if self.cursor >= ARMIN_DISPLAY_HEIGHT {
            return true;
        }
        self.current_grid[self.cursor] = self.final_grid[self.cursor].clone();
        self.cursor += 1;
        false
    }

    fn tick_positions(&mut self, cells_per_frame: usize) -> bool {
        for _ in 0..cells_per_frame {
            let Some((row, x)) = self.positions.get(self.cursor).copied() else {
                return true;
            };
            if row < ARMIN_DISPLAY_HEIGHT && x < ARMIN_WIDTH {
                self.current_grid[row][x] = self.final_grid[row][x];
            }
            self.cursor += 1;
        }
        false
    }

    fn tick_crt(&mut self) -> bool {
        let mid_row = ARMIN_DISPLAY_HEIGHT / 2;
        self.current_grid = empty_grid();

        let top = mid_row.saturating_sub(self.expansion);
        let bottom = (mid_row + self.expansion).min(ARMIN_DISPLAY_HEIGHT - 1);
        for row in top..=bottom {
            self.current_grid[row] = self.final_grid[row].clone();
        }

        self.expansion += 1;
        self.expansion > ARMIN_DISPLAY_HEIGHT
    }
}

pub fn armin_pixel(x: usize, y: usize) -> bool {
    if x >= ARMIN_WIDTH || y >= ARMIN_HEIGHT {
        return false;
    }
    let byte_index = y * BYTES_PER_ROW + x / 8;
    let bit_index = x % 8;
    ((ARMIN_BITS[byte_index] >> bit_index) & 1) == 0
}

pub fn armin_char(x: usize, row: usize) -> char {
    let upper = armin_pixel(x, row * 2);
    let lower = armin_pixel(x, row * 2 + 1);
    match (upper, lower) {
        (true, true) => '█',
        (true, false) => '▀',
        (false, true) => '▄',
        (false, false) => ' ',
    }
}

pub fn armin_final_grid() -> Vec<Vec<char>> {
    (0..ARMIN_DISPLAY_HEIGHT)
        .map(|row| (0..ARMIN_WIDTH).map(|x| armin_char(x, row)).collect())
        .collect()
}

fn empty_grid() -> Vec<Vec<char>> {
    vec![vec![' '; ARMIN_WIDTH]; ARMIN_DISPLAY_HEIGHT]
}

fn normalize_grid(grid: &[Vec<char>]) -> Vec<Vec<char>> {
    (0..ARMIN_DISPLAY_HEIGHT)
        .map(|row| {
            let mut normalized = grid.get(row).cloned().unwrap_or_default();
            normalized.resize(ARMIN_WIDTH, ' ');
            normalized.truncate(ARMIN_WIDTH);
            normalized
        })
        .collect()
}

fn padded_line(content: &str, width: usize, padding: usize) -> String {
    let mut line = " ".repeat(padding.min(width));
    let remaining = width.saturating_sub(line.chars().count());
    line.push_str(&content.chars().take(remaining).collect::<String>());
    let visible = line.chars().count();
    if visible < width {
        line.push_str(&" ".repeat(width - visible));
    }
    line
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn armin_decodes_xbm_pixels_and_half_block_chars() {
        assert_eq!(ARMIN_WIDTH, 31);
        assert_eq!(ARMIN_HEIGHT, 36);
        assert_eq!(ARMIN_DISPLAY_HEIGHT, 18);
        assert!(!armin_pixel(0, 0));
        assert!(armin_pixel(9, 1));
        assert_eq!(armin_char(0, 0), ' ');
        assert_eq!(armin_char(9, 0), '▄');
    }

    #[test]
    fn armin_final_grid_has_expected_shape_and_content() {
        let grid = armin_final_grid();

        assert_eq!(grid.len(), ARMIN_DISPLAY_HEIGHT);
        assert!(grid.iter().all(|row| row.len() == ARMIN_WIDTH));
        assert!(grid.iter().flatten().any(|cell| *cell == '█'));
        assert_eq!(grid[0][9], '▄');
    }

    #[test]
    fn armin_scanline_reveals_one_row_per_tick() {
        let mut animation = ArminAnimation::new(ArminEffect::Scanline);

        assert_eq!(animation.visible_cell_count(), 0);
        assert!(!animation.tick());
        assert_eq!(animation.current_grid()[0], armin_final_grid()[0]);
        assert_eq!(animation.revealed_cell_count(), ARMIN_WIDTH);

        for _ in 1..ARMIN_DISPLAY_HEIGHT {
            animation.tick();
        }
        assert!(animation.tick());
        assert_eq!(animation.current_grid(), &armin_final_grid());
    }

    #[test]
    fn armin_typewriter_reveals_three_cells_per_tick() {
        let mut animation = ArminAnimation::new(ArminEffect::Typewriter);

        assert!(!animation.tick());
        assert_eq!(
            animation.current_grid()[0][0..3],
            armin_final_grid()[0][0..3]
        );
        assert_eq!(animation.revealed_cell_count(), 3);

        assert!(!animation.tick());
        assert_eq!(animation.revealed_cell_count(), 6);
    }

    #[test]
    fn armin_fade_reveals_fifteen_positions_per_tick() {
        let positions = sequential_positions();
        let mut animation = ArminAnimation::new(ArminEffect::Fade { positions });

        assert!(!animation.tick());

        assert_eq!(animation.revealed_cell_count(), 15);
        assert_eq!(
            animation.current_grid()[0][0..15],
            armin_final_grid()[0][0..15]
        );
    }

    #[test]
    fn armin_crt_expands_from_middle_row() {
        let mut animation = ArminAnimation::new(ArminEffect::Crt);
        let mid_row = ARMIN_DISPLAY_HEIGHT / 2;

        assert!(!animation.tick());

        assert_eq!(
            animation.current_grid()[mid_row],
            armin_final_grid()[mid_row]
        );
        assert_eq!(animation.revealed_cell_count(), ARMIN_WIDTH);

        assert!(!animation.tick());
        assert_eq!(
            animation.current_grid()[mid_row - 1],
            armin_final_grid()[mid_row - 1]
        );
        assert_eq!(
            animation.current_grid()[mid_row + 1],
            armin_final_grid()[mid_row + 1]
        );
    }

    #[test]
    fn armin_dissolve_starts_with_noise_and_resolves_twenty_positions_per_tick() {
        let positions = sequential_positions();
        let noise = vec![vec!['░'; ARMIN_WIDTH]; ARMIN_DISPLAY_HEIGHT];
        let mut animation = ArminAnimation::new(ArminEffect::Dissolve { positions, noise });

        assert_eq!(animation.current_grid()[0][0], '░');
        assert!(!animation.tick());

        assert_eq!(animation.revealed_cell_count(), 20);
        assert_eq!(
            animation.current_grid()[0][0..20],
            armin_final_grid()[0][0..20]
        );
        assert_eq!(animation.current_grid()[0][20], '░');
    }

    #[test]
    fn armin_render_clips_to_width_and_appends_message() {
        let mut animation = ArminAnimation::new(ArminEffect::Scanline);
        animation.tick();

        let lines = animation.render(10);

        assert_eq!(lines.len(), ARMIN_DISPLAY_HEIGHT + 1);
        assert!(lines.iter().all(|line| line.chars().count() == 10));
        assert!(lines.last().expect("message").starts_with(" ARMIN SA"));
    }

    fn sequential_positions() -> Vec<(usize, usize)> {
        (0..ARMIN_DISPLAY_HEIGHT)
            .flat_map(|row| (0..ARMIN_WIDTH).map(move |x| (row, x)))
            .collect()
    }
}
