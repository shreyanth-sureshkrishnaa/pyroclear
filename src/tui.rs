// tui.rs — raw terminal, key input, interactive picker & settings TUI.

use crate::{
    config::{AnimSettings, PaletteChoice},
    engine::{terminal_size, Rng},
    palettes::*,
    ESC,
};
use std::io::{self, Read, Write};
use std::time::Duration;

// ── Raw terminal guard ────────────────────────────────────────────────

/// RAII guard: enters raw mode on creation, restores the terminal on drop.
/// Also switches to the alternate screen buffer.
pub struct TermRawGuard {
    #[cfg(unix)]
    orig: libc::termios,
    #[cfg(windows)]
    orig: crate::win::RawConsole,
}

impl TermRawGuard {
    pub fn enter() -> io::Result<Self> {
        #[cfg(unix)]
        let orig = {
            let mut orig: libc::termios = unsafe { std::mem::zeroed() };
            if unsafe { libc::tcgetattr(libc::STDIN_FILENO, &mut orig) } != 0 {
                return Err(io::Error::last_os_error());
            }
            let mut raw = orig;
            raw.c_lflag &= !(libc::ICANON | libc::ECHO | libc::ISIG);
            raw.c_cc[libc::VMIN] = 1;
            raw.c_cc[libc::VTIME] = 0;
            unsafe { libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &raw) };
            orig
        };
        #[cfg(windows)]
        let orig = crate::win::enter_raw()?;

        print!("{ESC}[?1049h{ESC}[?25l"); // alternate screen, hide cursor
        io::stdout().flush().ok();
        Ok(Self { orig })
    }
}

impl Drop for TermRawGuard {
    fn drop(&mut self) {
        print!("{ESC}[?1049l{ESC}[?25h"); // restore screen and cursor
        io::stdout().flush().ok();
        #[cfg(unix)]
        unsafe {
            libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &self.orig)
        };
        #[cfg(windows)]
        crate::win::leave_raw(&self.orig);
    }
}

// ── Key reading ───────────────────────────────────────────────────────

#[derive(Debug, PartialEq)]
pub enum Key {
    Up,
    Down,
    Left,
    Right,
    PageUp,
    PageDown,
    Enter,
    Char(char),
    Esc,
    Backspace,
    Other,
}

pub fn read_key() -> Key {
    let mut buf = [0u8; 6];
    let n = std::io::stdin().read(&mut buf).unwrap_or(0);
    if n == 0 {
        return Key::Other;
    }
    match &buf[..n as usize] {
        [0x1b, b'[', b'A', ..] => Key::Up,
        [0x1b, b'[', b'B', ..] => Key::Down,
        [0x1b, b'[', b'C', ..] => Key::Right,
        [0x1b, b'[', b'D', ..] => Key::Left,
        [0x1b, b'[', b'5', b'~', ..] => Key::PageUp,
        [0x1b, b'[', b'6', b'~', ..] => Key::PageDown,
        [0x1b, ..] if n == 1 => Key::Esc,
        [0x0d] | [0x0a] => Key::Enter,
        [0x7f] | [0x08] => Key::Backspace,
        [c] if *c >= 0x20 && *c < 0x7f => Key::Char(*c as char),
        _ => Key::Other,
    }
}

// ── Hex prompt ────────────────────────────────────────────────────────

pub fn prompt_hex(label: &str, row: u16) -> Option<String> {
    let mut input = String::new();
    loop {
        print!("{ESC}[{row};1H{ESC}[2K{ESC}[38;2;255;200;80m{label}{ESC}[0m {input}_");
        io::stdout().flush().ok();
        match read_key() {
            Key::Enter => {
                let s = input.trim().to_string();
                if s.is_empty() {
                    return None;
                }
                if hex_to_rgb(&s).is_some() {
                    return Some(s);
                }
                print!(
                    "{ESC}[{row};1H{ESC}[2K\
                     {ESC}[38;2;255;70;70m  ✗ invalid hex — need #rrggbb{ESC}[0m"
                );
                io::stdout().flush().ok();
                std::thread::sleep(Duration::from_millis(800));
                input.clear();
            }
            Key::Backspace => {
                input.pop();
            }
            Key::Char(c) => {
                if input.len() < 8 {
                    input.push(c);
                }
            }
            Key::Esc => return None,
            _ => {}
        }
    }
}

// ── String helper ─────────────────────────────────────────────────────

/// Truncate to at most `max_chars` display characters (Unicode-safe).
fn truncate_display(s: &str, max_chars: usize) -> &str {
    if max_chars == 0 {
        return "";
    }
    for (n, (byte_idx, _)) in s.char_indices().enumerate() {
        if n >= max_chars {
            return &s[..byte_idx];
        }
    }
    s
}

// ── Picker filter ─────────────────────────────────────────────────────

/// Build a filtered list of NAMED_PALETTES indices matching `search`.
/// Index == NAMED_PALETTES.len() is the "Custom" sentinel.
fn apply_filter(search: &str) -> Vec<usize> {
    let s = search.to_lowercase();
    if s.is_empty() {
        let mut v: Vec<usize> = (0..NAMED_PALETTES.len()).collect();
        v.push(NAMED_PALETTES.len());
        return v;
    }
    let mut v: Vec<usize> = NAMED_PALETTES
        .iter()
        .enumerate()
        .filter(|(_, entry)| {
            let (id, display, desc, _, _) = entry;
            id.to_lowercase().contains(s.as_str())
                || display.to_lowercase().contains(s.as_str())
                || desc.to_lowercase().contains(s.as_str())
        })
        .map(|(i, _)| i)
        .collect();
    if "custom".contains(s.as_str()) {
        v.push(NAMED_PALETTES.len());
    }
    v
}

// ── Picker draw ───────────────────────────────────────────────────────

fn draw_picker(
    selected: usize,
    filter: &[usize],
    search: &str,
    search_active: bool,
    (cols, rows): (usize, usize),
) {
    let sw_w = (cols.saturating_sub(50)).clamp(10, 30);

    // Row 1: title bar
    print!("{ESC}[1;1H");
    let title = " pyroclear  ◆  color picker ";
    let gap = cols.saturating_sub(title.len());
    print!(
        "{ESC}[48;2;20;20;36m{ESC}[38;2;255;200;80m{title}\
         {ESC}[38;2;50;50;72m{}{ESC}[0m",
        " ".repeat(gap)
    );

    // Row 2: search bar
    print!("{ESC}[2;1H{ESC}[2K");
    let match_count = filter.iter().filter(|&&i| i < NAMED_PALETTES.len()).count();
    if search.is_empty() && !search_active {
        print!(
            "  {ESC}[38;2;55;55;78m/{ESC}[0m \
             {ESC}[38;2;52;52;72msearch palettes...{ESC}[0m  \
             {ESC}[38;2;52;52;72m{match_count} palettes{ESC}[0m"
        );
    } else {
        let caret = if search_active { "_" } else { "" };
        let col = if search_active {
            "255;220;80"
        } else {
            "150;150;180"
        };
        print!(
            "  {ESC}[38;2;{col}m/{search}{caret}{ESC}[0m  \
             {ESC}[38;2;95;95;115m{match_count} match{}{ESC}[0m",
            if match_count == 1 { "" } else { "es" }
        );
    }

    // Palette list
    let list_start = 2usize;
    let list_end = rows.saturating_sub(4);
    let visible = list_end.saturating_sub(list_start);
    let offset = if selected >= visible {
        selected - visible + 1
    } else {
        0
    };

    for slot in 0..visible {
        let fi_pos = slot + offset;
        let row = (list_start + slot + 1) as u16;
        print!("{ESC}[{row};1H{ESC}[2K");
        if fi_pos >= filter.len() {
            continue;
        }

        let fi = filter[fi_pos];
        let is_sel = fi_pos == selected;
        let cursor = if is_sel { "▸" } else { " " };

        if is_sel {
            print!("{ESC}[48;2;20;26;46m");
        }

        let (name_str, desc_str, sw_str, fhex, thex) = if fi < NAMED_PALETTES.len() {
            let (id, display, desc, fh, th) = NAMED_PALETTES[fi];
            let p = if id == "fire" {
                soften(&FIRE_PALETTE, SOFTEN_DESATURATE, SOFTEN_BRIGHTEN)
            } else {
                let from = hex_to_rgb(fh).unwrap_or((0, 0, 0));
                let to = hex_to_rgb(th).unwrap_or((255, 255, 255));
                soften(
                    &generate_palette(from, to),
                    SOFTEN_DESATURATE,
                    SOFTEN_BRIGHTEN,
                )
            };
            (display, desc, palette_swatch(&p, sw_w), fh, th)
        } else {
            (
                "Custom",
                "enter your own #rrggbb gradient",
                swatch((50, 0, 60), (255, 180, 80), sw_w),
                "",
                "",
            )
        };

        if is_sel {
            print!("{ESC}[1;38;2;255;230;100m");
        } else {
            print!("{ESC}[38;2;178;178;205m");
        }

        print!("  {cursor} {name_str:<18}  {sw_str}");

        if fi < NAMED_PALETTES.len() {
            print!(
                "  {ESC}[38;2;82;82;108m{fhex}\
                 {ESC}[38;2;48;48;68m→\
                 {ESC}[38;2;82;82;108m{thex}{ESC}[0m"
            );
        }

        let used_approx = 4 + 18 + 2 + sw_w + 2 + 17;
        if cols > used_approx + 6 {
            let max_d = cols.saturating_sub(used_approx + 4);
            let td = truncate_display(desc_str, max_d);
            let dc = if is_sel { "140;140;172" } else { "62;62;82" };
            print!("  {ESC}[38;2;{dc}m{td}{ESC}[0m");
        }

        print!("{ESC}[0m");
    }

    // Separator
    let sep_row = (list_end + 1) as u16;
    print!(
        "{ESC}[{sep_row};1H{ESC}[2K\
         {ESC}[38;2;35;35;55m{}{ESC}[0m",
        "─".repeat(cols)
    );

    // Preview swatch for selected entry
    let prev_row = (rows - 2) as u16;
    print!("{ESC}[{prev_row};1H{ESC}[2K");
    if let Some(&fi) = filter.get(selected) {
        if fi < NAMED_PALETTES.len() {
            let (id, display, _, fh, th) = NAMED_PALETTES[fi];
            let p = if id == "fire" {
                soften(&FIRE_PALETTE, SOFTEN_DESATURATE, SOFTEN_BRIGHTEN)
            } else {
                let from = hex_to_rgb(fh).unwrap_or((0, 0, 0));
                let to = hex_to_rgb(th).unwrap_or((255, 255, 255));
                soften(
                    &generate_palette(from, to),
                    SOFTEN_DESATURATE,
                    SOFTEN_BRIGHTEN,
                )
            };
            let pw = cols.saturating_sub(22).min(72);
            let ps = palette_swatch(&p, pw);
            print!("  {ESC}[38;2;92;92;115m▸ {ESC}[38;2;172;172;198m{display:<16}{ESC}[0m  {ps}");
        } else {
            print!("  {ESC}[38;2;92;92;115m▸ Custom gradient — press Enter to configure{ESC}[0m");
        }
    }

    // Key hints
    let hint_row = rows as u16;
    print!("{ESC}[{hint_row};1H{ESC}[2K");
    print!(
        "{ESC}[48;2;15;15;28m \
         {ESC}[38;2;255;200;80m↑↓{ESC}[38;2;98;98;128m move  \
         {ESC}[38;2;255;200;80mPgUp/Dn{ESC}[38;2;98;98;128m page  \
         {ESC}[38;2;255;200;80m/{ESC}[38;2;98;98;128m search  \
         {ESC}[38;2;255;200;80mr{ESC}[38;2;98;98;128m random  \
         {ESC}[38;2;255;200;80mEnter{ESC}[38;2;98;98;128m select  \
         {ESC}[38;2;255;200;80mEsc{ESC}[38;2;98;98;128m·{ESC}[38;2;255;200;80mq{ESC}[38;2;98;98;128m quit \
         {ESC}[0m"
    );

    io::stdout().flush().ok();
}

// ── Interactive picker ────────────────────────────────────────────────

/// Run the interactive palette picker. Returns the chosen PaletteChoice
/// or None if the user cancelled.
pub fn interactive_pick() -> Option<PaletteChoice> {
    let _guard = TermRawGuard::enter().ok()?;

    let mut selected = 0usize;
    let mut search = String::new();
    let mut search_active = false;
    let mut filter = apply_filter("");
    let page_size = 10usize;

    loop {
        let size = terminal_size();
        draw_picker(selected, &filter, &search, search_active, size);

        let key = read_key();

        if search_active {
            match key {
                Key::Esc | Key::Enter => {
                    search_active = false;
                }
                Key::Backspace => {
                    search.pop();
                    filter = apply_filter(&search);
                    selected = 0;
                }
                Key::Char(c) => {
                    search.push(c);
                    filter = apply_filter(&search);
                    selected = 0;
                }
                _ => {}
            }
        } else {
            match key {
                Key::Up => {
                    selected = selected.saturating_sub(1);
                }
                Key::Down => {
                    if !filter.is_empty() && selected + 1 < filter.len() {
                        selected += 1;
                    }
                }
                Key::PageUp => {
                    selected = selected.saturating_sub(page_size);
                }
                Key::PageDown => {
                    if !filter.is_empty() {
                        selected = (selected + page_size).min(filter.len() - 1);
                    }
                }
                Key::Char('/') => {
                    search_active = true;
                }
                Key::Char('r') | Key::Char('R') => {
                    if !filter.is_empty() {
                        let mut rng = Rng::new();
                        selected = (rng.next_u64() % filter.len() as u64) as usize;
                    }
                }
                Key::Enter => {
                    if let Some(&fi) = filter.get(selected) {
                        if fi < NAMED_PALETTES.len() {
                            let (id, _, _, _, _) = NAMED_PALETTES[fi];
                            return Some(PaletteChoice::Named(id.to_string()));
                        } else {
                            let (_, rows) = terminal_size();
                            let base = rows as u16 - 4;
                            print!("{ESC}[{base};1H{ESC}[J");
                            println!(
                                "{ESC}[{base};1H\
                                 {ESC}[38;2;175;175;200m  Enter hex colors (e.g. #ff0000){ESC}[0m"
                            );
                            io::stdout().flush().ok();
                            let from_str = prompt_hex("  From:", base + 2)?;
                            let to_str = prompt_hex("  To:  ", base + 3)?;
                            let from = hex_to_rgb(&from_str)?;
                            let to = hex_to_rgb(&to_str)?;
                            return Some(PaletteChoice::Custom { from, to });
                        }
                    }
                }
                Key::Esc => {
                    if !search.is_empty() {
                        search.clear();
                        filter = apply_filter("");
                        selected = 0;
                    } else {
                        return None;
                    }
                }
                Key::Char('q') => return None,
                _ => {}
            }
        }
    }
}

// ── Settings draw ─────────────────────────────────────────────────────

fn draw_settings(selected: usize, settings: &AnimSettings, (cols, rows): (usize, usize)) {
    // Title bar
    print!("{ESC}[1;1H");
    let title = " pyroclear  ◆  animation settings ";
    let gap = cols.saturating_sub(title.len());
    print!(
        "{ESC}[48;2;20;20;36m{ESC}[38;2;255;200;80m{title}\
         {ESC}[38;2;50;50;72m{}{ESC}[0m",
        " ".repeat(gap)
    );

    let items = [
        (
            "FPS / Speed    ",
            match settings.fps {
                15 => "15 fps  (extremely slow / cinematic)",
                30 => "30 fps  (standard retro feel)",
                45 => "45 fps  (smooth legacy animation)",
                60 => "60 fps  (default smooth 60fps)",
                75 => "75 fps  (high refresh rate)",
                90 => "90 fps  (ultra high refresh rate)",
                120 => "120 fps (blazing fast execution)",
                _ => "custom fps",
            },
        ),
        (
            "Wind / Breeze  ",
            match settings.wind {
                -2 => "Strong Left   (blowing hard left)",
                -1 => "Gentle Left   (gently drifting left)",
                0 => "None          (rising straight up)",
                1 => "Gentle Right  (gently drifting right)",
                2 => "Strong Right  (blowing hard right)",
                _ => "unknown wind",
            },
        ),
        (
            "Flame Height   ",
            match settings.height {
                0 => "Low           (fast decay, small fire)",
                1 => "Medium        (default height decay)",
                2 => "High          (slow decay, tall flames)",
                3 => "Extreme       (minimum decay, full screen)",
                _ => "unknown height",
            },
        ),
        (
            "Fire Direction ",
            if settings.direction {
                "Top → Bottom  (falling fire effect)"
            } else {
                "Bottom → Top  (classic rising flames)"
            },
        ),
    ];

    let start_row = 3u16;
    for (idx, (label, value)) in items.iter().enumerate() {
        let row = start_row + idx as u16 * 2;
        let is_sel = idx == selected;
        let cursor = if is_sel { "▸" } else { " " };

        print!("{ESC}[{row};1H{ESC}[2K");
        if is_sel {
            print!("{ESC}[48;2;20;26;46m");
            print!("  {cursor} {ESC}[1;38;2;255;230;100m{label}{ESC}[0m");
            print!("{ESC}[48;2;20;26;46m   ◀  {ESC}[1;38;2;255;255;255m{value:<44}{ESC}[0m◀   ");
            let taken = 4 + 1 + label.len() + 6 + 44 + 4;
            if cols > taken {
                print!("{}", " ".repeat(cols - taken));
            }
            print!("{ESC}[0m");
        } else {
            print!("    {label}      {ESC}[38;2;178;178;205m{value}{ESC}[0m");
        }
    }

    let sep_row = start_row + items.len() as u16 * 2 + 1;
    print!(
        "{ESC}[{sep_row};1H{ESC}[2K\
         {ESC}[38;2;35;35;55m{}{ESC}[0m",
        "─".repeat(cols)
    );

    let hint_row = rows as u16;
    print!("{ESC}[{hint_row};1H{ESC}[2K");
    print!(
        "{ESC}[48;2;15;15;28m \
         {ESC}[38;2;255;200;80m↑↓{ESC}[38;2;98;98;128m navigate  \
         {ESC}[38;2;255;200;80m←→{ESC}[38;2;98;98;128m adjust  \
         {ESC}[38;2;255;200;80mEnter/s{ESC}[38;2;98;98;128m save & run  \
         {ESC}[38;2;255;200;80mEsc/q{ESC}[38;2;98;98;128m cancel \
         {ESC}[0m"
    );

    io::stdout().flush().ok();
}

// ── Interactive settings ──────────────────────────────────────────────

pub fn interactive_settings(current: &AnimSettings) -> Option<AnimSettings> {
    let _guard = TermRawGuard::enter().ok()?;
    let mut settings = current.clone();
    let mut selected = 0usize;
    let fps_options = [15, 30, 45, 60, 75, 90, 120];

    loop {
        let size = terminal_size();
        draw_settings(selected, &settings, size);

        match read_key() {
            Key::Up => {
                selected = selected.saturating_sub(1);
            }
            Key::Down => {
                if selected < 3 {
                    selected += 1;
                }
            }
            Key::Left => match selected {
                0 => {
                    if let Some(idx) = fps_options.iter().position(|&x| x == settings.fps) {
                        settings.fps = if idx > 0 {
                            fps_options[idx - 1]
                        } else {
                            fps_options[fps_options.len() - 1]
                        };
                    }
                }
                1 => {
                    settings.wind = if settings.wind > -2 {
                        settings.wind - 1
                    } else {
                        2
                    };
                }
                2 => {
                    settings.height = if settings.height > 0 {
                        settings.height - 1
                    } else {
                        3
                    };
                }
                3 => {
                    settings.direction = !settings.direction;
                }
                _ => {}
            },
            Key::Right => match selected {
                0 => {
                    if let Some(idx) = fps_options.iter().position(|&x| x == settings.fps) {
                        settings.fps = if idx + 1 < fps_options.len() {
                            fps_options[idx + 1]
                        } else {
                            fps_options[0]
                        };
                    }
                }
                1 => {
                    settings.wind = if settings.wind < 2 {
                        settings.wind + 1
                    } else {
                        -2
                    };
                }
                2 => {
                    settings.height = if settings.height < 3 {
                        settings.height + 1
                    } else {
                        0
                    };
                }
                3 => {
                    settings.direction = !settings.direction;
                }
                _ => {}
            },
            Key::Enter | Key::Char('s') => return Some(settings),
            Key::Esc | Key::Char('q') => return None,
            _ => {}
        }
    }
}

// ── Prompt string helper ──────────────────────────────────────────────

pub fn prompt_string(label: &str, row: u16) -> Option<String> {
    let mut input = String::new();
    loop {
        print!("{ESC}[{row};1H{ESC}[2K{ESC}[38;2;255;200;80m{label}{ESC}[0m {input}_");
        io::stdout().flush().ok();
        match read_key() {
            Key::Enter => {
                let s = input.trim().to_string();
                if !s.is_empty() {
                    return Some(s);
                }
            }
            Key::Backspace => {
                input.pop();
            }
            Key::Char(c) => {
                if input.len() < 30 {
                    input.push(c);
                }
            }
            Key::Esc => return None,
            _ => {}
        }
    }
}

// ── Interactive Custom Palette Manager ─────────────────────────────────

pub fn interactive_custom() -> Option<PaletteChoice> {
    let _guard = TermRawGuard::enter().ok()?;
    let mut selected = 0usize;
    let mut entries = crate::config::load_custom_palettes();

    loop {
        let size = terminal_size();
        let (cols, rows) = size;

        // Draw header
        print!("{ESC}[1;1H");
        let title = " pyroclear  ◆  custom palettes ";
        let gap = cols.saturating_sub(title.len());
        print!(
            "{ESC}[48;2;20;20;36m{ESC}[38;2;255;200;80m{title}\
             {ESC}[38;2;50;50;72m{}{ESC}[0m",
            " ".repeat(gap)
        );

        // Draw list
        let list_start = 2usize;
        let list_end = rows.saturating_sub(4);
        let visible = list_end.saturating_sub(list_start);

        if selected >= entries.len() && !entries.is_empty() {
            selected = entries.len() - 1;
        }

        let offset = if selected >= visible {
            selected - visible + 1
        } else {
            0
        };

        for slot in 0..visible {
            let row = (list_start + slot + 1) as u16;
            print!("{ESC}[{row};1H{ESC}[2K");
            let idx = slot + offset;
            if idx >= entries.len() {
                continue;
            }

            let entry = &entries[idx];
            let is_sel = idx == selected;
            let cursor = if is_sel { "▸" } else { " " };

            if is_sel {
                print!("{ESC}[48;2;20;26;46m");
            }

            let from_rgb = hex_to_rgb(&entry.from).unwrap_or((0, 0, 0));
            let to_rgb = hex_to_rgb(&entry.to).unwrap_or((255, 255, 255));
            let p = soften(
                &generate_palette(from_rgb, to_rgb),
                SOFTEN_DESATURATE,
                SOFTEN_BRIGHTEN,
            );
            let sw_w = (cols.saturating_sub(55)).clamp(10, 30);
            let sw_str = palette_swatch(&p, sw_w);

            if is_sel {
                print!("{ESC}[1;38;2;255;230;100m");
            } else {
                print!("{ESC}[38;2;178;178;205m");
            }

            print!(
                "  {cursor} {:<15} ({:<10})  {sw_str}  ",
                entry.display, entry.name
            );
            print!(
                "{ESC}[38;2;82;82;108m{}\
                 {ESC}[38;2;48;48;68m→\
                 {ESC}[38;2;82;82;108m{}{ESC}[0m",
                entry.from, entry.to
            );

            print!("{ESC}[0m");
        }

        if entries.is_empty() {
            print!("{ESC}[4;1H{ESC}[2K  No custom palettes saved yet. Press 'n' to create one!");
        }

        // Separator
        let sep_row = (list_end + 1) as u16;
        print!(
            "{ESC}[{sep_row};1H{ESC}[2K\
             {ESC}[38;2;35;35;55m{}{ESC}[0m",
            "─".repeat(cols)
        );

        // Preview swatch for selected entry
        let prev_row = (rows - 2) as u16;
        print!("{ESC}[{prev_row};1H{ESC}[2K");
        if !entries.is_empty() && selected < entries.len() {
            let entry = &entries[selected];
            let from_rgb = hex_to_rgb(&entry.from).unwrap_or((0, 0, 0));
            let to_rgb = hex_to_rgb(&entry.to).unwrap_or((255, 255, 255));
            let p = soften(
                &generate_palette(from_rgb, to_rgb),
                SOFTEN_DESATURATE,
                SOFTEN_BRIGHTEN,
            );
            let pw = cols.saturating_sub(22).min(72);
            let ps = palette_swatch(&p, pw);
            print!(
                "  {ESC}[38;2;92;92;115m▸ {ESC}[38;2;172;172;198m{:<16}{ESC}[0m  {ps}",
                entry.display
            );
        }

        // Key hints
        let hint_row = rows as u16;
        print!("{ESC}[{hint_row};1H{ESC}[2K");
        print!(
            "{ESC}[48;2;15;15;28m \
             {ESC}[38;2;255;200;80m↑↓{ESC}[38;2;98;98;128m move  \
             {ESC}[38;2;255;200;80mn{ESC}[38;2;98;98;128m new  \
             {ESC}[38;2;255;200;80md{ESC}[38;2;98;98;128m delete  \
             {ESC}[38;2;255;200;80mEnter{ESC}[38;2;98;98;128m run  \
             {ESC}[38;2;255;200;80mEsc/q{ESC}[38;2;98;98;128m back \
             {ESC}[0m"
        );
        io::stdout().flush().ok();

        match read_key() {
            Key::Up => {
                selected = selected.saturating_sub(1);
            }
            Key::Down => {
                if !entries.is_empty() && selected + 1 < entries.len() {
                    selected += 1;
                }
            }
            Key::Char('n') | Key::Char('N') => {
                let base = rows as u16 - 4;
                print!("{ESC}[{base};1H{ESC}[J");
                println!(
                    "{ESC}[{base};1H\
                     {ESC}[38;2;175;175;200m  Create new custom palette{ESC}[0m"
                );
                io::stdout().flush().ok();

                if let Some(name) = prompt_string("  Slug (id):", base + 1) {
                    let slug = name.to_lowercase().replace(" ", "-");
                    if let Some(display) = prompt_string("  Name:     ", base + 2) {
                        if let Some(from_str) = prompt_hex("  From hex: ", base + 3) {
                            if let Some(to_str) = prompt_hex("  To hex:   ", base + 4) {
                                entries.push(crate::config::CustomPaletteEntry {
                                    name: slug,
                                    display,
                                    from: from_str,
                                    to: to_str,
                                });
                                crate::config::save_custom_palettes(&entries);
                                selected = entries.len() - 1;
                            }
                        }
                    }
                }
            }
            Key::Char('d') | Key::Char('D') => {
                if !entries.is_empty() && selected < entries.len() {
                    entries.remove(selected);
                    crate::config::save_custom_palettes(&entries);
                    if selected > 0 && selected >= entries.len() {
                        selected = entries.len() - 1;
                    }
                }
            }
            Key::Enter => {
                if !entries.is_empty() && selected < entries.len() {
                    let entry = &entries[selected];
                    if let Some(choice) = entry.to_palette_choice() {
                        return Some(choice);
                    }
                }
            }
            Key::Esc | Key::Char('q') => return None,
            _ => {}
        }
    }
}
