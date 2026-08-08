// engine.rs — PRNG, terminal I/O, fire simulation loop.

use crate::{config::AnimSettings, palettes::Palette, ESC};
use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

// ── Simulation constants ──────────────────────────────────────────────

const MAX_HEAT: u8 = 36;
const STEPS_PER_FRAME: u32 = 2;
const MAX_DURATION: Duration = Duration::from_millis(2200);
const SOURCE_COOL_START: f32 = 0.38;
const DIE_OUT_THRESHOLD: u8 = 2;

// ── PRNG (xorshift64*) ────────────────────────────────────────────────

pub struct Rng(u64);

impl Default for Rng {
    fn default() -> Self {
        Self::new()
    }
}

impl Rng {
    pub fn new() -> Self {
        let d = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
        let seed = (d.as_secs().wrapping_mul(6364136223846793005) ^ d.subsec_nanos() as u64) | 1;
        Rng(seed)
    }

    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    pub fn range(&mut self, lo: i32, hi: i32) -> i32 {
        let span = (hi - lo + 1) as u64;
        lo + (self.next_u64() % span) as i32
    }
}

// ── Terminal size ─────────────────────────────────────────────────────

pub fn terminal_size() -> (usize, usize) {
    #[cfg(unix)]
    unsafe {
        let mut ws: libc::winsize = std::mem::zeroed();
        if libc::ioctl(libc::STDOUT_FILENO, libc::TIOCGWINSZ, &mut ws) == 0
            && ws.ws_col > 0
            && ws.ws_row > 0
        {
            return (ws.ws_col as usize, ws.ws_row as usize);
        }
    }
    #[cfg(windows)]
    if let Some((w, h)) = crate::win::terminal_size() {
        return (w, h);
    }
    (80, 24)
}

// ── Renderer ──────────────────────────────────────────────────────────

#[derive(PartialEq, Clone, Copy)]
enum CellColor {
    Default,
    Rgb(u8, u8, u8),
}

fn render(buf: &mut String, grid: &[u8], cols: usize, rows: usize, palette: &Palette) {
    buf.clear();
    buf.push_str(ESC);
    buf.push_str("[H");

    let mut last: Option<CellColor> = None;
    for y in 0..rows {
        for x in 0..cols {
            let heat = grid[y * cols + x];
            let color = if heat == 0 {
                CellColor::Default
            } else {
                let (r, g, b) = palette[heat as usize];
                CellColor::Rgb(r, g, b)
            };

            if last != Some(color) {
                use std::fmt::Write as _;
                match color {
                    CellColor::Default => {
                        let _ = write!(buf, "{ESC}[49m");
                    }
                    CellColor::Rgb(r, g, b) => {
                        let _ = write!(buf, "{ESC}[48;2;{r};{g};{b}m");
                    }
                }
                last = Some(color);
            }
            buf.push(' ');
        }
        buf.push('\n');
    }
}

fn resize_grid(cols: usize, rows: usize, top_down: bool) -> Vec<u8> {
    let mut grid = vec![0u8; cols * rows];
    let source_row = if top_down { 0 } else { rows - 1 };
    for x in 0..cols {
        grid[source_row * cols + x] = MAX_HEAT;
    }
    grid
}

// ── Burn loop ─────────────────────────────────────────────────────────

pub fn burn(palette: &Palette, settings: &AnimSettings, interrupted: Arc<AtomicBool>) {
    let (mut cols, mut rows) = terminal_size();
    let mut grid = resize_grid(cols, rows, settings.direction);
    let mut rng = Rng::new();

    let stdout = io::stdout();
    let mut out = stdout.lock();
    // Hide cursor + full clear (screen + scrollback) so no residual content
    let _ = write!(out, "{ESC}[?25l{ESC}[0m{ESC}[H{ESC}[2J{ESC}[3J");

    let start = Instant::now();
    let source_cool_at = MAX_DURATION.mul_f32(SOURCE_COOL_START);
    let mut frame = String::with_capacity(cols * rows * 8);
    let frame_delay = Duration::from_millis(1000 / settings.fps.max(1) as u64);
    let top_down = settings.direction;

    loop {
        if interrupted.load(Ordering::Relaxed) {
            break;
        }

        let elapsed = start.elapsed();
        if elapsed > MAX_DURATION {
            break;
        }

        // Live resize
        let (new_cols, new_rows) = terminal_size();
        if new_cols != cols || new_rows != rows {
            cols = new_cols;
            rows = new_rows;
            grid = resize_grid(cols, rows, top_down);
            frame.reserve(cols * rows * 8);
        }

        // Refresh source row while below cool-down threshold
        let source_row = if top_down { 0 } else { rows - 1 };
        if elapsed <= source_cool_at {
            for x in 0..cols {
                grid[source_row * cols + x] = MAX_HEAT;
            }
        }

        // Propagation steps
        for _ in 0..STEPS_PER_FRAME {
            if top_down {
                // Heat flows downward: row y radiates into row y+1
                for x in 0..cols {
                    for y in 0..rows - 1 {
                        let above = grid[y * cols + x];

                        let decay = match settings.height {
                            0 => rng.range(1, 4), // Low
                            1 => rng.range(0, 3), // Medium
                            2 => rng.range(0, 2), // High
                            3 => rng.range(0, 1), // Extreme
                            _ => rng.range(0, 3),
                        };

                        let drift = match settings.wind {
                            -2 => rng.range(-2, 0), // Strong Left
                            -1 => rng.range(-1, 0), // Gentle Left
                            0 => rng.range(-1, 1),  // None
                            1 => rng.range(0, 1),   // Gentle Right
                            2 => rng.range(0, 2),   // Strong Right
                            _ => rng.range(-1, 1),
                        };

                        let nx = (x as i32 + drift).clamp(0, cols as i32 - 1) as usize;
                        let new_val = (above as i32 - decay).max(0) as u8;
                        grid[(y + 1) * cols + nx] = new_val;
                    }
                }

                if elapsed > source_cool_at {
                    for x in 0..cols {
                        let idx = x; // top row (row 0)
                        let dec = rng.range(2, 6);
                        grid[idx] = (grid[idx] as i32 - dec).max(0) as u8;
                    }
                }
            } else {
                // Heat flows upward: row y radiates into row y-1 (original behaviour)
                for x in 0..cols {
                    for y in 1..rows {
                        let below = grid[y * cols + x];

                        let decay = match settings.height {
                            0 => rng.range(1, 4), // Low
                            1 => rng.range(0, 3), // Medium
                            2 => rng.range(0, 2), // High
                            3 => rng.range(0, 1), // Extreme
                            _ => rng.range(0, 3),
                        };

                        let drift = match settings.wind {
                            -2 => rng.range(-2, 0), // Strong Left
                            -1 => rng.range(-1, 0), // Gentle Left
                            0 => rng.range(-1, 1),  // None
                            1 => rng.range(0, 1),   // Gentle Right
                            2 => rng.range(0, 2),   // Strong Right
                            _ => rng.range(-1, 1),
                        };

                        let nx = (x as i32 + drift).clamp(0, cols as i32 - 1) as usize;
                        let new_val = (below as i32 - decay).max(0) as u8;
                        grid[(y - 1) * cols + nx] = new_val;
                    }
                }

                if elapsed > source_cool_at {
                    for x in 0..cols {
                        let idx = (rows - 1) * cols + x;
                        let dec = rng.range(2, 6);
                        grid[idx] = (grid[idx] as i32 - dec).max(0) as u8;
                    }
                }
            }
        }

        render(&mut frame, &grid, cols, rows, palette);
        let _ = out.write_all(frame.as_bytes());
        let _ = out.flush();

        if elapsed > source_cool_at {
            let peak = grid.iter().copied().max().unwrap_or(0);
            if peak < DIE_OUT_THRESHOLD {
                break;
            }
        }

        std::thread::sleep(frame_delay);
    }

    let _ = write!(out, "{ESC}[?25h"); // always restore cursor
}
