# KeSp Controller

Cross-platform configurator for the KeSp split ergonomic keyboard.

Built with [Rust](https://www.rust-lang.org/) and [Slint](https://slint.dev/) UI framework.

![License: GPL-3.0](https://img.shields.io/badge/License-GPLv3-blue.svg)

## Features

- **Keymap editor** with visual keyboard layout (loaded from firmware)
- **Key selector** with categorized grid, Mod-Tap/Layer-Tap builders, hex input
- **Heatmap overlay** showing key press frequency (blue to red gradient)
- **Layer management** with switch, rename, and active indicator
- **Tap Dance** editing (4 actions per slot)
- **Combos** creation with visual key picker
- **Key Overrides** with modifier checkboxes (Ctrl/Shift/Alt)
- **Leader Keys** with sequence builder
- **Macros** with visual step builder (key presses + delays)
- **Statistics** (hand balance, finger load, row usage, top keys, bigrams)
- **OTA firmware update** via USB (no programming cable needed)
- **ESP32 flasher** (esptool-like, via programming port)
- **Settings** with keyboard layout selector (QWERTY, AZERTY, DVORAK, etc.)
- **Dracula theme** throughout

## Download

Pre-built binaries for Linux, Windows, and macOS are available on the [Releases](https://github.com/mornepousse/KeSp_controller/releases) page.

## Build from source

### Requirements

- Rust toolchain (1.75+)
- Linux: `libudev-dev libfontconfig1-dev`

### Build

```bash
cargo build --release
```

Binary will be at `target/release/KeSp_controller`.

## Usage

1. Plug in your KeSp keyboard via USB
2. Launch KeSp Controller
3. The app auto-connects to the keyboard
4. Use the tabs to configure: Keymap, Advanced, Macros, Stats, Settings, Flash

## Keyboard compatibility

Designed for the KeSp/KaSe split keyboard with:
- USB CDC serial (VID: 0xCAFE, PID: 0x4001)
- Binary protocol v2
- ESP32-S3 MCU

## License

GPL-3.0 - See [LICENSE](LICENSE)

Made with [Slint](https://slint.dev/)
