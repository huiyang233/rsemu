# CLAUDE.md — rsemu Project Guide

## Project Overview

rsemu is an STM32 MCU emulator with pluggable peripherals, using unicorn-engine for CPU emulation. It has a CLI runner and a Tauri-based GUI (Rust backend + React/TypeScript frontend).

## Build & Run

```bash
# CLI
cargo run -p rsemu-cli --release -- --board board.toml --no-gui --max-steps 1000000

# GUI (dev)
cd apps/rsemu-gui && npm install && npm run tauri dev

# GUI (backend release mode for perf)
npm run tauri dev -- --release
```

## Workspace Layout

```
crates/rsemu-core       — CPU, memory bus, machine runtime, bus-device traits
crates/rsemu-svd        — SVD register description parser
crates/rsemu-targets    — Target MCU definitions (STM32F103, STM32F407)
crates/rsemu-peripherals — Pluggable peripheral implementations (UART, ST7789, SSD1306, LED)
apps/rsemu-cli          — Board loading + CLI run loop
apps/rsemu-gui          — Tauri 2.0 GUI app (Rust backend + React/TS frontend)
```

## Key Traits (rsemu-core)

- `CpuCore` — CPU step/reset abstraction over unicorn-engine
- `SpiSlave` / `I2cSlave` / `UartDevice` / `GpioListener` — bus-device protocols
- `SystemBus` — MMIO routing, memory read/write with peripheral event dispatch

## Code Conventions

- **Error handling**: `Result<T, String>` everywhere; use `.map_err(|e| format!("context: {e}"))` to add context
- **New peripherals**: Implement the relevant bus trait (`SpiSlave`, `I2cSlave`, etc.), add variant to `PeripheralSpec` enum with `#[serde(tag = "type")]`
- **Config format**: Peripherals configured in `board.toml` via `[[peripherals]]` TOML arrays
- **GUI events**: Backend emits Tauri events (`sim-status`, `uart-output`, `led-changed`, `display-frame`); frontend subscribes via Zustand store

## Build Profiles

- Release: `opt-level = 3`, `lto = "fat"`, `codegen-units = 1`
- Dev profile already optimizes core/peripherals/targets crates at `opt-level = 3` for usable perf during development

## Testing

- Standard `#[test]` attribute tests
- Integration tests in `apps/rsemu-gui/src-tauri/tests/`
- Perf tests use `#[ignore]` for manual invocation
- Tests often require real firmware binaries in `firmware/`

## Frontend Stack (rsemu-gui)

- **State**: Zustand (`apps/rsemu-gui/src/store/appStore.ts`)
- **UI**: React + TailwindCSS
- **Terminal**: xterm.js for UART console
- **Display**: Canvas-based widget for ST7789/SSD1306 frames (base64 ARGB from backend)

## Things to Know

- The `stm32f1xx-hal`, `stm32f4xx-hal`, and `stm32-emulator` directories are excluded from the workspace (external references)
- Bus-device architecture routes MMIO writes to registered peripheral trait objects
- GUI backend prints perf stats as `[EMU][PERF] ...` every second
- Verbose step trace: `RSEMU_GUI_STEP_TRACE=1 npm run tauri dev`
