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
    pub direction: u8,   // 0 = Bottom→Top (default), 1 = Top→Bottom, 2 = Left→Right, 3 = Right→Left
    pub duration: f32,   // 1 = 1 seconds, 1.2 = 1.2 seconds .. 5 = 5 seconds
    pub flames_duration: f32,   // 0 = stops instantly, 1 = stops at the end of the animation
    pub effect: String,  // one of engine::EFFECTS, or "random"
    pub wave_speed: u8,  // 1 = Very Slow .. 3 = Normal .. 5 = Very Fast
    pub wave_angle: i32, // crest tilt in degrees, -30..=30; 0 = level
    pub vortex_outward: bool, // false = arms sweep in to the center, true = out from it
    pub vortex_speed: u8, // 1 = Very Slow .. 3 = Normal .. 5 = Very Fast
    pub vortex_clockwise: bool, // galaxy spin direction on screen
    pub explosion_count: u8,  // 1 = Single, 2 = Few, 3 = Some, 4 = Many, 5 = Barrage
    pub explosion_speed: u8,  // 1 = Very Slow .. 3 = Normal .. 5 = Very Fast
    pub explosion_rhythm: u8, // 0 = Together, 1 = Staggered, 2 = Chain (left → right)
    pub explosion_debris: u8, // 0 = None, 1 = Some, 2 = Lots
}

impl Default for AnimSettings {
    fn default() -> Self {
        Self {
            fps: 60,
            wind: 0,
            height: 1,
            direction: 0,
            duration: 2.2,
            flames_duration: 0.38,
            effect: "fire".to_string(),
            wave_speed: 3,
            wave_angle: 0,
            vortex_outward: false,
            vortex_speed: 3,
            vortex_clockwise: true,
            explosion_count: 3,
            explosion_speed: 3,
            explosion_rhythm: 1,
            explosion_debris: 1,
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

/// Reads an env var, treating unset and empty as the same thing.
fn env_path(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|v| !v.trim().is_empty())
}

/// The user's home directory.
///
/// On Windows `HOME` is normally unset (only MSYS/Git Bash sets it, and to a
/// POSIX path like `/c/Users/x` that native Windows APIs can't open), so
/// `USERPROFILE` is preferred there, with `HOMEDRIVE` + `HOMEPATH` as a
/// fallback for the rare setups that only define those.
fn home_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        if let Some(profile) = env_path("USERPROFILE") {
            return Some(PathBuf::from(profile));
        }
        if let (Some(drive), Some(path)) = (env_path("HOMEDRIVE"), env_path("HOMEPATH")) {
            return Some(PathBuf::from(format!("{drive}{path}")));
        }
    }
    env_path("HOME").map(PathBuf::from)
}

pub fn config_dir() -> Option<PathBuf> {
    if let Some(xdg) = env_path("XDG_CONFIG_HOME") {
        return Some(PathBuf::from(xdg).join("pyroclear"));
    }
    Some(home_dir()?.join(".config").join("pyroclear"))
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
                    if let Ok(n) = val.parse::<u8>() {
                        settings.direction = n.clamp(0, 3);
                    }
                }
                "duration" =>  {
                    if let Ok(f) = val.parse::<f32>() {
                        settings.duration = f;
                    }
                }
                "flames_duration" =>  {
                    if let Ok(f) = val.parse::<f32>() {
                        settings.flames_duration = f;
                    }
                }
                "effect" if validate_effect(&val).is_ok() => settings.effect = val,
                "wave_speed" => {
                    if let Ok(n) = val.parse::<u8>() {
                        settings.wave_speed = n.clamp(1, 5);
                    }
                }
                "wave_angle" => {
                    if let Ok(n) = val.parse::<i32>() {
                        settings.wave_angle = n.clamp(-30, 30);
                    }
                }
                "vortex_outward" => {
                    if let Ok(b) = val.parse::<bool>() {
                        settings.vortex_outward = b;
                    }
                }
                "vortex_speed" => {
                    if let Ok(n) = val.parse::<u8>() {
                        settings.vortex_speed = n.clamp(1, 5);
                    }
                }
                "vortex_clockwise" => {
                    if let Ok(b) = val.parse::<bool>() {
                        settings.vortex_clockwise = b;
                    }
                }
                "explosion_count" => {
                    if let Ok(n) = val.parse::<u8>() {
                        settings.explosion_count = n.clamp(1, 5);
                    }
                }
                "explosion_speed" => {
                    if let Ok(n) = val.parse::<u8>() {
                        settings.explosion_speed = n.clamp(1, 5);
                    }
                }
                "explosion_rhythm" => {
                    if let Ok(n) = val.parse::<u8>() {
                        settings.explosion_rhythm = n.min(2);
                    }
                }
                "explosion_debris" => {
                    if let Ok(n) = val.parse::<u8>() {
                        settings.explosion_debris = n.min(2);
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
    content.push_str(&format!("fps              = {}\n", settings.fps));
    content.push_str(&format!("wind             = {}\n", settings.wind));
    content.push_str(&format!("height           = {}\n", settings.height));
    content.push_str(&format!("direction        = {}\n", settings.direction)); // 0=Bottom→Top 1=Top→Bottom 2=Left→Right 3=Right→Left
    content.push_str(&format!("duration         = {}\n", settings.duration));
    content.push_str(&format!("flames_duration  = {}\n", settings.flames_duration));
    content.push_str(&format!("effect           = \"{}\"\n", settings.effect));
    content.push_str(&format!("wave_speed       = {}\n", settings.wave_speed));
    content.push_str(&format!("wave_angle       = {}\n", settings.wave_angle));
    content.push_str(&format!("vortex_outward   = {}\n", settings.vortex_outward));
    content.push_str(&format!("vortex_speed     = {}\n", settings.vortex_speed));
    content.push_str(&format!("vortex_clockwise = {}\n", settings.vortex_clockwise));
    content.push_str(&format!("explosion_count  = {}\n", settings.explosion_count));
    content.push_str(&format!("explosion_speed  = {}\n", settings.explosion_speed));
    content.push_str(&format!("explosion_rhythm = {}\n", settings.explosion_rhythm)); // 0=Together 1=Staggered 2=Chain
    content.push_str(&format!("explosion_debris = {}\n", settings.explosion_debris)); // 0=None 1=Some 2=Lots
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

fn validate_effect(name: &str) -> Result<(), String> {
    if name == "random" || crate::engine::EFFECTS.contains(&name) {
        return Ok(());
    }
    Err(format!(
        "Unknown effect '{name}' (expected {} or random)",
        crate::engine::EFFECTS.join(", ")
    ))
}

/// The value of `--effect <name>`, if given. Exits on a missing or unknown name.
fn effect_arg() -> Option<String> {
    let args: Vec<String> = std::env::args().collect();
    let i = args.iter().position(|a| a == "--effect")?;
    let checked = match args.get(i + 1) {
        Some(name) => validate_effect(name).map(|()| name.clone()),
        None => Err("--effect needs a name".to_string()),
    };
    match checked {
        Ok(name) => Some(name),
        Err(e) => {
            eprintln!(
                "{ESC}[1;38;2;255;70;70m✗ error:{ESC}[0m {e}\n\
                 {ESC}[38;2;95;95;115m  tip: pyroclear --effect wave{ESC}[0m"
            );
            std::process::exit(1);
        }
    }
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

/// Parse CLI flags. Returns (parsed palette choice, optional overridden settings, run_settings flag, is_reset flag).
fn parse_args(saved_settings: &AnimSettings) -> (Option<PaletteChoice>, Option<AnimSettings>, bool, bool) {
    use crate::display::*;

    let args: Vec<String> = std::env::args().collect();
    let mut color = None;
    let mut from = None;
    let mut to = None;
    let mut run_settings = false;
    let mut is_reset = false;

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
            "--pick" | "-p" => match interactive_pick(saved_settings.clone()) {
                Some((c, new_settings)) => return (Some(c), Some(new_settings), false, false),
                None => std::process::exit(0),
            },
            "--settings" | "-s" => {
                run_settings = true;
            }
            "--random" | "-r" => {
                let c = random_palette_choice();
                return (Some(c), None, false, false);
            }
            "--reset" => {
                is_reset = true;
            }
            "--custom" => match interactive_custom(saved_settings.clone()) {
                Some((c, new_settings)) => return (Some(c), Some(new_settings), false, false),
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
        return (Some(PaletteChoice::Custom { from: fc, to: tc }), None, false, false);
    }

    if let Some(name) = color {
        if let Err(e) = validate_named(&name) {
            eprintln!(
                "{ESC}[1;38;2;255;70;70m✗ error:{ESC}[0m {e}\n\
                 {ESC}[38;2;95;95;115m  tip: run pyroclear --pick to browse all palettes{ESC}[0m"
            );
            std::process::exit(1);
        }
        return (Some(PaletteChoice::Named(name)), None, false, false);
    }

    (None, None, run_settings, is_reset)
}

/// Resolve the final (palette choice, animation settings) pair for this run.
pub fn resolve_choice() -> (PaletteChoice, AnimSettings) {
    let (saved_choice, mut saved_settings) = load_config();
    // Applied before any TUI opens, so the TUI starts on it and has the last word.
    let effect = effect_arg();
    if let Some(e) = &effect {
        saved_settings.effect = e.clone();
    }
    let (parsed_choice, parsed_settings, run_settings, is_reset) = parse_args(&saved_settings);

    if is_reset {
        let c = PaletteChoice::Named("fire".to_string());
        let default_settings = AnimSettings::default();
        if !has_no_save() {
            save_config(&c, &default_settings);
        }
        eprintln!(
            "  {ESC}[38;2;255;200;80m◆ Reset:{ESC}[0m \
             {ESC}[38;2;195;195;215mall settings reset to default (fire){ESC}[0m"
        );
        return (c, default_settings);
    }

    // Settings returned from the TUI (--pick, --custom) take priority over saved settings.
    let mut settings = parsed_settings.unwrap_or(saved_settings);
    let mut choice_opt = parsed_choice;

    if run_settings {
        if let Some((new_choice, new_settings)) = interactive_settings(&settings) {
            settings = new_settings;
            choice_opt = Some(new_choice);
        } else {
            std::process::exit(0);
        }
    }

    // A new palette, or `--effect` alone, is saved for future runs.
    if choice_opt.is_some() || effect.is_some() {
        let choice = choice_opt
            .or(saved_choice)
            .unwrap_or(PaletteChoice::Named("fire".to_string()));
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
