# rsemu

`rsemu` is an MCU emulator prototype focused on STM32 targets with pluggable peripherals.

## Run Model

Runtime is driven by a board config file (`board.toml`) instead of many CLI flags.

```bash
cargo run -p rsemu-cli --release -- --board examples/board.toml --no-gui --max-steps 1000000
```

CLI options:

- `--board <path>` board config path (default `board.toml`)
- `--max-steps <n>` instruction-step limit
- `--no-gui` disable display window
- `--fast` run without realtime pacing
- `--dump-frames` dump ST7789 frames (if enabled)

## board.toml Example

See `examples/board.toml` for a complete example.

```toml
target = "STM32F407"
firmware = "firmware/rtthread.bin"
load_addr = 0x08000000
cycle_scale = 1

[[peripherals]]
type = "uart_terminal"
id = "console0"
enabled = true
usart = "USART1"
tx = { port = "A", pin = 9 }
rx = { port = "A", pin = 10 }

[[peripherals]]
type = "st7789"
id = "lcd0"
enabled = true
width = 240
height = 320
spi_base = 0x40013000
cs = { port = "A", pin = 4 }
dc = { port = "A", pin = 3 }
dump_frames = false
output_dir = "/tmp/rsemu-st7789"

[[peripherals]]
type = "led"
id = "red"
enabled = true
pin = { port = "F", pin = 12 }
active_low = true
```

## Peripheral Notes

- `uart_terminal`: entering this config enables an interactive serial console in the current terminal.
- `st7789`: receives SPI/GPIO traffic and can show frames in a GUI window.
- `led`: watches a configured GPIO pin and prints state changes (`on` / `off`).

## GUI (rsemu-gui)

GUI app path: `apps/rsemu-gui`.

Quick start:

```bash
cd apps/rsemu-gui
npm install
npm run tauri dev
```

Useful run modes:

- Dev (fast iteration): `npm run tauri dev`
- Backend release in dev session (higher throughput): `npm run tauri dev -- --release`
- Build app bundle: `npm run tauri build`

Notes:

- GUI runtime uses realtime + unlocked render behavior for display workloads.
- Backend prints perf stats as `[EMU][PERF] ...` every second.
- Optional verbose step trace (for diagnosis): `RSEMU_GUI_STEP_TRACE=1 npm run tauri dev`
- Detailed guide: `apps/rsemu-gui/doc/realtime-unlockedrender-guide.md`

## Workspace Layout

- `crates/rsemu-core`: CPU, memory bus, machine runtime
- `crates/rsemu-svd`: SVD parsing
- `crates/rsemu-targets`: target definitions (`f103`, `f407`)
- `crates/rsemu-peripherals`: pluggable peripheral implementations
- `apps/rsemu-cli`: board loading + run loop
- `apps/rsemu-gui`: Tauri 2.0 GUI app (Rust backend + React/TS frontend)
- `examples/`: example board configs and sample files
