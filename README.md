# rsemu

`rsemu` is an embedded emulator prototype designed for long-term multi-architecture support.

## Current scope

- Architecture abstraction separated from target modeling.
- Initial target: `STM32F103` (`ARM Cortex-M3` / `ARMv7-M`).
- Optional SVD input to populate peripheral and register layout.

## Why this layout

- `rsemu-core/src/cpu`: architecture and CPU core implementations.
- `rsemu-core/src/machine.rs`: machine assembly and runtime orchestration.
- `rsemu-core/src/memory.rs`: firmware images and memory blocks.
- `rsemu-core/src/target.rs`: target and peripheral specifications.
- `rsemu-svd/src`: SVD model, parser, and XML helpers.
- `rsemu-targets/src/stm32`: STM32 family targets.
- `rsemu-cli/src`: CLI parsing and app entry logic.

## Limitations in this prototype

- CPU execution is still minimal. `CortexM3` currently supports reset vector loading and a `NOP` smoke step only.
- The SVD parser currently supports a minimal subset of CMSIS-SVD XML.
- Peripheral behavior is register-backed only; there are no timers, interrupts, DMA, or clock trees yet.
- Firmware loading supports raw binary and a minimal ELF32 little-endian loader.

## Example

```bash
cargo run -p rsemu-cli -- --svd path/to/STM32F103.svd
```

```bash
cargo run -p rsemu-cli -- --svd path/to/STM32F103.svd --firmware path/to/firmware.bin
```

```bash
cargo run -p rsemu-cli -- --firmware path/to/firmware.elf
```
