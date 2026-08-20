// engine.rs — PRNG, terminal I/O, fire simulation loop.

use crate::{config::AnimSettings, palettes::Palette, ESC};
use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

// ── Simulation constants ──────────────────────────────────────────────

const MAX_HEAT: u8 = 36;
const STEPS_PER_FRAME: u32 = 2;
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

/// Render a frame with fire-erase semantics:
/// - Cells with heat > 0  → fire color (overlays everything)
/// - Cells with heat == 0 and burned → default bg (already erased by fire)
/// - Cells with heat == 0 and NOT burned → skipped (original terminal content shows through)
fn render(
    buf: &mut String,
    grid: &[u8],
    burned: &[bool],
    cols: usize,
    rows: usize,
    palette: &Palette,
) {
    use std::fmt::Write as _;
    buf.clear();

    let mut last_color: Option<CellColor> = None;
    let mut need_move = true;
    let mut write_col = 0usize;
    let mut write_row = 0usize;

    for y in 0..rows {
        for x in 0..cols {
            let idx = y * cols + x;
            let heat = grid[idx];

            if heat == 0 && !burned[idx] {
                // Unburned, no heat — leave original terminal content alone.
                need_move = true;
                continue;
            }

            // This cell needs to be written. Position if needed.
            if need_move || write_row != y || write_col != x {
                let _ = write!(buf, "{ESC}[{};{}H", y + 1, x + 1);
                last_color = None; // re-emit color after move
                need_move = false;
                write_row = y;
                write_col = x;
            }

            let color = if heat > 0 {
                let (r, g, b) = palette[heat as usize];
                CellColor::Rgb(r, g, b)
            } else {
                // burned, heat == 0 — clear to default bg
                CellColor::Default
            };

            if last_color != Some(color) {
                match color {
                    CellColor::Default => {
                        let _ = write!(buf, "{ESC}[49m");
                    }
                    CellColor::Rgb(r, g, b) => {
                        let _ = write!(buf, "{ESC}[48;2;{r};{g};{b}m");
                    }
                }
                last_color = Some(color);
            }

            buf.push(' ');
            write_col += 1;
        }
    }

    let _ = write!(buf, "{ESC}[0m");
}

/// Final pass: erase any cells that were never touched by fire.
fn clear_unburned(buf: &mut String, burned: &[bool], cols: usize, rows: usize) {
    use std::fmt::Write as _;
    buf.clear();
    let _ = write!(buf, "{ESC}[49m");
    let mut need_move = true;
    let mut write_col = 0usize;
    let mut write_row = 0usize;

    for y in 0..rows {
        for x in 0..cols {
            let idx = y * cols + x;
            if burned[idx] {
                need_move = true;
                continue;
            }
            if need_move || write_row != y || write_col != x {
                let _ = write!(buf, "{ESC}[{};{}H", y + 1, x + 1);
                need_move = false;
                write_row = y;
                write_col = x;
            }
            buf.push(' ');
            write_col += 1;
        }
    }
    let _ = write!(buf, "{ESC}[0m");
}

fn resize_grid(cols: usize, rows: usize, direction: u8) -> Vec<u8> {
    let mut grid = vec![0u8; cols * rows];
    match direction {
        1 => {
            // Top → Bottom: seed top row
            for x in 0..cols {
                grid[x] = MAX_HEAT;
            }
        }
        2 => {
            // Left → Right: seed left column
            for y in 0..rows {
                grid[y * cols] = MAX_HEAT;
            }
        }
        3 => {
            // Right → Left: seed right column
            for y in 0..rows {
                grid[y * cols + (cols - 1)] = MAX_HEAT;
            }
        }
        _ => {
            // Bottom → Top (default): seed bottom row
            for x in 0..cols {
                grid[(rows - 1) * cols + x] = MAX_HEAT;
            }
        }
    }
    grid
}

// ── Burn loop ─────────────────────────────────────────────────────────

pub fn burn(palette: &Palette, settings: &AnimSettings, interrupted: Arc<AtomicBool>) {
    let (mut cols, mut rows) = terminal_size();
    let mut grid = resize_grid(cols, rows, settings.direction);
    // burned[i] = true once cell i has ever had heat > 0.
    // Burned cells are actively cleared; unburned cells are left untouched
    // so any existing terminal text shows through until fire reaches it.
    let mut burned = vec![false; cols * rows];
    let mut rng = Rng::new();

    let stdout = io::stdout();
    let mut out = stdout.lock();
    // Hide cursor only — do NOT clear the screen so existing content remains visible.
    let _ = write!(out, "{ESC}[?25l");

    let start = Instant::now();
    let max_duration: Duration = Duration::from_secs_f32(settings.duration);
    let source_cool_at = max_duration.mul_f32(settings.flames_duration);
    let mut frame = String::with_capacity(cols * rows * 8);
    let frame_delay = Duration::from_millis(1000 / settings.fps.max(1) as u64);
    let direction = settings.direction;

    loop {
        if interrupted.load(Ordering::Relaxed) {
            break;
        }

        let elapsed = start.elapsed();
        if elapsed > max_duration {
            break;
        }

        // Live resize
        let (new_cols, new_rows) = terminal_size();
        if new_cols != cols || new_rows != rows {
            cols = new_cols;
            rows = new_rows;
            grid = resize_grid(cols, rows, direction);
            burned = vec![false; cols * rows];
            frame.reserve(cols * rows * 8);
        }

        // Refresh source edge while below cool-down threshold
        if elapsed <= source_cool_at {
            match direction {
                1 => {
                    for x in 0..cols { grid[x] = MAX_HEAT; }
                }
                2 => {
                    for y in 0..rows { grid[y * cols] = MAX_HEAT; }
                }
                3 => {
                    for y in 0..rows { grid[y * cols + (cols - 1)] = MAX_HEAT; }
                }
                _ => {
                    for x in 0..cols { grid[(rows - 1) * cols + x] = MAX_HEAT; }
                }
            }
        }

        // Propagation steps
        for _ in 0..STEPS_PER_FRAME {
            match direction {
                1 => {
                    // Top → Bottom: row y radiates into row y+1
                    for x in 0..cols {
                        for y in 0..rows - 1 {
                            let above = grid[y * cols + x];
                            let decay = match settings.height {
                                0 => rng.range(1, 4),
                                1 => rng.range(0, 3),
                                2 => rng.range(0, 2),
                                3 => rng.range(0, 1),
                                _ => rng.range(0, 3),
                            };
                            let drift = match settings.wind {
                                -2 => rng.range(-2, 0),
                                -1 => rng.range(-1, 0),
                                0  => rng.range(-1, 1),
                                1  => rng.range(0, 1),
                                2  => rng.range(0, 2),
                                _  => rng.range(-1, 1),
                            };
                            let nx = (x as i32 + drift).clamp(0, cols as i32 - 1) as usize;
                            let new_val = (above as i32 - decay).max(0) as u8;
                            grid[(y + 1) * cols + nx] = new_val;
                        }
                    }
                    if elapsed > source_cool_at {
                        for x in 0..cols {
                            let dec = rng.range(2, 6);
                            grid[x] = (grid[x] as i32 - dec).max(0) as u8; // top row
                        }
                    }
                }
                2 => {
                    // Left → Right: col x radiates into col x+1; drift shifts row
                    for y in 0..rows {
                        for x in 0..cols - 1 {
                            let left = grid[y * cols + x];
                            let decay = match settings.height {
                                0 => rng.range(1, 4),
                                1 => rng.range(0, 3),
                                2 => rng.range(0, 2),
                                3 => rng.range(0, 1),
                                _ => rng.range(0, 3),
                            };
                            // wind causes vertical drift for horizontal fire
                            let drift = match settings.wind {
                                -2 => rng.range(-2, 0),
                                -1 => rng.range(-1, 0),
                                0  => rng.range(-1, 1),
                                1  => rng.range(0, 1),
                                2  => rng.range(0, 2),
                                _  => rng.range(-1, 1),
                            };
                            let ny = (y as i32 + drift).clamp(0, rows as i32 - 1) as usize;
                            let new_val = (left as i32 - decay).max(0) as u8;
                            grid[ny * cols + (x + 1)] = new_val;
                        }
                    }
                    if elapsed > source_cool_at {
                        for y in 0..rows {
                            let dec = rng.range(2, 6);
                            grid[y * cols] = (grid[y * cols] as i32 - dec).max(0) as u8; // left col
                        }
                    }
                }
                3 => {
                    // Right → Left: col x radiates into col x-1; drift shifts row
                    for y in 0..rows {
                        for x in (1..cols).rev() {
                            let right = grid[y * cols + x];
                            let decay = match settings.height {
                                0 => rng.range(1, 4),
                                1 => rng.range(0, 3),
                                2 => rng.range(0, 2),
                                3 => rng.range(0, 1),
                                _ => rng.range(0, 3),
                            };
                            let drift = match settings.wind {
                                -2 => rng.range(-2, 0),
                                -1 => rng.range(-1, 0),
                                0  => rng.range(-1, 1),
                                1  => rng.range(0, 1),
                                2  => rng.range(0, 2),
                                _  => rng.range(-1, 1),
                            };
                            let ny = (y as i32 + drift).clamp(0, rows as i32 - 1) as usize;
                            let new_val = (right as i32 - decay).max(0) as u8;
                            grid[ny * cols + (x - 1)] = new_val;
                        }
                    }
                    if elapsed > source_cool_at {
                        for y in 0..rows {
                            let idx = y * cols + (cols - 1);
                            let dec = rng.range(2, 6);
                            grid[idx] = (grid[idx] as i32 - dec).max(0) as u8; // right col
                        }
                    }
                }
                _ => {
                    // Bottom → Top: row y radiates into row y-1 (default)
                    for x in 0..cols {
                        for y in 1..rows {
                            let below = grid[y * cols + x];
                            let decay = match settings.height {
                                0 => rng.range(1, 4),
                                1 => rng.range(0, 3),
                                2 => rng.range(0, 2),
                                3 => rng.range(0, 1),
                                _ => rng.range(0, 3),
                            };
                            let drift = match settings.wind {
                                -2 => rng.range(-2, 0),
                                -1 => rng.range(-1, 0),
                                0  => rng.range(-1, 1),
                                1  => rng.range(0, 1),
                                2  => rng.range(0, 2),
                                _  => rng.range(-1, 1),
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
                            grid[idx] = (grid[idx] as i32 - dec).max(0) as u8; // bottom row
                        }
                    }
                }
            }
        }

        // Update burned mask: any cell with heat > 0 is permanently marked.
        for (i, &h) in grid.iter().enumerate() {
            if h > 0 {
                burned[i] = true;
            }
        }

        render(&mut frame, &grid, &burned, cols, rows, palette);
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

    // Final pass: erase any cells the fire never touched.
    clear_unburned(&mut frame, &burned, cols, rows);
    let _ = out.write_all(frame.as_bytes());
    let _ = out.flush();

    let _ = write!(out, "{ESC}[?25h"); // always restore cursor
}
