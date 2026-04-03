# Realtime + UnlockedRender Guide

Date: 2026-04-03

## User Requirement

1. Keep delay semantics effective (`rt_thread_mdelay(500)` should behave close to 500ms wall time).
2. Keep display refresh fast (avoid GUI stepping speed collapsing to ~26k steps/s).

## Reproduction Artifacts

1. Screen firmware:
`/Users/yanghui/Documents/trae_projects/rsemu/stm32f4xx-hal/target/thumbv7em-none-eabihf/release/examples/screen-color`
2. RT-Thread firmware:
`/Users/yanghui/Documents/trae_projects/rsemu/firmware/rtthread.bin`
3. User-provided source for RT-Thread app:
`/Users/yanghui/Library/Containers/com.tencent.xinWeChat/Data/Documents/xwechat_files/wxid_aubgqm8msoy021_fa8b/msg/file/2026-04/01_kernel_1/applications/main.c`

## Observations

1. Existing GUI realtime pacing path produced very low speed (~26k steps/s).
2. CLI `--fast` path reaches multi-million steps/s (user logs show ~5M to 10M+ quickly).

## Changes Implemented (2026-04-03)

1. Reworked GUI realtime loop to "unlocked CPU + wall-clock throttled streaming/render".
2. Added wall-clock driven SysTick injection path.
3. Added RCC clock model that can track clock changes from MMIO events.
4. Added F407-related clock inference fallback (from SysTick LOAD) in dedicated clock model files:
`apps/rsemu-gui/src-tauri/src/clock_model.rs` and `apps/rsemu-cli/src/clock_model.rs`.
5. Added `Machine::set_systick_reload_scaling(false)` and O(1) SysTick `tick_many` fast-forward in core.
6. Kept device-specific logic out of common entry files (`emulator.rs`/`app.rs`) and moved it to dedicated modules.
7. Fixed test-side RT-Thread log parsing to strip ANSI escape sequences before extracting `count`.

## Verification Results

1. `f407_screen_color_realtime_unlocked_render_emits_frames` passed.
2. Observed performance for `screen-color`: about 2.0M steps/s, continuous frames emitted.
3. `f407_rtthread_led_delay_stays_effective_in_realtime` still fails.
4. For `rtthread.bin`, actual observed `led on` intervals are about 2ms (expected around 500ms).
5. RT-Thread serial output is present and valid; failure is timing semantics, not "no output".

## Current Problem

1. Delay semantics are still incorrect in realtime-unlocked mode for RT-Thread workloads.
2. The system currently over-advances perceived OS time under this mode (symptom: `mdelay(500)` behaves near 2ms).
3. This is now the primary blocker after render throughput was fixed.

## Next Focus

1. Pin down the exact RT-Thread delay time source path used in this firmware image (SysTick VAL/readback path vs scheduler tick accounting).
2. Align "wall-clock tick injection" with RT-Thread's effective time base so both interrupt rate and readable counter semantics are consistent.
3. Add a dedicated regression check for this path to prevent future drift while keeping unlocked render throughput.

## Follow-up Fix (2026-04-03)

1. Kept unlocked render path for display-heavy workloads (screen firmware path unchanged).
2. Added a CPU realtime step pacer in GUI emulator loop:
   target steps/s = `core_clock_hz / systick_reload_divider`.
3. Keep pacing enabled by default, and auto-disable it once real display frame activity is detected (so display-heavy firmware keeps unlocked throughput).
4. Retained wall-clock SysTick/timer injection for consistency with existing realtime-unlocked architecture.

## Follow-up Verification

1. `f407_screen_color_realtime_unlocked_render_emits_frames`:
   ~1.93M steps/s, frames continuously emitted (throughput preserved).
2. `f407_rtthread_led_delay_stays_effective_in_realtime`:
   `led on` intervals ~`[449, 450, 514, 449, 447, 449] ms` (delay semantics restored near 500ms).

## GUI Throughput Tuning (2026-04-03, Round 2)

1. Added runtime perf breakdown logs in GUI emulator loop (`[EMU][PERF]`):
   includes steps/s, step/event/frame-encode time share, frames/s, mmio/s, serial/s.
2. Reduced frontend bridge pressure:
   `sim-steps` event is now throttled to 100ms instead of high-frequency stream-tick emission.
3. Relaxed stream event scan cadence when display exists:
   stream interval changed from 2ms to 6ms for display workloads.
4. Optimized display encode path:
   on little-endian hosts, base64 encodes frame bytes in-place (avoids per-frame raw-byte vector build).
5. Optimized ST7789 preview snapshot behavior:
   avoid repeated full-frame clone overwrite while a pending preview frame is already queued.

## GUI Throughput Tuning (2026-04-03, Round 3)

1. Aligned GUI CPU batch stepping fallback order with CLI behavior:
   default preferred batch is now `1000`, avoiding per-loop speculative `10k/5k` retries.
2. Simplified retry path in `step_cpu_resilient`:
   fallback sequence now focuses on smaller safe batches (`1000 -> 100 -> 10 -> 1`) instead of large retries.
3. Disabled high-frequency `[EMU] Steps: ...` logging by default:
   can be re-enabled only when needed via `RSEMU_GUI_STEP_TRACE=1`.
4. Reduced MMIO event overhead in LED tracking:
   precomputed GPIO peripheral names in `LedTracker` and removed per-event string allocation/formatting.
5. Reduced unnecessary clock-model work:
   `RccClockModel::apply_mmio` is now only called for `RCC`/`STK` MMIO events.
6. Relaxed display-mode event scan cadence from `6ms` to `8ms` to lower frontend bridge/event-loop pressure.

## Round 3 Verification

1. `f407_screen_color_realtime_unlocked_render_emits_frames`:
   `[screen] steps=36512000, elapsed=8.000131167s, steps/s=4563925.17, frames=507`
2. `f407_rtthread_led_delay_stays_effective_in_realtime`:
   `stable log intervals ms: [449, 446, 516, 451, 450, 450]`

## GUI Throughput Tuning (2026-04-03, Round 4)

1. Added adaptive CPU step batch controller in GUI loop:
   starts from `1000`, auto-upgrades to larger batches after stable full-batch runs,
   and auto-downgrades on retryable MAP/EXCEPTION errors.
2. This keeps retry overhead low on unstable ranges while still allowing larger batch throughput when firmware allows it.

## Round 4 Verification

1. `f407_screen_color_realtime_unlocked_render_emits_frames`:
   `[screen] steps=37038000, elapsed=8.000143333s, steps/s=4629667.05, frames=514`
2. `f407_rtthread_led_delay_stays_effective_in_realtime`:
   `stable log intervals ms: [453, 450, 510, 454, 448, 447]`
