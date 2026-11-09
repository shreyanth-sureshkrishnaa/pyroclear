// engine.rs — PRNG, terminal I/O, fire simulation loop, sweep effects
// (wave, vortex, explosion).

use crate::{config::AnimSettings, palettes::*, ESC};
use std::f32::consts::TAU;
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

// ── Effect dispatch ───────────────────────────────────────────────────

/// Every effect `--effect` accepts, besides `random`.
pub const EFFECTS: &[&str] = &["fire", "wave", "vortex", "explosion"];

pub fn burn(palette: &Palette, settings: &AnimSettings, interrupted: Arc<AtomicBool>) {
    let effect = if settings.effect == "random" {
        EFFECTS[(Rng::new().next_u64() % EFFECTS.len() as u64) as usize]
    } else {
        settings.effect.as_str()
    };
    let seed = Rng::new().next_u64() as i32; // new explosion layout and timing every run
    match Sweep::new(effect, palette, settings, seed) {
        Some(s) => sweep(&s, settings.fps, interrupted),
        None => fire(palette, settings, interrupted),
    }
}

/// A sweep effect (wave, vortex, explosion) configured from the settings,
/// ready to draw any frame: used by `burn` and by the TUI's live preview.
pub struct Sweep {
    field: Box<Field>,
    /// Colors it draws with: its own ramp when the default fire palette is active.
    pub palette: Palette,
    /// Length of one run, in seconds.
    pub duration: f32,
}

impl Sweep {
    /// `None` for `fire`, which is simulated rather than swept. `seed` picks
    /// the explosion layout and timing.
    pub fn new(effect: &str, palette: &Palette, settings: &AnimSettings, seed: i32) -> Option<Sweep> {
        // Fire colors fit neither water nor a galaxy: with the default palette,
        // those effects get their own ramp instead.
        let fire_palette = soften(&FIRE_PALETTE, SOFTEN_DESATURATE, SOFTEN_BRIGHTEN);
        let themed = |stops: &[(usize, (u8, u8, u8))]| {
            if *palette == fire_palette { ramp(stops) } else { *palette }
        };
        // Speed 1..=5 (Very Slow .. Very Fast) stretches or shrinks the run.
        let paced = |speed: u8| settings.duration * [1.8, 1.35, 1.0, 0.75, 0.55][speed.clamp(1, 5) as usize - 1];
        let (field, palette, duration): (Box<Field>, _, _) = match effect {
            "wave" => {
                let tilt = (settings.wave_angle.clamp(-30, 30) as f32).to_radians().tan();
                let field = move |c: &Cell, t, noise| wave(c, t, noise, tilt);
                (Box::new(field), themed(&SEA), paced(settings.wave_speed))
            }
            "vortex" => {
                let (outward, clockwise) = (settings.vortex_outward, settings.vortex_clockwise);
                let field = move |c: &Cell, t, noise| galaxy(c, t, noise, outward, clockwise);
                (Box::new(field), themed(&GALAXY), paced(settings.vortex_speed))
            }
            "explosion" => {
                let blasts = Blasts {
                    seed,
                    count: [1.0, 3.0, 6.0, 10.0, 16.0][settings.explosion_count.clamp(1, 5) as usize - 1],
                    rhythm: settings.explosion_rhythm.min(2),
                    debris: [0.0, 1.0, 2.2][settings.explosion_debris.min(2) as usize],
                };
                let field = move |c: &Cell, t, noise| explosions(c, t, noise, blasts);
                (Box::new(field), themed(&EXPLOSION), paced(settings.explosion_speed))
            }
            _ => return None,
        };
        Some(Sweep { field, palette, duration: duration.max(0.1) })
    }

    /// Writes the heat of frame `t` (0..=1 over the run) into `grid`.
    pub fn fill(&self, grid: &mut [u8], cols: usize, rows: usize, t: f32, rng: &mut Rng) {
        fill_field(grid, cols, rows, t, &*self.field, rng);
    }
}

// ── Sweep effects (wave, vortex, explosion) ───────────────────────────
//
// Unlike the fire, these are not simulated: each one is a pure function
// of (cell, time) returning an intensity in 0..=1, which is mapped onto
// the same heat → palette ramp and the same burned / unburned renderer.
// Cells keep their text until the effect first lights them; once lit they
// are erased when they go dark again. Every effect is fully dark at t = 0
// and at t = 1 and lights every cell in between (see the tests), so the
// final `clear_unburned` pass only matters when a run is cut short (Ctrl+C).

/// One screen cell, in normalized and in cell coordinates.
struct Cell {
    y: f32,    // 0 (top) ..= 1 (bottom)
    col: f32,  // column, in cells
    row: f32,  // row, in cells
    cols: f32, // screen width, in cells
    rows: f32, // screen height, in cells
}

/// (cell, t in 0..=1, noise in 0..1) → intensity in 0..=1.
type Field = dyn Fn(&Cell, f32, f32) -> f32;

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// Sea ramp for the wave: deep navy → blue → turquoise → foam white.
const SEA: [(usize, (u8, u8, u8)); 7] = [
    (1, (6, 28, 58)),
    (10, (10, 62, 112)),
    (18, (18, 104, 150)),
    (24, (40, 150, 180)),
    (28, (110, 200, 215)),
    (31, (190, 232, 238)),
    (36, (255, 255, 255)),
];

/// Galaxy ramp for the vortex, deep-space tones: near-black navy dust →
/// dark navy arms with a faint indigo touch → muted steel-blue highlights →
/// pale blue core → star white. Kept dark so the galaxy reads as space.
const GALAXY: [(usize, (u8, u8, u8)); 8] = [
    (1, (4, 6, 18)),
    (8, (8, 14, 40)),
    (14, (14, 24, 64)),
    (19, (26, 34, 92)),
    (24, (34, 54, 120)),
    (29, (60, 88, 156)),
    (33, (150, 175, 225)),
    (36, (240, 244, 255)),
];

/// Explosion ramp, brighter and yellower than the fire: dark red rim →
/// red-orange → orange → yellow → pale yellow-white core.
const EXPLOSION: [(usize, (u8, u8, u8)); 8] = [
    (1, (60, 8, 4)),
    (8, (140, 24, 8)),
    (14, (200, 52, 10)),
    (20, (238, 104, 16)),
    (26, (252, 160, 30)),
    (31, (255, 214, 70)),
    (34, (255, 240, 150)),
    (36, (255, 252, 225)),
];

/// 37-step palette interpolated between (heat index, color) stops.
fn ramp(stops: &[(usize, (u8, u8, u8))]) -> Palette {
    let mut p: Palette = [(0, 0, 0); 37];
    for w in stops.windows(2) {
        let ((i0, c0), (i1, c1)) = (w[0], w[1]);
        for (i, slot) in p.iter_mut().enumerate().take(i1 + 1).skip(i0) {
            let k = (i - i0) as f32 / (i1 - i0) as f32;
            let mix = |a: u8, b: u8| lerp(a as f32, b as f32, k).round() as u8;
            *slot = (mix(c0.0, c1.0), mix(c0.1, c1.1), mix(c0.2, c1.2));
        }
    }
    p
}

/// Cheap integer hash of a grid position → two stable values in 0..=1.
fn hash2(x: i32, y: i32) -> (f32, f32) {
    let h = (x as u32).wrapping_mul(0x8da6_b343) ^ (y as u32).wrapping_mul(0xd816_3841);
    let h = h.wrapping_mul(0x2c1b_3c6d) ^ (h >> 15);
    ((h & 0xffff) as f32 / 65535.0, (h >> 16) as f32 / 65535.0)
}

/// Distance to the nearest border of a drifting Voronoi net (F2 − F1 cellular
/// noise), in net-cell units: 0 on a border, larger towards cell centers.
/// One seed point per ~10 x 5 screen cells, each wandering inside its own
/// cell, on gently warped coordinates so borders curve instead of staying straight.
fn caustic_edge(col: f32, row: f32, t: f32) -> f32 {
    const SIZE: f32 = 10.0; // net cell size, in columns
    let (x, y) = (col / SIZE, row * 2.0 / SIZE); // rows are ~2x taller than columns
    // t spans the whole run (0..=1), so speeds are per run: ~1 drift cycle.
    let wx = x + 0.25 * (y * 1.7 + t * 4.0).sin();
    let wy = y + 0.25 * (x * 1.3 - t * 3.5).sin();
    let (ix, iy) = (wx.floor() as i32, wy.floor() as i32);

    let (mut f1, mut f2) = (f32::MAX, f32::MAX);
    for dy in -1..=1 {
        for dx in -1..=1 {
            let (cx, cy) = (ix + dx, iy + dy);
            // Two fixed phases for this cell's seed point.
            let (p, q) = hash2(cx, cy);
            let sx = cx as f32 + 0.5 + 0.4 * (t * 6.0 + p * TAU).sin();
            let sy = cy as f32 + 0.5 + 0.4 * (t * 5.0 + q * TAU).cos();
            let d = (wx - sx).hypot(wy - sy);
            if d < f1 {
                f2 = f1;
                f1 = d;
            } else if d < f2 {
                f2 = d;
            }
        }
    }
    f2 - f1
}

/// The sea rolls in from the top like a real wave: the crest rushes down
/// over the whole screen, holds, then washes back out through the top,
/// leaving the screen clear. Everything between the crest and the top is
/// ocean: a jagged foam crest, then grainy water with faint caustics.
/// Intensities index the sea ramp: ~0.25 deep water, ~0.6 wave face,
/// 0.85+ foam. `tilt` = tan(crest angle); > 0 puts the right side ahead.
fn wave(c: &Cell, t: f32, noise: f32, tilt: f32) -> f32 {
    const EDGE: f32 = 0.05; // max wobble of the crest
    // Two drifting swells give the crest an irregular, moving outline (|swell| <= 1).
    let swell = 0.6 * (c.col * 0.21 + t * 7.0).sin() + 0.4 * (c.col * 0.47 - t * 11.0).sin();
    // How far down the sea reaches, 0 (top) ..= 1 (bottom):
    // fast rush in (ease-out), hold, then a backwash that starts slow (ease-in).
    let reach = if t < 0.45 {
        let u = t / 0.45;
        1.0 - (1.0 - u) * (1.0 - u)
    } else if t < 0.6 {
        1.0
    } else {
        let v = (t - 0.6) / 0.4;
        1.0 - v * v
    };
    // Tilted crest: offset from the center column, in screen heights. Rows are
    // ~2x taller than columns, so a true angle needs half the slope in rows.
    let lean = tilt * (c.col + 0.5 - c.cols / 2.0) / (2.0 * c.rows);
    // The ends of a tilted crest sit up to `max_lean` above / below its middle,
    // so the run starts and ends that much further out to still cover everything.
    let max_lean = tilt.abs() * c.cols / (4.0 * c.rows);
    let margin = EDGE + 0.01 + max_lean;
    let front = lerp(-margin, 1.0 + margin, reach) + EDGE * swell + lean;
    if c.y > front {
        return 0.0;
    }

    let depth = (front - c.y) * c.rows; // rows behind the crest

    // Breaking crest: 1–2.5 rows of lacy foam, with holes where noise is low.
    let lace = (c.col * 0.9 + t * 13.0).sin() * (c.col * 0.37 - t * 5.0).sin();
    if depth < 1.8 + 0.7 * lace {
        return if noise < 0.18 { 0.74 } else { 0.88 + 0.12 * noise };
    }
    // Foam streaks trailing a few rows behind the crest.
    if depth < 6.0 && (c.col * 0.6 + depth * 1.7 - t * 3.0).sin() > 0.8 && noise > 0.3 {
        return 0.82;
    }
    // Per-cell grain, like the fire's: each cell jitters a few ramp steps every frame.
    let grain = (noise - 0.5) * 0.16;
    // Water body: lighter on the wave face, deeper further back, with rolling swell bands.
    let face = (depth / (0.5 * c.rows)).min(1.0);
    // One swell band every ~21 rows (2π / 0.3), tilted like the crest:
    // measured from the crest's leaning baseline instead of the row itself.
    let swell_row = c.row - lean * c.rows;
    let bands = 0.08 * (swell_row * 0.3 - t * 6.0 + 0.3 * (c.col * 0.2).sin()).sin();
    // Caustics: light lines along the borders of an organic Voronoi net.
    // Brightens the water by a few ramp steps only.
    let caustic = 0.12 * (1.0 - caustic_edge(c.col, c.row, t) / 0.12).max(0.0);
    lerp(0.62, 0.25, face) + bands + caustic + grain
}

/// A rotating spiral galaxy. Its arm tips reach in from beyond the screen
/// edge and wind down to the center (or, `outward`, burst from the center to
/// the edges), erasing everything they pass over; then the whole galaxy fades
/// out, dust first and arms last. Intensities index the galaxy ramp:
/// ~0.2 faint dust between the arms, up to ~0.8 on arms, 1.0 stars and core.
fn galaxy(c: &Cell, t: f32, noise: f32, outward: bool, clockwise: bool) -> f32 {
    const ARMS: f32 = 5.0; // spiral arm count (integer, keeps the angle seam invisible)
    const TWIST: f32 = 4.0; // how tightly the arms wind, radians per screen radius
    const REACH: f32 = 0.25; // how far the longest arm tips lead the rest
    const ARRIVE: f32 = 0.65; // t at which the tips have swept the whole screen
    const FADE_FROM: f32 = 0.55; // t at which the galaxy starts fading out
    const FLAT: f32 = 0.5; // on-screen height / width of the galaxy disc (1 = circle)

    // Fade-out: 1 until FADE_FROM, smoothly down to 0 at t = 1.
    let f = ((t - FADE_FROM) / (1.0 - FADE_FROM)).clamp(0.0, 1.0);
    let fade = 1.0 - f * f * (3.0 - 2.0 * f);
    if fade <= 0.0 {
        return 0.0;
    }

    // Elliptic polar coordinates: rows are ~2x taller than columns, and the
    // disc is squashed vertically by FLAT. r = 1 at the screen corners.
    let dx = c.col + 0.5 - c.cols / 2.0;
    let dy = (c.row + 0.5 - c.rows / 2.0) * 2.0 / FLAT;
    let r = dx.hypot(dy) / (c.cols / 2.0).hypot(c.rows / FLAT);
    // y grows downward, so a growing angle spins clockwise on screen;
    // mirroring it flips both the spin and the arms' winding.
    let a = if clockwise { dy.atan2(dx) } else { -dy.atan2(dx) };

    // Position along the spiral, slowly rotating over the run.
    let s = a + TWIST * r - t * 1.2;
    // 1 on an arm, 0 midway between two. The cosine keeps the shift soft;
    // the power < 1 widens each arm's bright part (lower = thicker arms).
    let arm = (0.5 + 0.5 * (ARMS * s).cos()).powf(0.4);
    // Per-arm irregularity (integer harmonics): some arms brighter and longer.
    let var = 0.55 + 0.45 * (3.0 * s).cos() * (7.0 * s + 2.0).cos();

    // The tips' front moves from just off-screen to past the far side by
    // t = ARRIVE; longer arms run `ahead` of it. `d` >= 0 once the tips have
    // passed this cell (how far behind them it is).
    let u = (t / ARRIVE).min(1.0);
    let p = u * u * (3.0 - 2.0 * u); // smoothstep: gentle start and arrival
    let ahead = REACH * arm * var;
    let d = if outward {
        lerp(-REACH - 0.01, 1.01, p) + ahead - r
    } else {
        r - (lerp(1.0 + REACH + 0.01, -0.01, p) - ahead)
    };
    if d < 0.0 {
        return 0.0;
    }

    // Stars: 2% of cells host one at a fixed spot; each shines for ~30% of
    // its own twinkle cycle (~0.5 s at the default duration), on its own phase.
    let (star, phase) = hash2(c.col as i32, c.row as i32);
    if star < 0.02 && (t * 8.0 + phase * TAU).cos() > 0.6 {
        return fade;
    }
    let rim = 0.15 * (1.0 - d / 0.15).max(0.0); // glowing edge where the tips advance
    let core = 0.6 * (1.0 - r / 0.12).max(0.0); // bright galactic core
    let arms = 0.6 * arm * var * (0.75 + 0.25 * noise);
    let grain = (noise - 0.5) * 0.2; // fire-like per-cell flicker
    let v = (0.22 + arms + rim + core + grain) * fade;
    // Dim cells drop out first as the fade deepens, flickering like dying embers.
    if v < 0.04 { 0.0 } else { v }
}

/// Smooth 2D value noise in 0..=1, one random value per integer grid point.
fn value_noise(x: f32, y: f32) -> f32 {
    let (ix, iy) = (x.floor() as i32, y.floor() as i32);
    let (fx, fy) = (x - x.floor(), y - y.floor());
    let (sx, sy) = (fx * fx * (3.0 - 2.0 * fx), fy * fy * (3.0 - 2.0 * fy));
    let v = |dx: i32, dy: i32| hash2(ix + dx, iy + dy).0;
    lerp(lerp(v(0, 0), v(1, 0), sx), lerp(v(0, 1), v(1, 1), sx), sy)
}

/// Explosion settings, resolved from `AnimSettings` once per run.
#[derive(Clone, Copy)]
struct Blasts {
    seed: i32,   // varies the layout and timing per run
    count: f32,  // about how many blasts fill the screen
    rhythm: u8,  // 0 = Together, 1 = Staggered, 2 = Chain (left → right)
    debris: f32, // streak / debris multiplier: 0 = none, 1 = normal
}

/// Several fireballs going off across the screen. The screen is cut into a
/// grid of about `count` slots; each slot gets one blast at a random spot
/// inside it, sized so its own slot is fully covered, so together they always
/// erase the whole screen.
fn explosions(c: &Cell, t: f32, noise: f32, b: Blasts) -> f32 {
    // (life, stagger): each blast lasts `life` of the run and starts within the
    // first `stagger`; life + stagger = 1, so every blast is over at t = 1.
    let (life, stagger) = match b.rhythm {
        0 => (0.95, 0.05),
        2 => (0.45, 0.55),
        _ => (0.7, 0.3),
    };
    let seed = b.seed; // shuffles each blast's spot and start time
    // Pixel-ish space: rows are ~2x taller than columns.
    let (w, h) = (c.cols, c.rows * 2.0);
    let gx = (b.count * w / h).sqrt().round().max(1.0);
    let gy = (b.count / gx).round().max(1.0);
    let (cw, ch) = (w / gx, h / gy);
    let (px, py) = (c.col + 0.5, (c.row + 0.5) * 2.0);

    let mut brightest = 0.0f32;
    for j in 0..gy as i32 {
        for i in 0..gx as i32 {
            let (jx, jy) = hash2(i.wrapping_add(seed), j.wrapping_sub(seed));
            // Center somewhere in the middle half of its slot.
            let cx = (i as f32 + 0.25 + 0.5 * jx) * cw;
            let cy = (j as f32 + 0.25 + 0.5 * jy) * ch;
            let (luck, _) = hash2(i.wrapping_sub(seed), j.wrapping_add(seed).wrapping_add(1));
            // Chain: a reaction sweeping left → right, with a little jitter.
            let start = if b.rhythm == 2 { 0.85 * cx / w + 0.15 * luck } else { luck };
            let local_t = (t - stagger * start) / life;
            if local_t <= 0.0 || local_t >= 1.0 {
                continue;
            }
            // Reach = distance to the slot's farthest corner, so r <= 1 covers the slot.
            let reach_x = (cx - i as f32 * cw).max((i + 1) as f32 * cw - cx);
            let reach_y = (cy - j as f32 * ch).max((j + 1) as f32 * ch - cy);
            let (dx, dy) = (px - cx, py - cy);
            let r = dx.hypot(dy) / reach_x.hypot(reach_y);
            if r > 2.4 {
                continue; // beyond this blast's longest streak
            }
            let v = blast(r, dy.atan2(dx), c, local_t, noise, i * 31 + j, b.debris);
            brightest = brightest.max(v);
        }
    }
    brightest
}

/// One billowing fireball, in its own polar coordinates (`r` = 1 at the edge
/// of the area it must cover, `a` = angle) and its own time `t` in 0..=1.
/// It bursts out until it covers r = 1, flinging streaks and debris ahead of
/// its lobed, ragged edge, then cools and crumbles away, dim cells first.
/// Intensities index the explosion ramp: ~0.25 dark red rim, ~0.6 orange,
/// 0.85+ yellow-white core. `id` keeps each blast's streaks different;
/// `debris` scales how many streaks and specks it throws (0 = none).
fn blast(r: f32, a: f32, c: &Cell, t: f32, noise: f32, id: i32, debris: f32) -> f32 {
    const ARRIVE: f32 = 0.4; // t at which the fireball covers its whole area
    const FADE_FROM: f32 = 0.45; // t at which it starts cooling and crumbling
    const LOBE: f32 = 0.12; // depth of the billowy lobes on the outline
    const JAG: f32 = 0.04; // per-cell raggedness of the outline
    const SPOKES: f32 = 40.0; // angular slots that may each hold a streak
    // Final radius: even the deepest lobe notch ends past the area's edge (r = 1).
    const R_END: f32 = 1.02 / (1.0 - LOBE - JAG / 2.0);

    let f = ((t - FADE_FROM) / (1.0 - FADE_FROM)).clamp(0.0, 1.0);
    let fade = 1.0 - f * f * (3.0 - 2.0 * f);
    let u = (t / ARRIVE).min(1.0);
    let radius = R_END * (1.0 - (1.0 - u).powi(3)); // ease-out: violent burst, then settles
    if fade <= 0.0 || radius <= 0.0 {
        return 0.0;
    }
    let grain = (noise - 0.5) * 0.16; // fire-like per-cell flicker

    // Lobed outline: integer angular harmonics (no seam), drifting over time.
    let lobes = 0.5 * (5.0 * a + 1.3 + t * 2.0).cos()
        + 0.3 * (8.0 * a + 0.4 - t * 3.0).cos()
        + 0.2 * (13.0 * a + 2.1).cos();
    let edge = radius * (1.0 + LOBE * lobes + JAG * (noise - 0.5));

    if r <= edge {
        // Inside: hot core cooling towards the rim, broken up by churning billows.
        let (px, py) = (c.col / 5.0, c.row * 2.0 / 5.0);
        let billow = 0.65 * value_noise(px + t * 1.5, py - t * 2.0)
            + 0.35 * value_noise(px * 2.3 - t * 3.0, py * 2.3 + t * 2.5);
        let heat = 1.0 - 0.75 * (r / edge).powf(1.3) + (billow - 0.5) * 0.55 + grain;
        let v = heat.max(0.1) * fade;
        // Crumbling: as it cools, cells drop out at a noisy, rising threshold.
        let threshold = 0.05 + 0.35 * (1.0 - fade) * noise;
        return if v < threshold { 0.0 } else { v };
    }

    // Outside: thin streaks flung out along some spokes, tapering to a point.
    let k = (a / TAU + 0.5) * SPOKES;
    let (has_streak, length) = hash2(k.floor() as i32, 911 + id);
    // More debris also flings the streaks further out.
    let tip = radius * (1.15 + 0.45 * length * debris.min(1.4));
    if has_streak < 0.35 * debris && r < tip {
        let taper = (tip - r) / (tip - edge).max(1e-3); // 1 at the fireball, 0 at the tip
        if (k.fract() - 0.5).abs() < 0.12 * taper {
            return (0.22 + 0.15 * taper + grain * 0.5).max(0.05) * fade;
        }
    }
    // Scattered debris specks just ahead of the fireball.
    if r < radius * 1.6 && hash2(c.col as i32, c.row as i32).0 < 0.012 * debris {
        return 0.3 * fade;
    }
    0.0
}

fn fill_field(grid: &mut [u8], cols: usize, rows: usize, t: f32, field: &Field, rng: &mut Rng) {
    for y in 0..rows {
        for x in 0..cols {
            let cell = Cell {
                y: (y as f32 + 0.5) / rows as f32,
                col: x as f32,
                row: y as f32,
                cols: cols as f32,
                rows: rows as f32,
            };
            let noise = (rng.next_u64() >> 40) as f32 / (1u64 << 24) as f32;
            // ceil: any lit cell gets heat >= 1, so it counts as burned.
            let i = field(&cell, t, noise).clamp(0.0, 1.0);
            grid[y * cols + x] = (i * MAX_HEAT as f32).ceil() as u8;
        }
    }
}

fn sweep(effect: &Sweep, fps: u32, interrupted: Arc<AtomicBool>) {
    let (mut cols, mut rows) = terminal_size();
    let mut grid = vec![0u8; cols * rows];
    let mut burned = vec![false; cols * rows];
    let mut rng = Rng::new();

    let stdout = io::stdout();
    let mut out = stdout.lock();
    let _ = write!(out, "{ESC}[?25l");

    let start = Instant::now();
    let mut frame = String::with_capacity(cols * rows * 8);
    let frame_delay = Duration::from_millis(1000 / fps.max(1) as u64);

    loop {
        if interrupted.load(Ordering::Relaxed) {
            break;
        }
        let t = start.elapsed().as_secs_f32() / effect.duration;
        if t >= 1.0 {
            break;
        }

        let (new_cols, new_rows) = terminal_size();
        if new_cols != cols || new_rows != rows {
            cols = new_cols;
            rows = new_rows;
            grid = vec![0u8; cols * rows];
            burned = vec![false; cols * rows];
            frame.reserve(cols * rows * 8);
        }

        effect.fill(&mut grid, cols, rows, t, &mut rng);
        for (b, &h) in burned.iter_mut().zip(&grid) {
            *b |= h > 0;
        }

        render(&mut frame, &grid, &burned, cols, rows, &effect.palette);
        let _ = out.write_all(frame.as_bytes());
        let _ = out.flush();
        std::thread::sleep(frame_delay);
    }

    clear_unburned(&mut frame, &burned, cols, rows);
    let _ = out.write_all(frame.as_bytes());
    let _ = out.flush();
    let _ = write!(out, "{ESC}[?25h");
}

// ── Fire loop ─────────────────────────────────────────────────────────

fn fire(palette: &Palette, settings: &AnimSettings, interrupted: Arc<AtomicBool>) {
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Each sweep effect must light every cell at some point (else the cell
    /// is only wiped by the final pass), and must be fully dark at t = 0 and t = 1.
    #[test]
    fn sweep_effects_cover_the_whole_screen() {
        let mut rng = Rng::new();
        let max_tilt = 30f32.to_radians().tan();
        let wave_left = move |c: &Cell, t, n| wave(c, t, n, -max_tilt);
        let wave_level = |c: &Cell, t, n| wave(c, t, n, 0.0);
        let wave_right = move |c: &Cell, t, n| wave(c, t, n, max_tilt);
        let galaxy_in = |c: &Cell, t, n| galaxy(c, t, n, false, true);
        let galaxy_out = |c: &Cell, t, n| galaxy(c, t, n, true, false);
        // Extremes of every explosion setting, on two different layouts.
        let blasts = |seed, count, rhythm, debris| {
            let b = Blasts { seed, count, rhythm, debris };
            move |c: &Cell, t, n| explosions(c, t, n, b)
        };
        let single_together = blasts(0, 1.0, 0, 0.0);
        let some_staggered = blasts(-987_654_321, 6.0, 1, 1.0);
        let barrage_chain = blasts(42, 16.0, 2, 2.2);
        let fields: [&Field; 8] = [
            &wave_left, &wave_level, &wave_right, &galaxy_in, &galaxy_out,
            &single_together, &some_staggered, &barrage_chain,
        ];
        for (cols, rows) in [(80, 24), (237, 61), (300, 10), (1, 1)] {
            for field in fields {
                let mut grid = vec![0u8; cols * rows];
                let mut hit = vec![false; cols * rows];

                fill_field(&mut grid, cols, rows, 0.0, field, &mut rng);
                assert!(grid.iter().all(|&h| h == 0), "lit at t=0 ({cols}x{rows})");

                for step in 0..=200 {
                    fill_field(&mut grid, cols, rows, step as f32 / 200.0, field, &mut rng);
                    for (h, &g) in hit.iter_mut().zip(&grid) {
                        *h |= g > 0;
                    }
                }
                assert!(grid.iter().all(|&h| h == 0), "still lit at t=1 ({cols}x{rows})");
                assert!(hit.iter().all(|&h| h), "cell never lit ({cols}x{rows})");
            }
        }
    }
}
