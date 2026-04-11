# rsemu

`rsemu` is an STM32 MCU emulator with pluggable peripherals, powered by [unicorn-engine](https://github.com/unicorn-engine/unicorn) for CPU emulation. It provides both a CLI runner and a Tauri-based GUI.

[中文文档](README_zh.md)

## Supported Targets

Target configs are JSON-driven — add a new chip by dropping a JSON + SVD file, no Rust code changes needed.

| Target | Core | HSI | Config |
|--------|------|-----|--------|
| STM32F103 | Cortex-M3 | 8 MHz | `configs/stm32f103.json` |
| STM32F401 | Cortex-M4 | 16 MHz | `configs/stm32f401.json` |
| STM32F407 | Cortex-M4 | 16 MHz | `configs/stm32f407.json` |
| STM32F411 | Cortex-M4 | 16 MHz | `configs/stm32f411.json` |
| STM32F427 | Cortex-M4 | 16 MHz | `configs/stm32f427.json` |
| STM32F429 | Cortex-M4 | 16 MHz | `configs/stm32f429.json` |

## Supported Peripherals

| Type | Bus | Description |
|------|-----|-------------|
| ST7789 (SPI) | SPI + GPIO | LCD display, 240x320 etc. |
| ST7789 (FSMC) | FSMC | LCD display via parallel bus |
| SSD1306 (I2C) | I2C | OLED monochrome display |
| LED | GPIO | Pin state monitor with callback |
| UART | USART | Serial console (xterm.js in GUI) |
| Button | GPIO | GPIO input injection |
| Custom | — | Escape hatch via `HashMap<String, String>` params |

## CLI

```bash
cargo run -p rsemu-cli --release -- --board examples/board.toml --no-gui --max-steps 1000000
```

Options:
- `--board <path>` — board config path (default `board.toml`)
- `--max-steps <n>` — instruction step limit
- `--no-gui` — disable display window
- `--fast` — run without realtime pacing
- `--dump-frames` — dump ST7789 frames to disk

## GUI

```bash
cd apps/rsemu-gui
npm install
npm run tauri dev                # dev mode
npm run tauri dev -- --release   # backend release for perf
npm run tauri build              # production bundle
```

Environment variables:
- `RSEMU_GUI_STEP_TRACE=1` — verbose step trace for diagnosis

## Board Config

See [`examples/board.toml`](examples/board.toml) for a complete example.

```toml
target = "STM32F407"
firmware = "firmware/rtthread.bin"
load_addr = 0x08000000

[[peripherals]]
type = "st7789_spi"
width = 240
height = 320
spi_base = 0x40013000
cs = { port = "A", pin = 4 }
dc = { port = "A", pin = 3 }

[[peripherals]]
type = "led"
pin = { port = "F", pin = 12 }
active_low = true
```

## Workspace Layout

```
crates/rsemu-core          — CPU, memory bus, machine runtime, bus-device traits
crates/rsemu-svd           — CMSIS SVD register description parser
crates/rsemu-targets       — Target registry (JSON config + SVD loading)
crates/rsemu-peripherals   — Pluggable peripheral implementations
apps/rsemu-cli             — Board loading + CLI run loop
apps/rsemu-gui             — Tauri 2.0 GUI (Rust backend + React/TS frontend)
configs/                   — Target JSON configs (one file per chip)
svds/                      — CMSIS SVD files
examples/                  — Example board configs
```

## License

MIT
