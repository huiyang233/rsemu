# rsemu

`rsemu` is an MCU emulator prototype focused on STM32 targets with pluggable peripherals.

## Run Model

Runtime is driven by a board config file (`board.toml`) instead of many CLI flags.

```bash
cargo run -p rsemu-cli --release -- --board board.toml --no-gui --max-steps 1000000
```

CLI options:

- `--board <path>` board config path (default `board.toml`)
- `--max-steps <n>` instruction-step limit
- `--no-gui` disable display window
- `--fast` run without realtime pacing
- `--dump-frames` dump ST7789 frames (if enabled)

## board.toml Example

```toml
target = "STM32F407"
svd = "examples/stm32f407.svd"
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

## Workspace Layout

- `crates/rsemu-core`: CPU, memory bus, machine runtime
- `crates/rsemu-svd`: SVD parsing
- `crates/rsemu-targets`: target definitions (`f103`, `f407`)
- `crates/rsemu-peripherals`: pluggable peripheral implementations
- `apps/rsemu-cli`: board loading + run loop
