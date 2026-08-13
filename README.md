# pyroclear

A terminal `clear` replacement that burns your screen down before wiping it. 

Written in modern Rust. Zero runtime dependencies beyond standard `libc` (Unix) or native Win32 API calls (Windows). Highly optimized, flicker-free, and customizable.

---

## Features

- **Platform Native**: Native Unix support (via direct `ioctl` syscalls and `termios` configuration) and native Windows support (via hand-rolled Win32 console API bindings for raw mode, virtual terminal processing, and console control handlers). Zero third-party runtime dependencies.
- **Transparent Background**: Empty cells inherit your terminal's default theme/opacity instead of drawing solid black rectangles.
- **300+ Built-in Palettes**: Categorized beautifully in `--list-colors` with aligned swatches.
- **Interactive TUIs**:
  - **Color Picker (`--pick`)**: Browse, search, filter, and preview palettes in real-time.
  - **Settings Manager (`--settings`)**: Adjust FPS, wind/drift, and flame height in raw mode.
  - **Custom Palette Manager (`--custom`)**: Build, name, delete, and save your own hex gradients.
- **Persistent Configuration**: Settings and palettes are automatically saved to `~/.config/pyroclear/config.toml` (`%USERPROFILE%\.config\pyroclear\config.toml` on Windows).
- **Signal-safe**: Interrupted runs (Ctrl-C) restore the terminal state and cursor cleanly (via custom Unix SIGINT handlers / Windows console control handlers).
- **Full terminal clear**: Erases both the visible screen **and** the scrollback buffer (via `\x1b[3J`) so nothing remains after the flames die out.

---

## Installation

Install via Cargo:

```bash
cargo install pyroclear
```

Or build from source:
- Install cargo
  ```bash
  # Arch based
  sudo pacman -S cargo
  ```
  ```bash
  # Fedora based
  sudo dnf install cargo
  ```
  ```bash
  # Debian based
  sudo apt install cargo
  ```
- Then clone, build, and install
  ```bash
  # Clone the repository
  git clone https://github.com/shreyanth-sureshkrishnaa/pyroclear.git
  cd pyroclear
  
  # Build and install (installs to your Cargo bin folder, e.g. ~/.cargo/bin)
  cargo build --release
  cargo install --path .
  ```

Install via NixOS Flakes:

```nix
{
  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixpkgs-unstable";
    pyroclear.url = "github:shreyanth-sureshkrishnaa/pyroclear";
  };

  outputs = {pyroclear, nixpkgs, ...}: {
    nixosConfigurations = {
      example = nixpkgs.lib.nixosSystem rec {
        # We support: aarch64-linux, x86_64-linux
        system = "x86_64-linux";

        modules = [
          {
            environment.systemPackages = [pyroclear.packages.${system}.default];
          }
        ];
      };
    };
  };
}
```

### Wire it up as `clear`

**Bash / Zsh (Linux, macOS, Git Bash on Windows)**
```bash
# Append to ~/.bashrc, ~/.zshrc, or ~/.bash_profile
alias clear="pyroclear"
```

**Fish (Linux / macOS)**
```fish
# Append to ~/.config/fish/config.fish
alias -s clear="pyroclear"
```

**PowerShell (Windows)**
```powershell
# Append to your PowerShell profile ($PROFILE)
Set-Alias -Name clear -Value pyroclear -Force
```

---

## Usage

```
pyroclear [OPTIONS]
```

### Command Modes

| Option | Description | Example |
| :--- | :--- | :--- |
| **`--start`** | Open the onboarding presentation & guide | `pyroclear --start` |
| **`--settings`, `-s`** | Adjust FPS, wind direction, animation durations, and flame height decay | `pyroclear --settings` |
| **`--pick`, `-p`** | Interactive color palette picker with live swatches | `pyroclear --pick` |
| **`--custom`** | TUI to save, name, manage and run custom gradients | `pyroclear --custom` |
| **`--color <name>`** | Burn with a specific named palette (saves as default) | `pyroclear --color toxic` |
| **`--from <hex> --to <hex>`**| Burn with a one-off custom gradient | `pyroclear --from "#002080" --to "#00f0ff"` |
| **`--list-colors`** | View all 300+ palettes grouped by color family | `pyroclear --list-colors` |
| **`--info`, `-i`** | Display active palette card and configured options | `pyroclear --info` |
| **`--random`, `-r`** | Run with a random palette every time | `pyroclear --random` |
| **`--no-save`** | Run choice without saving it to configuration | `pyroclear --color ocean --no-save` |
| **`--reset`** | Reset configuration to default fire palette | `pyroclear --reset` |
| **`-h, --help`** | Show quick help screen | `pyroclear --help` |

---

## Configuration

Your preferences are saved in:
- **Unix**: `~/.config/pyroclear/config.toml` (and `custom_palettes.toml` for custom palettes)
- **Windows**: `%USERPROFILE%\.config\pyroclear\config.toml` (and `custom_palettes.toml` for custom palettes)

If `XDG_CONFIG_HOME` is set, `$XDG_CONFIG_HOME/pyroclear/` is used instead on every
platform. Otherwise the home directory is resolved from `%USERPROFILE%` on Windows
(falling back to `%HOMEDRIVE%%HOMEPATH%`, then `$HOME`) and from `$HOME` elsewhere.

Run `pyroclear --help` to print the exact config path being used on your machine.

You can change them by running:
```bash
pyroclear -s
```
or by editing the config file (See the [formatting documentation](formatting.md))

---

## Performance

The physics engine runs at standard ~60 FPS (customizable) with multiple propagation steps per frame. The entire rendering buffer is flushed to stdout in a single write operation, ensuring sub-millisecond execution times even on massive high-refresh-rate displays.

---

## License

This project is licensed under the MIT License.
