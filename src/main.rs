// pyroclear — terminal goes up in flames (or another effect), then clears.
//
// Doom-fire algorithm: a heat grid (0..=36) where the bottom row is
// the fire source. Each frame, every cell pulls its new value from
// the cell below (with a bit of horizontal drift), minus random
// decay, so heat rises and cools. Heat → color via a palette.
// Whole frame is redrawn each tick via ANSI cursor-home, so no
// flicker and no need for a TUI crate.
//
// Terminal size is read via ioctl(TIOCGWINSZ) on unix / the Win32
// console API on Windows every frame (cheap, no subprocess spawn), so
// the grid follows you live if you resize mid-burn. Randomness is a
// tiny xorshift64* PRNG seeded off the clock — no `rand` crate needed.
//
// MODULE LAYOUT
//   palettes  — palette data, color math, swatch renderers
//   config    — AnimSettings, PaletteChoice, config I/O, CLI parsing
//   display   — banner, help, info, start guide, color list
//   engine    — PRNG, terminal size, fire simulation loop, wave / vortex /
//               explosion effects
//   tui       — raw terminal, key input, picker & settings TUIs
//   win       — hand-rolled Win32 console bindings (Windows only)

use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

pub const ESC: &str = "\x1b";

pub mod config;
pub mod display;
pub mod engine;
pub mod palettes;
pub mod tui;
#[cfg(windows)]
pub mod win;

use config::{build_palette, resolve_choice};
use engine::burn;

// ── SIGINT handler ────────────────────────────────────────────────────

static SIGINT_FIRED: AtomicBool = AtomicBool::new(false);

#[cfg(unix)]
extern "C" fn sigint_handler(_: libc::c_int) {
    SIGINT_FIRED.store(true, Ordering::Relaxed);
}

#[cfg(windows)]
unsafe extern "system" fn ctrl_handler(_: u32) -> i32 {
    SIGINT_FIRED.store(true, Ordering::Relaxed);
    1
}

fn install_sigint() -> Arc<AtomicBool> {
    let flag = Arc::new(AtomicBool::new(false));
    let flag2 = Arc::clone(&flag);

    #[cfg(unix)]
    unsafe {
        libc::signal(
            libc::SIGINT,
            sigint_handler as *const () as libc::sighandler_t,
        );
    }
    #[cfg(windows)]
    unsafe {
        crate::win::SetConsoleCtrlHandler(ctrl_handler, 1);
    }

    std::thread::spawn(move || loop {
        if SIGINT_FIRED.load(Ordering::Relaxed) {
            flag2.store(true, Ordering::Relaxed);
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    });

    flag
}

// ── Entry point ───────────────────────────────────────────────────────

fn main() {
    let (choice, settings) = resolve_choice();
    let palette = build_palette(&choice);

    let interrupted = install_sigint();

    burn(&palette, &settings, interrupted);

    // Final clear — always runs, even after SIGINT (cursor was restored in burn)
    // Full clear: reset attrs, home, erase screen + scrollback
    let stdout = io::stdout();
    let mut out = stdout.lock();
    let _ = write!(out, "{ESC}[0m{ESC}[H{ESC}[2J{ESC}[3J");
    let _ = out.flush();
}
