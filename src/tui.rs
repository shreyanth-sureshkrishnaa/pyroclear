// tui.rs — raw terminal, key input, interactive picker & settings TUI.

use crate::{
    config::{AnimSettings, PaletteChoice},
    engine::{terminal_size, Rng},
    palettes::*,
    ESC,
};
use std::io::{self, Read, Write};
use std::time::{Duration, Instant};


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
    Tab,
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
        [0x09] => Key::Tab,
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


#[cfg(unix)]
fn poll_stdin(timeout: Duration) -> io::Result<bool> {
    let mut pfd = libc::pollfd {
        fd: libc::STDIN_FILENO,
        events: libc::POLLIN,
        revents: 0,
    };
    let timeout_ms = timeout.as_millis() as libc::c_int;
    let ret = unsafe { libc::poll(&mut pfd, 1, timeout_ms) };
    if ret < 0 {
        let err = io::Error::last_os_error();
        if err.kind() == io::ErrorKind::Interrupted {
            return Ok(false);
        }
        return Err(err);
    }
    Ok(ret > 0 && (pfd.revents & libc::POLLIN) != 0)
}

#[cfg(windows)]
extern "system" {
    fn WaitForSingleObject(h_handle: *mut std::ffi::c_void, dw_milliseconds: u32) -> u32;
    fn GetStdHandle(n_std_handle: u32) -> *mut std::ffi::c_void;
}

#[cfg(windows)]
const STD_INPUT_HANDLE: u32 = (-10i32) as u32;
#[cfg(windows)]
const WAIT_OBJECT_0: u32 = 0x00000000;

#[cfg(windows)]
fn poll_stdin(timeout: Duration) -> io::Result<bool> {
    unsafe {
        let hin = GetStdHandle(STD_INPUT_HANDLE);
        if hin.is_null() {
            return Ok(false);
        }
        let ms = timeout.as_millis() as u32;
        let res = WaitForSingleObject(hin, ms);
        Ok(res == WAIT_OBJECT_0)
    }
}

// ── Live Fire Preview State ──────────────────────────────────────────

struct PreviewFire {
    grid: Vec<u8>,
    cols: usize,
    rows: usize,
}

impl PreviewFire {
    fn new(cols: usize, rows: usize) -> Self {
        Self {
            grid: vec![0; cols * rows],
            cols,
            rows,
        }
    }

    fn resize(&mut self, new_cols: usize, new_rows: usize) {
        if self.cols != new_cols || self.rows != new_rows {
            self.grid = vec![0; new_cols * new_rows];
            self.cols = new_cols;
            self.rows = new_rows;
        }
    }

    fn tick(&mut self, settings: &AnimSettings, rng: &mut Rng) {
        if self.grid.is_empty() {
            return;
        }
        // Run 2 propagation steps per tick to match the real fire speed
        for _ in 0..2 {
            // set heat source
            let source_row = if settings.direction { 0 } else { self.rows - 1 };
            for x in 0..self.cols {
                self.grid[source_row * self.cols + x] = 36;
            }

            // propagate heat
            if settings.direction {
                for x in 0..self.cols {
                    for y in 0..self.rows - 1 {
                        let above = self.grid[y * self.cols + x];
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
                            0 => rng.range(-1, 1),
                            1 => rng.range(0, 1),
                            2 => rng.range(0, 2),
                            _ => rng.range(-1, 1),
                        };
                        let nx = (x as i32 + drift).clamp(0, self.cols as i32 - 1) as usize;
                        let new_val = (above as i32 - decay).max(0) as u8;
                        self.grid[(y + 1) * self.cols + nx] = new_val;
                    }
                }
            } else {
                for x in 0..self.cols {
                    for y in 1..self.rows {
                        let below = self.grid[y * self.cols + x];
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
                            0 => rng.range(-1, 1),
                            1 => rng.range(0, 1),
                            2 => rng.range(0, 2),
                            _ => rng.range(-1, 1),
                        };
                        let nx = (x as i32 + drift).clamp(0, self.cols as i32 - 1) as usize;
                        let new_val = (below as i32 - decay).max(0) as u8;
                        self.grid[(y - 1) * self.cols + nx] = new_val;
                    }
                }
            }
        }
    }

    fn render_lines(&self, palette: &Palette) -> Vec<String> {
        let mut lines = Vec::with_capacity(self.rows);
        for y in 0..self.rows {
            let mut line = String::with_capacity(self.cols * 20);
            let mut current_bg_color: Option<(u8, u8, u8)> = None;
            let mut current_is_default = true;

            for x in 0..self.cols {
                let heat = self.grid[y * self.cols + x];
                if heat > 0 {
                    let rgb = palette[heat as usize];
                    if current_is_default || current_bg_color != Some(rgb) {
                        line.push_str(&format!("{ESC}[48;2;{r};{g};{b}m", r = rgb.0, g = rgb.1, b = rgb.2));
                        current_bg_color = Some(rgb);
                        current_is_default = false;
                    }
                    line.push_str(" ");
                } else {
                    if !current_is_default {
                        line.push_str(&format!("{ESC}[49m"));
                        current_is_default = true;
                        current_bg_color = None;
                    }
                    line.push_str(" ");
                }
            }
            line.push_str(&format!("{ESC}[0m"));
            lines.push(line);
        }
        lines
    }
}

// ── Visual Helper Utilities ──────────────────────────────────────────

fn visual_width(s: &str) -> usize {
    let mut len = 0;
    let mut in_esc = false;
    for c in s.chars() {
        if c == '\x1b' {
            in_esc = true;
        } else if in_esc {
            if c == 'm' || c == 'H' || c == 'J' || c == 'K' {
                in_esc = false;
            }
        } else {
            len += 1;
        }
    }
    len
}

fn pad_right(s: &str, width: usize) -> String {
    let w = visual_width(s);
    if w >= width {
        s.to_string()
    } else {
        format!("{}{}", s, " ".repeat(width - w))
    }
}

// ── Custom Palette Guided Prompt State ────────────────────────────────

#[derive(Clone, PartialEq)]
enum PromptStep {
    Slug,
    Display,
    From,
    To,
}

struct PromptState {
    step: PromptStep,
    slug: String,
    display: String,
    from: String,
    to: String,
    input_buffer: String,
}

// ── Setting Rows Helper ──────────────────────────────────────────────

fn format_setting_row(label: &str, value: &str, is_selected: bool) -> String {
    let indicator = if is_selected {
        format!("{ESC}[1;38;2;255;200;80m▸ {ESC}[0m")
    } else {
        "  ".to_string()
    };
    let styled_label = if is_selected {
        format!("{ESC}[1;38;2;255;255;255m{:<18}{ESC}[0m", label)
    } else {
        format!("{ESC}[38;2;170;170;190m{:<18}{ESC}[0m", label)
    };
    let val_str = if is_selected {
        format!(
            "{ESC}[38;2;255;200;80m◀ {ESC}[1;38;2;255;255;255m{:<14}{ESC}[0m{ESC}[38;2;255;200;80m ▶{ESC}[0m",
            value
        )
    } else {
        format!("  {:<14}  ", value)
    };
    pad_right(&format!("{}{}{}", indicator, styled_label, val_str), 42)
}

// ── Unified Dashboard Runner ──────────────────────────────────────────

pub fn run_dashboard(
    start_tab: usize,
    initial_choice: Option<PaletteChoice>,
    initial_settings: AnimSettings,
) -> Option<(PaletteChoice, AnimSettings)> {
    let _guard = TermRawGuard::enter().ok()?;

    let mut active_tab = start_tab.min(2);
    let mut selected_palette_idx = 0usize;
    let mut selected_setting_idx = 0usize;
    let mut selected_custom_idx = 0usize;
    let mut search_query = String::new();
    let mut search_active = false;
    let mut settings = initial_settings;
    let mut custom_entries = crate::config::load_custom_palettes();
    let mut show_help = false;
    let mut prompt_state: Option<PromptState> = None;
    let mut rng = Rng::new();
    let mut palette_source_is_custom = false;

    // Map selection index back to chosen palette
    let get_current_palette = |sel_idx: usize,
                               query: &str,
                               custom_list: &[crate::config::CustomPaletteEntry]|
     -> (String, String, String, String) {
        let filter = apply_filter(query);
        if let Some(&fi) = filter.get(sel_idx) {
            if fi < NAMED_PALETTES.len() {
                let (id, display, _, fh, th) = NAMED_PALETTES[fi];
                (id.to_string(), display.to_string(), fh.to_string(), th.to_string())
            } else {
                // "Custom" option
                ("custom".to_string(), "Custom".to_string(), "#32003c".to_string(), "#ffb450".to_string())
            }
        } else if !custom_list.is_empty() {
            // Default fallback if we are out of range
            ("fire".to_string(), "Fire".to_string(), "#800000".to_string(), "#ffffff".to_string())
        } else {
            ("fire".to_string(), "Fire".to_string(), "#800000".to_string(), "#ffffff".to_string())
        }
    };

    // Pre-calculate preview grid sizing based on starting window size
    let (mut cols, mut rows) = terminal_size();
    let mut preview_fire = PreviewFire::new(10, 10);
    let mut last_tick = Instant::now();
    let tick_duration = Duration::from_millis(33); // ~30 FPS

    // Set initial selection if an initial choice was supplied
    if let Some(ref choice) = initial_choice {
        match choice {
            PaletteChoice::Named(name) => {
                if let Some(pos) = NAMED_PALETTES.iter().position(|(id, _, _, _, _)| id == name) {
                    selected_palette_idx = pos;
                    palette_source_is_custom = false;
                } else if let Some(pos) = custom_entries.iter().position(|e| &e.name == name) {
                    selected_custom_idx = pos;
                    palette_source_is_custom = true;
                    if start_tab == 0 {
                        active_tab = 2;
                    }
                }
            }
            PaletteChoice::Custom { .. } => {
                // Custom selection triggers Tab 0 Custom option
                selected_palette_idx = NAMED_PALETTES.len();
                palette_source_is_custom = false;
            }
        }
    }


    loop {
        // 1. Maintain Frame Rate & Tick Animation
        let now = Instant::now();
        let elapsed = now.duration_since(last_tick);
        let timeout = if elapsed >= tick_duration {
            Duration::ZERO
        } else {
            tick_duration - elapsed
        };

        // 2. Poll Non-blocking Stdin
        let has_input = if timeout.is_zero() {
            false
        } else {
            poll_stdin(timeout).unwrap_or(false)
        };

        if has_input {
            let key = read_key();

            // Keyboard State Handler
            if show_help {
                // Any key closes the help screen
                show_help = false;
            } else if let Some(ref mut prompt) = prompt_state {
                match key {
                    Key::Backspace => {
                        prompt.input_buffer.pop();
                    }
                    Key::Esc => {
                        prompt_state = None;
                    }
                    Key::Enter => {
                        let trimmed = prompt.input_buffer.trim().to_string();
                        match prompt.step {
                            PromptStep::Slug => {
                                if !trimmed.is_empty() {
                                    prompt.slug = trimmed.to_lowercase().replace(" ", "-");
                                    prompt.input_buffer.clear();
                                    prompt.step = PromptStep::Display;
                                }
                            }
                            PromptStep::Display => {
                                if !trimmed.is_empty() {
                                    prompt.display = trimmed;
                                    prompt.input_buffer.clear();
                                    prompt.step = PromptStep::From;
                                }
                            }
                            PromptStep::From => {
                                if hex_to_rgb(&trimmed).is_some() {
                                    prompt.from = trimmed;
                                    prompt.input_buffer.clear();
                                    prompt.step = PromptStep::To;
                                }
                            }
                            PromptStep::To => {
                                if hex_to_rgb(&trimmed).is_some() {
                                    prompt.to = trimmed;
                                    // Save the new custom palette entry
                                    let new_entry = crate::config::CustomPaletteEntry {
                                        name: prompt.slug.clone(),
                                        display: prompt.display.clone(),
                                        from: prompt.from.clone(),
                                        to: prompt.to.clone(),
                                    };
                                    custom_entries.push(new_entry);
                                    crate::config::save_custom_palettes(&custom_entries);
                                    selected_custom_idx = custom_entries.len() - 1;
                                    prompt_state = None;
                                }
                            }
                        }
                    }
                    Key::Char(c) => {
                        if prompt.input_buffer.len() < 30 {
                            prompt.input_buffer.push(c);
                        }
                    }
                    _ => {}
                }
            } else if search_active {
                match key {
                    Key::Esc | Key::Enter => {
                        search_active = false;
                    }
                    Key::Backspace => {
                        search_query.pop();
                        selected_palette_idx = 0;
                    }
                    Key::Char(c) => {
                        search_query.push(c);
                        selected_palette_idx = 0;
                    }
                    _ => {}
                }
            } else {
                // Normal Dashboard Mode Keyboard Bindings
                match key {
                    Key::Char('1') => {
                        active_tab = 0;
                        palette_source_is_custom = false;
                    }
                    Key::Char('2') => {
                        active_tab = 1;
                    }
                    Key::Char('3') => {
                        active_tab = 2;
                        palette_source_is_custom = true;
                    }
                    Key::Tab => {
                        active_tab = (active_tab + 1) % 3;
                        if active_tab == 0 {
                            palette_source_is_custom = false;
                        } else if active_tab == 2 {
                            palette_source_is_custom = true;
                        }
                    }
                    Key::Char('h') | Key::Char('?') => show_help = true,
                    Key::Esc | Key::Char('q') => return None,
                    Key::Char('/') if active_tab == 0 => {
                        search_active = true;
                    }
                    Key::Char('r') | Key::Char('R') => {
                        if active_tab == 0 {
                            let filter = apply_filter(&search_query);
                            if !filter.is_empty() {
                                selected_palette_idx = (rng.next_u64() % filter.len() as u64) as usize;
                                palette_source_is_custom = false;
                            }
                        } else if active_tab == 2 && !custom_entries.is_empty() {
                            selected_custom_idx = (rng.next_u64() % custom_entries.len() as u64) as usize;
                            palette_source_is_custom = true;
                        }
                    }
                    Key::Char('n') | Key::Char('N') if active_tab == 2 => {
                        prompt_state = Some(PromptState {
                            step: PromptStep::Slug,
                            slug: String::new(),
                            display: String::new(),
                            from: String::new(),
                            to: String::new(),
                            input_buffer: String::new(),
                        });
                        palette_source_is_custom = true;
                    }
                    Key::Char('d') | Key::Char('D') if active_tab == 2 => {
                        palette_source_is_custom = true;
                        if !custom_entries.is_empty() && selected_custom_idx < custom_entries.len() {
                            custom_entries.remove(selected_custom_idx);
                            crate::config::save_custom_palettes(&custom_entries);
                            if selected_custom_idx > 0 && selected_custom_idx >= custom_entries.len() {
                                selected_custom_idx = custom_entries.len() - 1;
                            }
                        }
                    }
                    Key::Up => match active_tab {
                        0 => {
                            selected_palette_idx = selected_palette_idx.saturating_sub(1);
                            palette_source_is_custom = false;
                        }
                        1 => selected_setting_idx = selected_setting_idx.saturating_sub(1),
                        2 => {
                            selected_custom_idx = selected_custom_idx.saturating_sub(1);
                            palette_source_is_custom = true;
                        }
                        _ => {}
                    },
                    Key::Down => match active_tab {
                        0 => {
                            let filter = apply_filter(&search_query);
                            if !filter.is_empty() && selected_palette_idx + 1 < filter.len() {
                                selected_palette_idx += 1;
                            }
                            palette_source_is_custom = false;
                        }
                        1 => {
                            if selected_setting_idx < 5 {
                                selected_setting_idx += 1;
                            }
                        }
                        2 => {
                            if !custom_entries.is_empty() && selected_custom_idx + 1 < custom_entries.len() {
                                selected_custom_idx += 1;
                            }
                            palette_source_is_custom = true;
                        }
                        _ => {}
                    },
                    Key::PageUp => match active_tab {
                        0 => {
                            selected_palette_idx = selected_palette_idx.saturating_sub(10);
                            palette_source_is_custom = false;
                        }
                        2 => {
                            selected_custom_idx = selected_custom_idx.saturating_sub(10);
                            palette_source_is_custom = true;
                        }
                        _ => {}
                    },
                    Key::PageDown => match active_tab {
                        0 => {
                            let filter = apply_filter(&search_query);
                            if !filter.is_empty() {
                                selected_palette_idx = (selected_palette_idx + 10).min(filter.len() - 1);
                            }
                            palette_source_is_custom = false;
                        }
                        2 => {
                            if !custom_entries.is_empty() {
                                selected_custom_idx = (selected_custom_idx + 10).min(custom_entries.len() - 1);
                            }
                            palette_source_is_custom = true;
                        }
                        _ => {}
                    },
                    Key::Left if active_tab == 1 => {
                        let fps_options = [15, 30, 45, 60, 75, 90, 120];
                        let duration_options = [0.10, 0.20, 0.30, 0.38, 0.50, 0.70];
                        match selected_setting_idx {
                            0 => {
                                if let Some(idx) = fps_options.iter().position(|&x| x == settings.fps) {
                                    settings.fps = if idx > 0 { fps_options[idx - 1] } else { fps_options[fps_options.len() - 1] };
                                }
                            }
                            1 => {
                                settings.wind = if settings.wind > -2 { settings.wind - 1 } else { 2 };
                            }
                            2 => {
                                settings.height = if settings.height > 0 { settings.height - 1 } else { 3 };
                            }
                            3 => {
                                settings.direction = !settings.direction;
                            }
                            4 => {
                                if let Some(idx) = duration_options.iter().position(|&x| x == settings.flames_duration) {
                                    settings.flames_duration = if idx > 0 { duration_options[idx - 1] } else { duration_options[duration_options.len() - 1] };
                                }
                            }
                            5 => {
                                let next = ((settings.duration - 0.1) * 10.0).round() / 10.0;
                                settings.duration = if next >= 0.1 { next } else { 5.0 };
                            }
                            _ => {}
                        }
                    }
                    Key::Right if active_tab == 1 => {
                        let fps_options = [15, 30, 45, 60, 75, 90, 120];
                        let duration_options = [0.10, 0.20, 0.30, 0.38, 0.50, 0.70];
                        match selected_setting_idx {
                            0 => {
                                if let Some(idx) = fps_options.iter().position(|&x| x == settings.fps) {
                                    settings.fps = if idx + 1 < fps_options.len() { fps_options[idx + 1] } else { fps_options[0] };
                                }
                            }
                            1 => {
                                settings.wind = if settings.wind < 2 { settings.wind + 1 } else { -2 };
                            }
                            2 => {
                                settings.height = if settings.height < 3 { settings.height + 1 } else { 0 };
                            }
                            3 => {
                                settings.direction = !settings.direction;
                            }
                            4 => {
                                if let Some(idx) = duration_options.iter().position(|&x| x == settings.flames_duration) {
                                    settings.flames_duration = if idx + 1 < duration_options.len() { duration_options[idx + 1] } else { duration_options[0] };
                                }
                            }
                            5 => {
                                let next = ((settings.duration + 0.1) * 10.0).round() / 10.0;
                                settings.duration = if next <= 5.0 { next } else { 0.1 };
                            }
                            _ => {}
                        }
                    }
                    Key::Enter => {
                        // Confirm active choices and return them to caller
                        match active_tab {
                            0 => {
                                let filter = apply_filter(&search_query);
                                if let Some(&fi) = filter.get(selected_palette_idx) {
                                    if fi < NAMED_PALETTES.len() {
                                        let (id, _, _, _, _) = NAMED_PALETTES[fi];
                                        return Some((PaletteChoice::Named(id.to_string()), settings));
                                    } else {
                                        // Trigger inline custom editor fallback
                                        let base = rows as u16 - 4;
                                        print!("{ESC}[{base};1H{ESC}[J");
                                        println!("{ESC}[{base};1H{ESC}[38;2;175;175;200m  Enter hex colors (e.g. #ff0000){ESC}[0m");
                                        io::stdout().flush().ok();
                                        if let Some(from_str) = prompt_hex("  From:", base + 2) {
                                            if let Some(to_str) = prompt_hex("  To:  ", base + 3) {
                                                if let (Some(from), Some(to)) = (hex_to_rgb(&from_str), hex_to_rgb(&to_str)) {
                                                    return Some((PaletteChoice::Custom { from, to }, settings));
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                            1 => {
                                // Settings save & run - resolve correct palette target
                                if palette_source_is_custom {
                                    if !custom_entries.is_empty() && selected_custom_idx < custom_entries.len() {
                                        let entry = &custom_entries[selected_custom_idx];
                                        if let Some(choice) = entry.to_palette_choice() {
                                            return Some((choice, settings));
                                        }
                                    }
                                } else {
                                    let filter = apply_filter(&search_query);
                                    if let Some(&fi) = filter.get(selected_palette_idx) {
                                        if fi < NAMED_PALETTES.len() {
                                            let (id, _, _, _, _) = NAMED_PALETTES[fi];
                                            return Some((PaletteChoice::Named(id.to_string()), settings));
                                        }
                                    }
                                }
                                return Some((PaletteChoice::Named("fire".to_string()), settings));
                            }
                            2 => {
                                // Custom list confirm
                                if !custom_entries.is_empty() && selected_custom_idx < custom_entries.len() {
                                    let entry = &custom_entries[selected_custom_idx];
                                    if let Some(choice) = entry.to_palette_choice() {
                                        return Some((choice, settings));
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                    _ => {}
                }
            }
        }

        // 3. Ticking and Rendering Loop (Target: 30 FPS)
        if last_tick.elapsed() >= tick_duration {
            let current_size = terminal_size();
            if current_size != (cols, rows) {
                cols = current_size.0;
                rows = current_size.1;
                print!("{ESC}[H{ESC}[2J"); // clear screen on resizing
            }

            // Render Warning Screen if Window is Too Small
            if cols < 80 || rows < 20 {
                let mut buf = String::new();
                let box_w = 60usize.min(cols.saturating_sub(4));
                let box_h = 12usize.min(rows.saturating_sub(2));
                let start_x = (cols.saturating_sub(box_w)) / 2;
                let start_y = (rows.saturating_sub(box_h)) / 2;

                buf.push_str(&format!("{ESC}[H{ESC}[2J"));
                // outer box
                let top_line = format!("╭─{}─╮", "─".repeat(box_w.saturating_sub(4)));
                buf.push_str(&format!("{ESC}[{};{}H{ESC}[38;2;85;85;115m{}{ESC}[0m", start_y, start_x, top_line));
                for i in 1..box_h.saturating_sub(1) {
                    buf.push_str(&format!(
                        "{ESC}[{};{}H{ESC}[38;2;85;85;115m│{}{}│{ESC}[0m",
                        start_y + i,
                        start_x,
                        " ".repeat(box_w.saturating_sub(2)),
                        ""
                    ));
                }
                let bot_line = format!("╰─{}─╯", "─".repeat(box_w.saturating_sub(4)));
                buf.push_str(&format!("{ESC}[{};{}H{ESC}[38;2;85;85;115m{}{ESC}[0m", start_y + box_h - 1, start_x, bot_line));

                let msg1 = "Terminal size too small!";
                let msg2 = &format!("Current size: {}x{}", cols, rows);
                let msg3 = "Please resize to at least 80x20.";
                let msg4 = "Press [q] or [Esc] to exit.";

                buf.push_str(&format!("{ESC}[{};{}H{ESC}[1;38;2;255;70;70m{}{ESC}[0m", start_y + 2, start_x + (box_w.saturating_sub(msg1.len())) / 2, msg1));
                buf.push_str(&format!("{ESC}[{};{}H{ESC}[38;2;150;150;180m{}{ESC}[0m", start_y + 4, start_x + (box_w.saturating_sub(msg2.len())) / 2, msg2));
                buf.push_str(&format!("{ESC}[{};{}H{ESC}[38;2;150;150;180m{}{ESC}[0m", start_y + 6, start_x + (box_w.saturating_sub(msg3.len())) / 2, msg3));
                buf.push_str(&format!("{ESC}[{};{}H{ESC}[38;2;90;90;120m{}{ESC}[0m", start_y + 8, start_x + (box_w.saturating_sub(msg4.len())) / 2, msg4));

                print!("{buf}");
                io::stdout().flush().ok();

                last_tick = Instant::now();
                continue;
            }

            // Draw Unified Dashboard
            let mut buf = String::with_capacity(cols * rows * 24);

            // Row 1: Outer top frame border
            let left_w = 42usize;
            let right_w = cols.saturating_sub(49);
            let top_border = format!(
                "╭─{}─┬─{}─╮",
                "─".repeat(left_w),
                "─".repeat(right_w)
            );
            buf.push_str(&format!("{ESC}[1;1H{ESC}[38;2;60;60;85m{}{ESC}[0m", top_border));

            // Row 2: Tab Bar
            let tab0_str = if active_tab == 0 {
                format!("{ESC}[48;2;255;165;45m{ESC}[1;38;2;15;15;28m 🎨 Browse Palettes {ESC}[0m")
            } else {
                format!("{ESC}[38;2;160;160;185m 🎨 Browse Palettes {ESC}[0m")
            };
            let tab1_str = if active_tab == 1 {
                format!("{ESC}[48;2;255;165;45m{ESC}[1;38;2;15;15;28m ⚙️ Physics Settings {ESC}[0m")
            } else {
                format!("{ESC}[38;2;160;160;185m ⚙️ Physics Settings {ESC}[0m")
            };
            let tab2_str = if active_tab == 2 {
                format!("{ESC}[48;2;255;165;45m{ESC}[1;38;2;15;15;28m 🛠️ Custom Gradients {ESC}[0m")
            } else {
                format!("{ESC}[38;2;160;160;185m 🛠️ Custom Gradients {ESC}[0m")
            };

            let tabs_combined = format!("{}  {}  {}", tab0_str, tab1_str, tab2_str);
            let help_str = format!("{ESC}[38;2;90;90;120m[h] Help{ESC}[0m");
            let inner_w = cols.saturating_sub(4);
            let left_vis = visual_width(&tabs_combined);
            let right_vis = visual_width(&help_str);
            let space_count = inner_w.saturating_sub(left_vis + right_vis);
            let row2_content = format!("{}{}{}", tabs_combined, " ".repeat(space_count), help_str);
            buf.push_str(&format!(
                "{ESC}[2;1H{ESC}[38;2;60;60;85m│{ESC}[0m {} {ESC}[38;2;60;60;85m│{ESC}[0m",
                pad_right(&row2_content, cols.saturating_sub(4))
            ));

            // Row 3: Horizontal separator
            let middle_border = format!(
                "├─{}─┼─{}─┤",
                "─".repeat(left_w),
                "─".repeat(right_w)
            );
            buf.push_str(&format!("{ESC}[3;1H{ESC}[38;2;60;60;85m{}{ESC}[0m", middle_border));

            // 4. Generate Content Lines for Left Pane
            let mut left_lines = Vec::new();
            let body_h = rows.saturating_sub(5);

            match active_tab {
                0 => {
                    // Palettes Browsing Tab
                    let search_bar = if search_query.is_empty() && !search_active {
                        format!("{ESC}[38;2;55;55;75mSearch: / {ESC}[38;2;55;55;70m(press / to start search){ESC}[0m")
                    } else {
                        let caret = if search_active { "_" } else { "" };
                        format!("{ESC}[38;2;255;165;45mSearch: /{ESC}[1;38;2;255;255;255m{}{}{ESC}[0m", search_query, caret)
                    };
                    left_lines.push(pad_right(&search_bar, left_w));
                    left_lines.push(pad_right(&format!("{ESC}[38;2;45;45;65m{} {ESC}[0m", "╌".repeat(left_w)), left_w));

                    // List of visible named palettes
                    let filter = apply_filter(&search_query);
                    let list_rows = body_h.saturating_sub(2);
                    let offset = if selected_palette_idx >= list_rows {
                        selected_palette_idx - list_rows + 1
                    } else {
                        0
                    };

                    for slot in 0..list_rows {
                        let fi_pos = slot + offset;
                        if fi_pos >= filter.len() {
                            left_lines.push(pad_right("", left_w));
                            continue;
                        }
                        let fi = filter[fi_pos];
                        let is_sel = fi_pos == selected_palette_idx;
                        let indicator = if is_sel {
                            format!("{ESC}[1;38;2;255;200;80m▸ {ESC}[0m")
                        } else {
                            "  ".to_string()
                        };

                        let (display_name, swatch_str, hex_str) = if fi < NAMED_PALETTES.len() {
                            let (id, display, _, fh, th) = NAMED_PALETTES[fi];
                            let p = if id == "fire" {
                                soften(&FIRE_PALETTE, SOFTEN_DESATURATE, SOFTEN_BRIGHTEN)
                            } else {
                                let from = hex_to_rgb(fh).unwrap_or((0, 0, 0));
                                let to = hex_to_rgb(th).unwrap_or((255, 255, 255));
                                soften(&generate_palette(from, to), SOFTEN_DESATURATE, SOFTEN_BRIGHTEN)
                            };
                            (
                                display.to_string(),
                                palette_swatch(&p, 10),
                                format!("{ESC}[38;2;90;90;110m{}→{}{ESC}[0m", &fh[1..], &th[1..]),
                            )
                        } else {
                            (
                                "Custom".to_string(),
                                swatch((50, 0, 60), (255, 180, 80), 10),
                                format!("{ESC}[38;2;90;90;110mcustom gradient{ESC}[0m"),
                            )
                        };

                        let name_styled = if is_sel {
                            format!("{ESC}[1;38;2;255;255;255m{:<14}{ESC}[0m", truncate_display(&display_name, 14))
                        } else {
                            format!("{ESC}[38;2;160;160;180m{:<14}{ESC}[0m", truncate_display(&display_name, 14))
                        };

                        let entry_line = format!("{}{}{}  {}", indicator, name_styled, swatch_str, hex_str);
                        left_lines.push(pad_right(&entry_line, left_w));
                    }
                }
                1 => {
                    // Physics Settings Tab
                    left_lines.push(format_setting_row(
                        "FPS / Speed",
                        &format!("{} fps", settings.fps),
                        selected_setting_idx == 0,
                    ));
                    left_lines.push(pad_right("", left_w));

                    let wind_label = match settings.wind {
                        -2 => "Strong Left",
                        -1 => "Gentle Left",
                        0 => "None",
                        1 => "Gentle Right",
                        2 => "Strong Right",
                        _ => "None",
                    };
                    left_lines.push(format_setting_row(
                        "Wind / Breeze",
                        wind_label,
                        selected_setting_idx == 1,
                    ));
                    left_lines.push(pad_right("", left_w));

                    let height_label = match settings.height {
                        0 => "Low",
                        1 => "Medium",
                        2 => "High",
                        3 => "Extreme",
                        _ => "Medium",
                    };
                    left_lines.push(format_setting_row(
                        "Flame Height",
                        height_label,
                        selected_setting_idx == 2,
                    ));
                    left_lines.push(pad_right("", left_w));

                    left_lines.push(format_setting_row(
                        "Fire Direction",
                        if settings.direction { "Top → Bottom" } else { "Bottom → Top" },
                        selected_setting_idx == 3,
                    ));
                    left_lines.push(pad_right("", left_w));

                    let decay_speed = match settings.flames_duration {
                        0.10 => "Very Fast",
                        0.20 => "Fast",
                        0.30 => "Normal",
                        0.38 => "Default",
                        0.50 => "Slow",
                        0.70 => "Very Slow",
                        _ => "Normal",
                    };
                    left_lines.push(format_setting_row(
                        "Fire Decay",
                        decay_speed,
                        selected_setting_idx == 4,
                    ));
                    left_lines.push(pad_right("", left_w));

                    left_lines.push(format_setting_row(
                        "Duration",
                        &format!("{:.1}s", settings.duration),
                        selected_setting_idx == 5,
                    ));

                    // Fill remainder rows
                    while left_lines.len() < body_h {
                        left_lines.push(pad_right("", left_w));
                    }
                }
                2 => {
                    // Custom Palettes Tab
                    if let Some(ref prompt) = prompt_state {
                        // Drawing non-blocking inline guided form input
                        left_lines.push(pad_right(&format!("{ESC}[1;38;2;255;165;45m🛠️  Create Custom Palette{ESC}[0m"), left_w));
                        left_lines.push(pad_right(&format!("{ESC}[38;2;60;60;80m{} {ESC}[0m", "━".repeat(left_w)), left_w));
                        left_lines.push(pad_right("", left_w));

                        let slug_active = prompt.step == PromptStep::Slug;
                        left_lines.push(pad_right(&format!("  Slug ID: {}", if slug_active { "" } else { &prompt.slug }), left_w));
                        if slug_active {
                            left_lines.push(pad_right(&format!("  {ESC}[48;2;25;25;44m> {}{} {ESC}[0m", prompt.input_buffer, "_"), left_w));
                        }
                        left_lines.push(pad_right("", left_w));

                        let disp_active = prompt.step == PromptStep::Display;
                        left_lines.push(pad_right(&format!("  Name:    {}", if disp_active || slug_active { "" } else { &prompt.display }), left_w));
                        if disp_active {
                            left_lines.push(pad_right(&format!("  {ESC}[48;2;25;25;44m> {}{} {ESC}[0m", prompt.input_buffer, "_"), left_w));
                        }
                        left_lines.push(pad_right("", left_w));

                        let from_active = prompt.step == PromptStep::From;
                        left_lines.push(pad_right(&format!("  From Color (hex): {}", if from_active || disp_active || slug_active { "" } else { &prompt.from }), left_w));
                        if from_active {
                            left_lines.push(pad_right(&format!("  {ESC}[48;2;25;25;44m> {}{} {ESC}[0m", prompt.input_buffer, "_"), left_w));
                        }
                        left_lines.push(pad_right("", left_w));

                        let to_active = prompt.step == PromptStep::To;
                        left_lines.push(pad_right(&format!("  To Color (hex):   {}", if to_active || from_active || disp_active || slug_active { "" } else { &prompt.to }), left_w));
                        if to_active {
                            left_lines.push(pad_right(&format!("  {ESC}[48;2;25;25;44m> {}{} {ESC}[0m", prompt.input_buffer, "_"), left_w));
                        }

                        // Fill remainder rows
                        while left_lines.len() < body_h {
                            left_lines.push(pad_right("", left_w));
                        }
                    } else {
                        // Regular custom list mode
                        left_lines.push(pad_right(&format!("{ESC}[38;2;90;90;120m[n] Create New     [d] Delete selected{ESC}[0m"), left_w));
                        left_lines.push(pad_right(&format!("{ESC}[38;2;45;45;65m{} {ESC}[0m", "╌".repeat(left_w)), left_w));

                        let list_rows = body_h.saturating_sub(2);
                        let offset = if selected_custom_idx >= list_rows {
                            selected_custom_idx - list_rows + 1
                        } else {
                            0
                        };

                        if custom_entries.is_empty() {
                            left_lines.push(pad_right("  No custom gradients saved yet.", left_w));
                            left_lines.push(pad_right("  Press 'n' to create one!", left_w));
                        } else {
                            for slot in 0..list_rows {
                                let idx = slot + offset;
                                if idx >= custom_entries.len() {
                                    left_lines.push(pad_right("", left_w));
                                    continue;
                                }
                                let is_sel = idx == selected_custom_idx;
                                let entry = &custom_entries[idx];

                                let indicator = if is_sel {
                                    format!("{ESC}[1;38;2;255;200;80m▸ {ESC}[0m")
                                } else {
                                    "  ".to_string()
                                };

                                let name_styled = if is_sel {
                                    format!("{ESC}[1;38;2;255;255;255m{:<14}{ESC}[0m", truncate_display(&entry.display, 14))
                                } else {
                                    format!("{ESC}[38;2;160;160;180m{:<14}{ESC}[0m", truncate_display(&entry.display, 14))
                                };

                                let from_rgb = hex_to_rgb(&entry.from).unwrap_or((0, 0, 0));
                                let to_rgb = hex_to_rgb(&entry.to).unwrap_or((255, 255, 255));
                                let p = soften(&generate_palette(from_rgb, to_rgb), SOFTEN_DESATURATE, SOFTEN_BRIGHTEN);
                                let sw = palette_swatch(&p, 10);
                                let hex_str = format!("{ESC}[38;2;90;90;110m{}→{}{ESC}[0m", &entry.from[1..], &entry.to[1..]);

                                let entry_line = format!("{}{}{}  {}", indicator, name_styled, sw, hex_str);
                                left_lines.push(pad_right(&entry_line, left_w));
                            }
                        }
                    }
                }
                _ => {}
            }

            // 5. Generate Content Lines for Right Pane (Preview and Specs)
            let mut right_lines = Vec::new();

            // Calculate responsive sizes - fire preview box can now scale up to 32 rows tall
            let prev_box_h = body_h.saturating_sub(6).clamp(10, 32);
            let specs_box_h = body_h.saturating_sub(prev_box_h + 2);


            // Dynamic live fire preview tick & render based on active source type
            let (preview_id, preview_disp, preview_fh, preview_th) = if palette_source_is_custom && !custom_entries.is_empty() && selected_custom_idx < custom_entries.len() {
                let entry = &custom_entries[selected_custom_idx];
                ("custom".to_string(), entry.display.clone(), entry.from.clone(), entry.to.clone())
            } else {
                get_current_palette(selected_palette_idx, &search_query, &custom_entries)

            };

            let preview_pal = if preview_id == "fire" {
                soften(&FIRE_PALETTE, SOFTEN_DESATURATE, SOFTEN_BRIGHTEN)
            } else {
                let from = hex_to_rgb(&preview_fh).unwrap_or((0, 0, 0));
                let to = hex_to_rgb(&preview_th).unwrap_or((255, 255, 255));
                soften(&generate_palette(from, to), SOFTEN_DESATURATE, SOFTEN_BRIGHTEN)
            };

            let prev_fire_w = right_w.saturating_sub(4);
            preview_fire.resize(prev_fire_w, prev_box_h);
            preview_fire.tick(&settings, &mut rng);

            // Draw upper box: live fire preview
            let prev_title = " Live Fire Preview ";
            let prev_top = format!("╭─{}{}{}╮", prev_title, "─".repeat(right_w.saturating_sub(prev_title.len() + 4)), "─");
            right_lines.push(format!("{ESC}[38;2;60;60;85m{}{ESC}[0m", prev_top));

            let fire_lines = preview_fire.render_lines(&preview_pal);
            for fire_line in fire_lines {
                right_lines.push(format!(
                    "{ESC}[38;2;60;60;85m│{ESC}[0m {} {ESC}[38;2;60;60;85m│{ESC}[0m",
                    pad_right(&fire_line, right_w.saturating_sub(2))
                ));
            }
            let prev_bot = format!("╰{}╯", "─".repeat(right_w.saturating_sub(2)));
            right_lines.push(format!("{ESC}[38;2;60;60;85m{}{ESC}[0m", prev_bot));

            // Draw lower box: detailed specs
            let specs_title = " Palette Specs ";
            let specs_top = format!("╭─{}{}{}╮", specs_title, "─".repeat(right_w.saturating_sub(specs_title.len() + 4)), "─");
            right_lines.push(format!("{ESC}[38;2;60;60;85m{}{ESC}[0m", specs_top));

            let mut spec_content = Vec::new();
            spec_content.push(format!("{ESC}[38;2;255;165;45mName:    {ESC}[1;38;2;255;255;255m{}{ESC}[0m", preview_disp));
            spec_content.push(format!("{ESC}[38;2;255;165;45mID/Slug: {ESC}[38;2;170;170;190m{}{ESC}[0m", preview_id));
            spec_content.push(format!("{ESC}[38;2;255;165;45mColors:  {ESC}[38;2;90;90;120m{} {ESC}[38;2;60;60;85m→{ESC}[38;2;90;90;120m {}{ESC}[0m", preview_fh, preview_th));
            spec_content.push(format!("{ESC}[38;2;255;165;45mPhysics: {ESC}[38;2;140;140;160m{} FPS | Height: {}{ESC}[0m", settings.fps, match settings.height { 0 => "Low", 1 => "Medium", 2 => "High", 3 => "Extreme", _ => "Medium" }));

            for i in 0..specs_box_h.saturating_sub(2) {
                let text = if i < spec_content.len() { &spec_content[i] } else { "" };
                right_lines.push(format!(
                    "{ESC}[38;2;60;60;85m│{ESC}[0m  {}  {ESC}[38;2;60;60;85m│{ESC}[0m",
                    pad_right(text, right_w.saturating_sub(4))
                ));
            }
            let specs_bot = format!("╰{}╯", "─".repeat(right_w.saturating_sub(2)));
            right_lines.push(format!("{ESC}[38;2;60;60;85m{}{ESC}[0m", specs_bot));

            // 6. Zip Left and Right Panes into Body rows
            for y in 0..body_h {
                let left_content = left_lines.get(y).cloned().unwrap_or_else(|| pad_right("", left_w));
                let right_content = right_lines.get(y).cloned().unwrap_or_else(|| pad_right("", right_w));
                buf.push_str(&format!(
                    "{ESC}[{};1H{ESC}[38;2;60;60;85m│{ESC}[0m {} {ESC}[38;2;60;60;85m│{ESC}[0m {} {ESC}[38;2;60;60;85m│{ESC}[0m",
                    y + 4,
                    left_content,
                    right_content
                ));
            }

            // 7. Bottom frame border
            let bot_row = rows.saturating_sub(1);
            let bot_border = format!(
                "╰─{}─┴─{}─╯",
                "─".repeat(left_w),
                "─".repeat(right_w)
            );
            buf.push_str(&format!("{ESC}[{};1H{ESC}[38;2;60;60;85m{}{ESC}[0m", bot_row, bot_border));

            // 8. Footer hints
            let footer_row = rows;
            let hints_line = if prompt_state.is_some() {
                format!(
                    "{ESC}[48;2;15;15;28m \
                     {ESC}[38;2;255;200;80mEnter{ESC}[38;2;98;98;128m submit field  \
                     {ESC}[38;2;255;200;80mEsc{ESC}[38;2;98;98;128m cancel  \
                     {ESC}[0m"
                )
            } else if search_active {
                format!(
                    "{ESC}[48;2;15;15;28m \
                     {ESC}[38;2;255;200;80mEnter/Esc{ESC}[38;2;98;98;128m finish search  \
                     {ESC}[38;2;255;200;80mBackspace/Chars{ESC}[38;2;98;98;128m type query  \
                     {ESC}[0m"
                )
            } else {
                format!(
                    "{ESC}[48;2;15;15;28m \
                     {ESC}[38;2;255;200;80mTab/1-3{ESC}[38;2;98;98;128m tabs  \
                     {ESC}[38;2;255;200;80m↑↓{ESC}[38;2;98;98;128m move  \
                     {ESC}[38;2;255;200;80m←→{ESC}[38;2;98;98;128m adjust  \
                     {ESC}[38;2;255;200;80mEnter{ESC}[38;2;98;98;128m save & run  \
                     {ESC}[38;2;255;200;80mh{ESC}[38;2;98;98;128m help  \
                     {ESC}[38;2;255;200;80mEsc/q{ESC}[38;2;98;98;128m quit \
                     {ESC}[0m"
                )
            };
            buf.push_str(&format!(
                "{ESC}[{};1H{ESC}[2K{}",
                footer_row,
                pad_right(&hints_line, cols)
            ));

            // 9. Overlay Keyboard Help Modal Card
            if show_help {
                let help_w = 56usize;
                let help_h = 14usize;
                let help_x = (cols.saturating_sub(help_w)) / 2;
                let help_y = (rows.saturating_sub(help_h)) / 2;

                // Help box background shading
                for i in 0..help_h {
                    buf.push_str(&format!(
                        "{ESC}[{};{}H{ESC}[48;2;20;20;32m{}{ESC}[0m",
                        help_y + i,
                        help_x,
                        " ".repeat(help_w)
                    ));
                }

                // Help box borders
                let h_top = format!("╭─ Help & Keyboard Bindings ─{}╮", "─".repeat(help_w.saturating_sub(30)));
                buf.push_str(&format!("{ESC}[{};{}H{ESC}[1;38;2;255;165;45m{}{ESC}[0m", help_y, help_x, h_top));
                for i in 1..help_h.saturating_sub(1) {
                    buf.push_str(&format!(
                        "{ESC}[{};{}H{ESC}[38;2;120;120;145m│{ESC}[0m{ESC}[{};{}H{ESC}[38;2;120;120;145m│{ESC}[0m",
                        help_y + i,
                        help_x,
                        help_y + i,
                        help_x + help_w - 1
                    ));
                }
                let h_bot = format!("╰{}╯", "─".repeat(help_w.saturating_sub(2)));
                buf.push_str(&format!("{ESC}[{};{}H{ESC}[38;2;120;120;145m{}{ESC}[0m", help_y + help_h - 1, help_x, h_bot));

                // Help text items
                let help_items = [
                    ("Tab / 1-3", "Switch between main Dashboard tabs"),
                    ("Up / Down", "Move selection in lists and settings"),
                    ("PageUp/Dn", "Page through palette lists rapidly"),
                    ("Left/Right", "Adjust physics sliders/toggle direction"),
                    ("/", "Start searching palettes (Tab 1)"),
                    ("n / N", "Create a new custom palette (Tab 3)"),
                    ("d / D", "Delete selected custom palette (Tab 3)"),
                    ("r / R", "Pick a random item under current tab"),
                    ("Enter", "Apply active choices, save to disk & RUN"),
                    ("Esc / q", "Discard adjustments and exit dashboard"),
                ];

                for (idx, (keys, action)) in help_items.iter().enumerate() {
                    let r = help_y + 2 + idx;
                    buf.push_str(&format!(
                        "{ESC}[{};{}H  {ESC}[1;38;2;255;200;80m{:<12}{ESC}[0m {ESC}[38;2;170;170;190m{}{ESC}[0m",
                        r,
                        help_x + 2,
                        keys,
                        action
                    ));
                }
            }

            // Single atomic write to stdout
            print!("{buf}");
            io::stdout().flush().ok();

            last_tick = Instant::now();
        }
    }
}

// ── Obsolete shims wrapping the new unified dashboard ─────────────────

pub fn interactive_pick(current_settings: AnimSettings) -> Option<(PaletteChoice, AnimSettings)> {
    if let Some((new_choice, new_settings)) = run_dashboard(0, None, current_settings) {
        if !crate::config::has_no_save() {
            crate::config::save_config(&new_choice, &new_settings);
        }
        Some((new_choice, new_settings))
    } else {
        None
    }
}

pub fn interactive_settings(current: &AnimSettings) -> Option<AnimSettings> {
    let (saved_choice, _) = crate::config::load_config();
    let choice = saved_choice.unwrap_or(PaletteChoice::Named("fire".to_string()));
    if let Some((new_choice, new_settings)) = run_dashboard(1, Some(choice), current.clone()) {
        if !crate::config::has_no_save() {
            crate::config::save_config(&new_choice, &new_settings);
        }
        Some(new_settings)
    } else {
        None
    }
}

pub fn interactive_custom(current_settings: AnimSettings) -> Option<(PaletteChoice, AnimSettings)> {
    if let Some((new_choice, new_settings)) = run_dashboard(2, None, current_settings) {
        if !crate::config::has_no_save() {
            crate::config::save_config(&new_choice, &new_settings);
        }
        Some((new_choice, new_settings))
    } else {
        None
    }
}

