// display.rs — banner, help, info, start, and color-list output.

use crate::{
    config::{load_config, PaletteChoice},
    palettes::*,
    ESC,
};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

// ── Banner ────────────────────────────────────────────────────────────

pub fn print_banner() {
    let lines = [
        "  ██████╗ ██╗   ██╗██████╗  ██████╗  ██████╗██╗     ███████╗ █████╗ ██████╗ ",
        "  ██╔══██╗╚██╗ ██╔╝██╔══██╗██╔═══██╗██╔════╝██║     ██╔════╝██╔══██╗██╔══██╗",
        "  ██████╔╝ ╚████╔╝ ██████╔╝██║   ██║██║     ██║     █████╗  ███████║██████╔╝",
        "  ██╔═══╝   ╚██╔╝  ██╔══██╗██║   ██║██║     ██║     ██╔══╝  ██╔══██║██╔══██╗",
        "  ██║        ██║   ██║  ██║╚██████╔╝╚██████╗███████╗███████╗██║  ██║██║  ██║",
        "  ╚═╝        ╚═╝   ╚═╝  ╚═╝ ╚═════╝  ╚═════╝╚══════╝╚══════╝╚═╝  ╚═╝╚═╝  ╚═╝",
    ];
    let colors: [(u8, u8, u8); 6] = [
        (200, 8, 0),
        (228, 45, 0),
        (246, 90, 0),
        (252, 140, 5),
        (255, 185, 18),
        (255, 225, 48),
    ];
    for (line, &(r, g, b)) in lines.iter().zip(colors.iter()) {
        println!("{ESC}[38;2;{r};{g};{b}m{line}{ESC}[0m");
    }
    let n = NAMED_PALETTES.len();
    println!(
        "  {ESC}[38;2;85;85;108mv{VERSION}  \
         {ESC}[38;2;55;55;75m·  \
         {ESC}[38;2;85;85;108m{n} palettes  \
         {ESC}[38;2;55;55;75m·  \
         {ESC}[38;2;65;65;88mWatch your terminal go up in flames!{ESC}[0m"
    );
    println!();
}

// ── Version ───────────────────────────────────────────────────────────

pub fn print_version() {
    print_banner();
    let n = NAMED_PALETTES.len();
    println!("  {ESC}[38;2;130;130;155mVersion   {ESC}[1;38;2;255;220;80m{VERSION}{ESC}[0m");
    println!("  {ESC}[38;2;130;130;155mPalettes  {ESC}[1;38;2;255;220;80m{n}{ESC}[0m");
    println!("  {ESC}[38;2;130;130;155mLicense   {ESC}[38;2;110;110;135mMIT{ESC}[0m");
    println!();
}

// ── Info ──────────────────────────────────────────────────────────────

pub fn print_info() {
    let (choice, _) = load_config();
    let choice = choice.unwrap_or(PaletteChoice::Named("fire".to_string()));
    print_banner();
    println!("  {ESC}[1;38;2;255;200;80mActive Palette{ESC}[0m\n");
    match &choice {
        PaletteChoice::Named(name) => {
            if let Some((id, display, desc, from_hex, to_hex)) = NAMED_PALETTES
                .iter()
                .find(|(i, _, _, _, _)| *i == name.as_str())
            {
                let from = hex_to_rgb(from_hex).unwrap_or((0, 0, 0));
                let to = hex_to_rgb(to_hex).unwrap_or((255, 255, 255));
                let p = if *id == "fire" {
                    soften(&FIRE_PALETTE, SOFTEN_DESATURATE, SOFTEN_BRIGHTEN)
                } else {
                    soften(
                        &generate_palette(from, to),
                        SOFTEN_DESATURATE,
                        SOFTEN_BRIGHTEN,
                    )
                };
                let sw = palette_swatch(&p, 44);
                println!("  {ESC}[1;38;2;255;255;255m{display}{ESC}[0m  {ESC}[38;2;75;75;95m({id}){ESC}[0m");
                println!("  {sw}\n");
                println!("  {ESC}[38;2;160;160;185m{desc}{ESC}[0m");
                println!(
                    "  {ESC}[38;2;95;95;118mFrom{ESC}[0m  {ESC}[38;2;195;195;215m{from_hex}  \
                     {ESC}[38;2;95;95;118mTo{ESC}[0m  {ESC}[38;2;195;195;215m{to_hex}{ESC}[0m"
                );
            } else {
                println!("  {ESC}[38;2;195;195;215m{name}{ESC}[0m {ESC}[38;2;95;95;118m(no info){ESC}[0m");
            }
        }
        PaletteChoice::Custom { from, to } => {
            let fh = format!("#{:02x}{:02x}{:02x}", from.0, from.1, from.2);
            let th = format!("#{:02x}{:02x}{:02x}", to.0, to.1, to.2);
            let p = soften(
                &generate_palette(*from, *to),
                SOFTEN_DESATURATE,
                SOFTEN_BRIGHTEN,
            );
            let sw = palette_swatch(&p, 44);
            println!("  {ESC}[1;38;2;255;255;255mCustom gradient{ESC}[0m");
            println!("  {sw}\n");
            println!(
                "  {ESC}[38;2;95;95;118mFrom{ESC}[0m  {ESC}[38;2;195;195;215m{fh}  \
                 {ESC}[38;2;95;95;118mTo{ESC}[0m  {ESC}[38;2;195;195;215m{th}{ESC}[0m"
            );
        }
    }
    println!();
}

// ── Help ──────────────────────────────────────────────────────────────

pub fn print_help() {
    fn sec(title: &str) {
        println!("\n  {ESC}[1;38;2;255;200;80m{title}{ESC}[0m");
        println!("  {ESC}[38;2;45;45;65m{}{ESC}[0m", "─".repeat(60));
    }

    print_banner();
    let n = NAMED_PALETTES.len();

    sec("USAGE");
    println!("    {ESC}[38;2;200;200;220mpyroclear {ESC}[38;2;130;130;155m[OPTIONS]{ESC}[0m");
    println!(
        "    {ESC}[38;2;82;82;105m(no flags: burn with the saved palette, default is fire){ESC}[0m"
    );

    sec("MODES");
    let modes: &[(&str, &str)] = &[
        ("--color <name>", "Named palette  (saved for future runs)"),
        (
            "--from <hex> --to <hex>",
            "Custom gradient (saved for future runs)",
        ),
        ("--pick,   -p", "Interactive TUI color picker"),
        ("--settings, -s", "Interactive TUI settings page"),
        ("--custom", "Interactive custom palette manager (TUI)"),
        ("--random, -r", "Random palette — different every run"),
        ("--reset", "Reset to default (fire), then burn"),
    ];
    for (flag, desc) in modes {
        println!(
            "    {ESC}[38;2;255;165;45m{flag:<28}{ESC}[0m {ESC}[38;2;185;185;210m{desc}{ESC}[0m"
        );
    }

    sec("DISCOVERY");
    let list_desc = format!("Palette grid ({n} palettes) with live swatches");
    let disc: &[(&str, &str)] = &[
        ("--start", "Premium onboarding guide & setup"),
        ("--list-colors, --list", list_desc.as_str()),
        ("--info, -i", "Info card for the currently active palette"),
        ("--version, -V", "Version and palette count"),
        ("--help, -h", "Show this help"),
    ];
    for (flag, desc) in disc {
        println!(
            "    {ESC}[38;2;255;165;45m{flag:<28}{ESC}[0m {ESC}[38;2;185;185;210m{desc}{ESC}[0m"
        );
    }

    sec("OPTIONS");
    println!(
        "    {ESC}[38;2;255;165;45m--no-save{ESC}[0m  \
         {ESC}[38;2;82;82;105m(combine with any mode){ESC}[0m  \
         {ESC}[38;2;185;185;210mSkip writing the choice to config{ESC}[0m"
    );

    sec("EXAMPLES");
    let ex: &[(&str, &str)] = &[
        ("pyroclear --start", "interactive guide & onboarding"),
        ("pyroclear", "burn with saved / default palette"),
        ("pyroclear --color ocean", "burn ocean & save it"),
        (
            "pyroclear --settings",
            "configure speed, wind, height decay",
        ),
        ("pyroclear --custom", "manage and run saved custom palettes"),
        ("pyroclear --random", "random palette each run"),
        (
            "pyroclear --from \"#002080\" --to \"#00f0ff\"",
            "custom gradient",
        ),
        ("pyroclear --pick", "interactive picker"),
        (
            "pyroclear --color lava --no-save",
            "one-off, no config change",
        ),
        ("pyroclear --info", "show active palette card"),
        ("pyroclear --list-colors | less -R", "browse all palettes"),
    ];
    for (cmd, note) in ex {
        println!(
            "    {ESC}[38;2;80;185;255m${ESC}[0m \
             {ESC}[38;2;210;210;230m{cmd:<46}{ESC}[0m \
             {ESC}[38;2;82;82;105m# {note}{ESC}[0m"
        );
    }

    println!("\n  {ESC}[38;2;65;65;85mConfig: ~/.config/pyroclear/config.toml{ESC}[0m\n");
}

// ── Start / onboarding ────────────────────────────────────────────────

pub fn print_start() {
    fn header(title: &str) {
        println!("\n  {ESC}[1;38;2;255;150;50m◆  {title}{ESC}[0m");
        println!("  {ESC}[38;2;60;60;80m{}\n{ESC}[0m", "━".repeat(60));
    }

    print_banner();

    header("WHAT IS PYROCLEAR?");
    println!("  pyroclear is a high-fidelity, interactive terminal fire emulator");
    println!("  inspired by the classic Doom-fire algorithm.");
    println!("  It creates a live particle simulation in your shell, rendering");
    println!("  flames that rise, cooling down as they head upwards, and finally");
    println!("  clearing your terminal scrollback on exit.");

    header("HOW DOES THE PHYSICS ENGINE WORK?");
    println!("  1. Heat Grid: It maintains a 2D grid of thermal values (0 to 36).");
    println!("  2. Ignition Source: The bottom-most row is set to maximum heat (36).");
    println!("  3. Heat Propagation: Each tick, every cell reads the heat of the");
    println!("     cell below it, applying a small random decay and horizontal drift.");
    println!("  4. Color Palette Ramps: The thermal values index into a 37-color ramp.");
    println!("  5. Pure ANSI Escapes: The entire frame is flushed to stdout via raw");
    println!("     ANSI RGB commands. No heavy dependencies or TUI frameworks used.");

    header("HOW TO USE IT");
    println!(
        "  {ESC}[1;38;2;255;220;80m• Select Colors{ESC}[0m  Open the interactive color picker via:"
    );
    println!("                 {ESC}[38;2;80;185;255mpyroclear --pick{ESC}[0m (or {ESC}[38;2;80;185;255m-p{ESC}[0m)");
    println!();
    println!("  {ESC}[1;38;2;255;220;80m• Fine-Tune{ESC}[0m     Open the interactive animation settings panel to");
    println!("                 change FPS, wind direction, and flame decay height:");
    println!("                 {ESC}[38;2;80;185;255mpyroclear --settings{ESC}[0m (or {ESC}[38;2;80;185;255m-s{ESC}[0m)");
    println!();
    println!("  {ESC}[1;38;2;255;220;80m• Run Options{ESC}[0m   Run with custom arguments to bypass saved defaults:");
    println!("                 {ESC}[38;2;80;185;255mpyroclear --color toxic --no-save{ESC}[0m");
    println!("                 {ESC}[38;2;80;185;255mpyroclear --random{ESC}[0m");

    println!(
        "\n  {ESC}[38;2;90;90;110mFor full flag documentation, run: pyroclear --help{ESC}[0m\n"
    );
}

// ── Color list ────────────────────────────────────────────────────────

pub fn print_color_list() {
    print_banner();
    let (cols, _) = crate::engine::terminal_size();
    let n = NAMED_PALETTES.len();
    let two_col = cols >= 132;
    let id_w = 18usize;
    let sw_w = if two_col { 20usize } else { 28usize };

    let rule = "─".repeat(cols.saturating_sub(26));
    println!("  {ESC}[1;38;2;255;200;80m{n} palettes available{ESC}[0m  {ESC}[38;2;45;45;65m{rule}{ESC}[0m\n");

    for &(cat_name, start, end) in CATEGORIES {
        let count = end - start;
        let rl = cols.saturating_sub(cat_name.len() + 14);
        println!(
            "  {ESC}[38;2;255;160;35m▸ {cat_name}{ESC}[0m  \
             {ESC}[38;2;55;55;75m{count:>3} ╌{}{ESC}[0m",
            "╌".repeat(rl)
        );

        let palettes = &NAMED_PALETTES[start..end];

        if two_col {
            let half = count.div_ceil(2);
            for row in 0..half {
                let (id_l, _, _, from_l, to_l) = palettes[row];
                let sw_l = render_swatch(id_l, from_l, to_l, sw_w);
                print!(
                    "  {ESC}[1;38;2;255;225;100m{id_l:<id_w$}{ESC}[0m {sw_l} \
                     {ESC}[38;2;90;90;112m{from_l}{ESC}[38;2;50;50;70m→{ESC}[38;2;90;90;112m{to_l}{ESC}[0m"
                );
                let right = row + half;
                if right < count {
                    let (id_r, _, _, from_r, to_r) = palettes[right];
                    let sw_r = render_swatch(id_r, from_r, to_r, sw_w);
                    print!(
                        "    {ESC}[1;38;2;255;225;100m{id_r:<id_w$}{ESC}[0m {sw_r} \
                         {ESC}[38;2;90;90;112m{from_r}{ESC}[38;2;50;50;70m→{ESC}[38;2;90;90;112m{to_r}{ESC}[0m"
                    );
                }
                println!();
            }
        } else {
            for (id, _, desc, from_hex, to_hex) in palettes {
                let sw = render_swatch(id, from_hex, to_hex, sw_w);
                println!(
                    "  {ESC}[1;38;2;255;225;100m{id:<id_w$}{ESC}[0m {sw}  \
                     {ESC}[38;2;125;125;148m{desc}{ESC}[0m"
                );
            }
        }
        println!();
    }

    println!("  {ESC}[38;2;75;75;95m╌╌ Custom:      pyroclear --from \"#rrggbb\" --to \"#rrggbb\"{ESC}[0m");
    println!("  {ESC}[38;2;75;75;95m╌╌ Interactive: pyroclear --pick{ESC}[0m");
    println!("  {ESC}[38;2;75;75;95m╌╌ Random:      pyroclear --random{ESC}[0m\n");
}
