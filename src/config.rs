// config.rs — AnimSettings, PaletteChoice, config I/O, CLI parsing.

use crate::{
    palettes::*,
    tui::{interactive_custom, interactive_pick, interactive_settings},
    ESC,
};
use std::path::PathBuf;

// ── Data types ────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct AnimSettings {
    pub fps: u32,
    pub wind: i32,       // -2..=2 (Strong Left → Strong Right); 0 = None
    pub height: i32,     // 0 = Low, 1 = Medium, 2 = High, 3 = Extreme
    pub direction: bool, // false = bottom-up (default), true = top-down
}

impl Default for AnimSettings {
    fn default() -> Self {
        Self {
            fps: 60,
            wind: 0,
            height: 1,
            direction: false,
        }
    }
}

#[derive(Clone)]
pub enum PaletteChoice {
    Named(String),
    Custom {
        from: (u8, u8, u8),
        to: (u8, u8, u8),
    },
}

// ── Custom palette storage path ──────────────────────────────────────

pub fn config_dir() -> Option<PathBuf> {
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        return Some(PathBuf::from(xdg).join("pyroclear"));
    }
    let home = std::env::var("HOME").ok()?;
    Some(PathBuf::from(home).join(".config/pyroclear"))
}

pub fn config_path() -> Option<PathBuf> {
    Some(config_dir()?.join("config.toml"))
}

pub fn custom_palettes_path() -> Option<PathBuf> {
    Some(config_dir()?.join("custom_palettes.toml"))
}

// ── Custom palette entry ──────────────────────────────────────────────

#[derive(Clone, Debug)]
pub struct CustomPaletteEntry {
    pub name: String,    // slug / id (no spaces)
    pub display: String, // human-readable display name
    pub from: String,    // hex e.g. "#ff0000"
    pub to: String,      // hex e.g. "#ffffff"
}

impl CustomPaletteEntry {
    pub fn to_palette_choice(&self) -> Option<PaletteChoice> {
        let from = hex_to_rgb(&self.from)?;
        let to = hex_to_rgb(&self.to)?;
        Some(PaletteChoice::Custom { from, to })
    }
}

// ── Load / save ───────────────────────────────────────────────────────

pub fn load_config() -> (Option<PaletteChoice>, AnimSettings) {
    let mut choice = None;
    let mut settings = AnimSettings::default();
    let Some(path) = config_path() else {
        return (None, settings);
    };
    let Ok(content) = std::fs::read_to_string(path) else {
        return (None, settings);
    };

    let mut palette = None;
    let mut from = None;
    let mut to = None;

    for raw in content.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('[') || line.starts_with('#') {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            let val = v.trim().trim_matches('"').to_string();
            match k.trim() {
                "palette" => palette = Some(val),
                "from" => from = Some(val),
                "to" => to = Some(val),
                "fps" => {
                    if let Ok(n) = val.parse::<u32>() {
                        settings.fps = n.max(1);
                    }
                }
                "wind" => {
                    if let Ok(n) = val.parse::<i32>() {
                        settings.wind = n.clamp(-2, 2);
                    }
                }
                "height" => {
                    if let Ok(n) = val.parse::<i32>() {
                        settings.height = n.clamp(0, 3);
                    }
                }
                "direction" => {
                    if let Ok(b) = val.parse::<bool>() {
                        settings.direction = b;
                    }
                }
                _ => {}
            }
        }
    }

    if let (Some(f), Some(t)) = (from, to) {
        if let (Some(fc), Some(tc)) = (hex_to_rgb(&f), hex_to_rgb(&t)) {
            choice = Some(PaletteChoice::Custom { from: fc, to: tc });
        }
    } else if let Some(p) = palette {
        choice = Some(PaletteChoice::Named(p));
    }

    (choice, settings)
}

pub fn save_config(choice: &PaletteChoice, settings: &AnimSettings) {
    let Some(path) = config_path() else { return };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let mut content = String::new();
    match choice {
        PaletteChoice::Named(name) => {
            content.push_str("[color]\npalette = \"");
            content.push_str(name);
            content.push_str("\"\n");
        }
        PaletteChoice::Custom { from, to } => {
            content.push_str(&format!(
                "[color]\nfrom = \"#{:02x}{:02x}{:02x}\"\nto = \"#{:02x}{:02x}{:02x}\"\n",
                from.0, from.1, from.2, to.0, to.1, to.2
            ));
        }
    }
    content.push_str("\n[animation]\n");
    content.push_str(&format!("fps       = {}\n", settings.fps));
    content.push_str(&format!("wind      = {}\n", settings.wind));
    content.push_str(&format!("height    = {}\n", settings.height));
    content.push_str(&format!("direction = {}\n", settings.direction));
    let _ = std::fs::write(path, content);
}

// ── Custom palette I/O ────────────────────────────────────────────────

pub fn load_custom_palettes() -> Vec<CustomPaletteEntry> {
    let Some(path) = custom_palettes_path() else {
        return vec![];
    };
    let Ok(content) = std::fs::read_to_string(&path) else {
        return vec![];
    };

    let mut entries: Vec<CustomPaletteEntry> = Vec::new();
    let mut cur_name = String::new();
    let mut cur_display = String::new();
    let mut cur_from = String::new();
    let mut cur_to = String::new();

    let flush = |entries: &mut Vec<CustomPaletteEntry>,
                 name: &mut String,
                 display: &mut String,
                 from: &mut String,
                 to: &mut String| {
        if !name.is_empty() && !from.is_empty() && !to.is_empty() {
            let name_val = std::mem::take(name);
            let display_val = if display.is_empty() {
                name_val.clone()
            } else {
                std::mem::take(display)
            };
            entries.push(CustomPaletteEntry {
                name: name_val,
                display: display_val,
                from: std::mem::take(from),
                to: std::mem::take(to),
            });
        }
        name.clear();
        display.clear();
        from.clear();
        to.clear();
    };

    for raw in content.lines() {
        let line = raw.trim();
        if line == "[[palette]]" {
            flush(
                &mut entries,
                &mut cur_name,
                &mut cur_display,
                &mut cur_from,
                &mut cur_to,
            );
            continue;
        }
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            let val = v.trim().trim_matches('"').to_string();
            match k.trim() {
                "name" => cur_name = val,
                "display" => cur_display = val,
                "from" => cur_from = val,
                "to" => cur_to = val,
                _ => {}
            }
        }
    }
    flush(
        &mut entries,
        &mut cur_name,
        &mut cur_display,
        &mut cur_from,
        &mut cur_to,
    );
    entries
}

pub fn save_custom_palettes(entries: &[CustomPaletteEntry]) {
    let Some(path) = custom_palettes_path() else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let mut content = String::new();
    for e in entries {
        content.push_str("[[palette]]\n");
        content.push_str(&format!("name    = \"{}\"\n", e.name));
        content.push_str(&format!("display = \"{}\"\n", e.display));
        content.push_str(&format!("from    = \"{}\"\n", e.from));
        content.push_str(&format!("to      = \"{}\"\n", e.to));
        content.push('\n');
    }
    let _ = std::fs::write(path, content);
}

// ── Validation & helpers ──────────────────────────────────────────────

pub fn validate_named(name: &str) -> Result<(), String> {
    if NAMED_PALETTES.iter().any(|(id, _, _, _, _)| *id == name) {
        return Ok(());
    }
    let custom = load_custom_palettes();
    if custom.iter().any(|e| e.name == name) {
        return Ok(());
    }
    Err(format!("Unknown palette '{name}'"))
}

pub fn has_no_save() -> bool {
    std::env::args().any(|a| a == "--no-save")
}

pub fn random_palette_choice() -> PaletteChoice {
    use crate::engine::Rng;
    let mut rng = Rng::new();
    let idx = (rng.next_u64() % NAMED_PALETTES.len() as u64) as usize;
    let (id, _, _, _, _) = NAMED_PALETTES[idx];
    PaletteChoice::Named(id.to_string())
}

// ── Build final palette ───────────────────────────────────────────────

pub fn build_palette(choice: &PaletteChoice) -> Palette {
    let raw = match choice {
        PaletteChoice::Named(name) => match name.as_str() {
            "fire" => FIRE_PALETTE,
            other => {
                if let Some((_, _, _, from_hex, to_hex)) =
                    NAMED_PALETTES.iter().find(|(id, _, _, _, _)| *id == other)
                {
                    let from = hex_to_rgb(from_hex).unwrap_or((0, 0, 0));
                    let to = hex_to_rgb(to_hex).unwrap_or((255, 255, 255));
                    generate_palette(from, to)
                } else {
                    // Try looking in custom saved palettes
                    let custom = load_custom_palettes();
                    if let Some(entry) = custom.iter().find(|e| e.name == other) {
                        let from = hex_to_rgb(&entry.from).unwrap_or((0, 0, 0));
                        let to = hex_to_rgb(&entry.to).unwrap_or((255, 255, 255));
                        generate_palette(from, to)
                    } else {
                        FIRE_PALETTE
                    }
                }
            }
        },
        PaletteChoice::Custom { from, to } => generate_palette(*from, *to),
    };
    soften(&raw, SOFTEN_DESATURATE, SOFTEN_BRIGHTEN)
}

// ── CLI argument parsing ──────────────────────────────────────────────

/// Parse CLI flags. Returns (parsed palette choice, run_settings flag).
fn parse_args() -> (Option<PaletteChoice>, bool) {
    use crate::display::*;

    let args: Vec<String> = std::env::args().collect();
    let mut color = None;
    let mut from = None;
    let mut to = None;
    let mut run_settings = false;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--color" | "-c" => {
                i += 1;
                color = args.get(i).cloned();
            }
            "--from" => {
                i += 1;
                from = args.get(i).cloned();
            }
            "--to" => {
                i += 1;
                to = args.get(i).cloned();
            }
            "--list-colors" | "--list" => {
                print_color_list();
                std::process::exit(0);
            }
            "--pick" | "-p" => match interactive_pick() {
                Some(c) => return (Some(c), false),
                None => std::process::exit(0),
            },
            "--settings" | "-s" => {
                run_settings = true;
            }
            "--random" | "-r" => {
                let c = random_palette_choice();
                if let PaletteChoice::Named(ref name) = c {
                    if let Some((_, display, _, _, _)) = NAMED_PALETTES
                        .iter()
                        .find(|(id, _, _, _, _)| *id == name.as_str())
                    {
                        eprintln!(
                            "  {ESC}[38;2;255;200;80m◆ Random:{ESC}[0m \
                             {ESC}[1;38;2;255;255;255m{display}{ESC}[0m  \
                             {ESC}[38;2;95;95;115m({name}){ESC}[0m"
                        );
                    }
                }
                return (Some(c), false);
            }
            "--reset" => {
                let c = PaletteChoice::Named("fire".to_string());
                let default_settings = AnimSettings::default();
                save_config(&c, &default_settings);
                eprintln!(
                    "  {ESC}[38;2;255;200;80m◆ Reset:{ESC}[0m \
                     {ESC}[38;2;195;195;215mpalette reset to default (fire){ESC}[0m"
                );
                return (Some(c), false);
            }
            "--custom" => match interactive_custom() {
                Some(c) => return (Some(c), false),
                None => std::process::exit(0),
            },
            "--start" => {
                print_start();
                std::process::exit(0);
            }
            "--info" | "-i" => {
                print_info();
                std::process::exit(0);
            }
            "--version" | "-V" => {
                print_version();
                std::process::exit(0);
            }
            "-h" | "--help" => {
                print_help();
                std::process::exit(0);
            }
            "--no-save" => {} // detected separately via has_no_save()
            _ => {}
        }
        i += 1;
    }

    if from.is_some() || to.is_some() {
        let (Some(f), Some(t)) = (from, to) else {
            eprintln!(
                "{ESC}[1;38;2;255;70;70m✗ error:{ESC}[0m --from and --to must be used together\n\
                 {ESC}[38;2;95;95;115m  tip: pyroclear --from \"#1a0000\" --to \"#ffcc00\"{ESC}[0m"
            );
            std::process::exit(1);
        };
        let (Some(fc), Some(tc)) = (hex_to_rgb(&f), hex_to_rgb(&t)) else {
            eprintln!(
                "{ESC}[1;38;2;255;70;70m✗ error:{ESC}[0m Invalid hex color — expected format #rrggbb\n\
                 {ESC}[38;2;95;95;115m  tip: e.g. --from \"#ff0000\" --to \"#ffffff\"{ESC}[0m"
            );
            std::process::exit(1);
        };
        return (Some(PaletteChoice::Custom { from: fc, to: tc }), false);
    }

    if let Some(name) = color {
        if let Err(e) = validate_named(&name) {
            eprintln!(
                "{ESC}[1;38;2;255;70;70m✗ error:{ESC}[0m {e}\n\
                 {ESC}[38;2;95;95;115m  tip: run --list-colors or --pick to browse all palettes{ESC}[0m"
            );
            std::process::exit(1);
        }
        return (Some(PaletteChoice::Named(name)), false);
    }

    (None, run_settings)
}

/// Resolve the final (palette choice, animation settings) pair for this run.
pub fn resolve_choice() -> (PaletteChoice, AnimSettings) {
    let (saved_choice, mut settings) = load_config();
    let (parsed_choice, run_settings) = parse_args();

    if run_settings {
        if let Some(new_settings) = interactive_settings(&settings) {
            settings = new_settings;
            if !has_no_save() {
                let active_choice = parsed_choice
                    .clone()
                    .or_else(|| saved_choice.clone())
                    .unwrap_or(PaletteChoice::Named("fire".to_string()));
                save_config(&active_choice, &settings);
            }
        } else {
            std::process::exit(0);
        }
    }

    if let Some(choice) = parsed_choice {
        if !has_no_save() {
            save_config(&choice, &settings);
        }
        return (choice, settings);
    }

    (
        saved_choice.unwrap_or(PaletteChoice::Named("fire".to_string())),
        settings,
    )
}
