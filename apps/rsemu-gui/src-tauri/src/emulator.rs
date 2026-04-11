use base64::Engine;
use rsemu_core::cpu::armv7em::CortexM4;
use rsemu_core::cpu::armv7m::CortexM3;
use rsemu_core::TargetSpec;
use std::sync::Arc;
use tauri::Emitter;
use std::time::{Duration, Instant};

struct WallClockSystickDriver {
    last: Instant,
    carry: f64,
}

impl WallClockSystickDriver {
    fn new() -> Self {
        Self {
            last: Instant::now(),
            carry: 0.0,
        }
    }

    fn tick<C: CpuCore>(
        &mut self,
        machine: &mut Machine<C>,
        core_clock_hz: u32,
        systick_reload_divider: u32,
    ) {
        let now = Instant::now();
        let elapsed = now.saturating_duration_since(self.last);
        self.last = now;

        // Avoid giant bursts when the host thread is paused.
        let elapsed = elapsed.min(Duration::from_millis(200));
        let ticks_per_sec = (core_clock_hz.max(1) as f64) / (systick_reload_divider.max(1) as f64);
        self.carry += elapsed.as_secs_f64() * ticks_per_sec;
        let whole = self.carry.floor() as u64;
        self.carry -= whole as f64;
        if whole > 0 {
            machine.advance_systick_ticks(whole);
        }
    }
}

struct WallClockTimerDriver {
    last: Instant,
    carry: f64,
}

impl WallClockTimerDriver {
    fn new() -> Self {
        Self {
            last: Instant::now(),
            carry: 0.0,
        }
    }

    fn tick<C: CpuCore>(&mut self, machine: &mut Machine<C>, core_clock_hz: u32) {
        let now = Instant::now();
        let elapsed = now.saturating_duration_since(self.last);
        self.last = now;

        // Avoid giant bursts when the host thread is paused.
        let elapsed = elapsed.min(Duration::from_millis(200));
        self.carry += elapsed.as_secs_f64() * (core_clock_hz.max(1) as f64);
        let whole = self.carry.floor() as u64;
        self.carry -= whole as f64;
        if whole > 0 {
            machine.advance_timers_cycles(whole);
        }
    }
}

struct WallClockStepPacer {
    start: Instant,
    steps: u64,
    steps_per_sec: f64,
}

impl WallClockStepPacer {
    fn new(core_clock_hz: u32, systick_reload_divider: u32) -> Self {
        let steps_per_sec = (core_clock_hz.max(1) as f64)
            / (systick_reload_divider.max(1) as f64);
        Self {
            start: Instant::now(),
            steps: 0,
            steps_per_sec,
        }
    }

    fn on_steps(&mut self, ran: u32) {
        self.steps = self.steps.saturating_add(ran as u64);
        let expected_secs = self.steps as f64 / self.steps_per_sec.max(1.0);
        let elapsed_secs = self.start.elapsed().as_secs_f64();
        if expected_secs > elapsed_secs {
            std::thread::sleep(Duration::from_secs_f64(expected_secs - elapsed_secs));
        }
    }
}

fn normalize_ssd1306_size(width: u16, height: u16) -> (u16, u16) {
    let w = match width {
        64 | 72 | 96 | 128 => width,
        _ => 128,
    };
    let h = match height {
        32 | 40 | 48 | 64 => height,
        _ => 64,
    };
    (w, h)
}

use rsemu_core::{
    BusContext, CpuCore, CpuType, FirmwareLoader, GpioPin, I2cBus, Machine,
    RccClockModel, SpiBus, StepBatchController, gpio_idr_addr,
};
use rsemu_peripherals::display::St7789;
use rsemu_peripherals::led::Led;
use rsemu_peripherals::ssd1306::Ssd1306I2c;
use rsemu_peripherals::{PeripheralConfig, PinMapping};
use serde::Serialize;
use std::sync::mpsc::{Receiver, TryRecvError};
use tauri::AppHandle;

use crate::state::{ControlMsg, SimConfig};

// ── Tauri event payloads ─────────────────────────────────────────────────────

#[derive(Clone, Serialize)]
pub struct SimStatusPayload {
    pub steps: u64,
    pub running: bool,
    pub error: Option<String>,
}

#[derive(Clone, Serialize)]
struct LedChangedPayload {
    id: String,
    on: bool,
}

#[derive(Clone, Serialize)]
struct DisplayFramePayload {
    width: u16,
    height: u16,
    /// RGBA pixels encoded as base64 (4 bytes per pixel, little-endian)
    data: String,
}

#[derive(Clone, Serialize)]
struct UartOutputPayload {
    peripheral: String,
    bytes: Vec<u8>,
}

#[derive(Default, Clone, Copy)]
struct EventProcessStats {
    frame_emitted: bool,
    display_activity: bool,
    serial_events: usize,
    mmio_events: usize,
    frame_encode_ns: u128,
}

// ── Public entry point ───────────────────────────────────────────────────────

pub fn run_emulator(target: TargetSpec, config: SimConfig, app: AppHandle, control_rx: Receiver<ControlMsg>) {
    eprintln!("[EMU] Starting emulator for board: {}", config.board);
    eprintln!("[EMU] Firmware: {}", config.firmware_path);
    eprintln!("[EMU] Peripherals: {} items", config.peripherals.len());
    for (i, p) in config.peripherals.iter().enumerate() {
        eprintln!("[EMU]   [{}] {:?}", i, p);
    }

    eprintln!("[EMU] Target loaded: {} ({} peripherals)", target.name, target.peripherals.len());

    let peripherals = target.peripherals.clone();
    let cpu_init_result = match target.cpu_type {
        CpuType::CortexM4 => CortexM4::new().map(|cpu| {
            run_machine(cpu, target, config, app.clone(), control_rx, &peripherals);
        }),
        CpuType::CortexM3 => CortexM3::new().map(|cpu| {
            run_machine(cpu, target, config, app.clone(), control_rx, &peripherals);
        }),
    };
    if let Err(e) = cpu_init_result {
        eprintln!("[EMU] ERROR initializing CPU: {e}");
        app.emit("sim-status", SimStatusPayload { steps: 0, running: false, error: Some(e) }).ok();
    }
}

// ── Generic machine runner ───────────────────────────────────────────────────

fn run_machine<C: CpuCore>(
    cpu: C,
    target: TargetSpec,
    config: SimConfig,
    app: AppHandle,
    control_rx: Receiver<ControlMsg>,
    target_peripherals: &[rsemu_core::PeripheralSpec],
) {
    // Extract values needed for pacer before moving target into Machine
    let core_clock_hz = target.core_clock_hz;
    let systick_reload_divider = target.systick_reload_divider;
    let hsi_hz = target.hsi_hz;
    let has_pllcfgr = target.has_pllcfgr;

    let mut machine = Machine::new(cpu, target.clone());

    // Load firmware
    eprintln!("[EMU] Loading firmware from: {}", config.firmware_path);
    let fw = match FirmwareLoader::load_file(&config.firmware_path, 0x0800_0000) {
        Ok(fw) => {
            eprintln!("[EMU] Firmware loaded: {} segments", fw.segments().len());
            for (i, seg) in fw.segments().iter().enumerate() {
                eprintln!("[EMU]   Segment {}: 0x{:08x} ({} bytes)", i, seg.load_address, seg.bytes.len());
            }
            fw
        }
        Err(e) => {
            eprintln!("[EMU] ERROR loading firmware: {}", e);
            app.emit("sim-status", SimStatusPayload { steps: 0, running: false, error: Some(e.clone()) }).ok();
            return;
        }
    };
    if let Err(e) = machine.load_firmware(&fw) {
        eprintln!("[EMU] ERROR loading firmware into memory: {}", e);
        app.emit("sim-status", SimStatusPayload { steps: 0, running: false, error: Some(e.clone()) }).ok();
        return;
    }
    eprintln!("[EMU] Firmware loaded into machine memory");

    if let Err(e) = machine.reset_cpu() {
        eprintln!("[EMU] ERROR resetting CPU: {}", e);
        app.emit("sim-status", SimStatusPayload { steps: 0, running: false, error: Some(e.clone()) }).ok();
        return;
    }
    eprintln!("[EMU] CPU reset complete. Initial PC: 0x{:08x}", machine.cpu().program_counter());

    // ── Build BusContext ───────────────────────────────────────────────────
    let mut bus_ctx = BusContext::new();
    let mut uart_peripherals: Vec<String> = Vec::new();

    eprintln!("[EMU] Initializing peripherals...");
    for (idx, pc) in config.peripherals.iter().enumerate() {
        match pc {
            PeripheralConfig::St7789Spi { width, height, spi_base, cs, dc, res } => {
                eprintln!("[EMU]   [{}] ST7789(SPI) {}x{} base 0x{:08x}", idx, width, height, spi_base);
                eprintln!("[EMU]        CS: P{}{}, DC: P{}{}", cs.port, cs.pin, dc.port, dc.pin);
                let device = St7789::new(
                    *width, *height, *spi_base,
                    cs.clone(), dc.clone(), res.clone(),
                    String::new(), false, true,
                );
                let cs_port = cs.port.chars().next().unwrap_or('A').to_ascii_uppercase();
                let dc_port = dc.port.chars().next().unwrap_or('A').to_ascii_uppercase();
                let spi_peripheral = target.peripherals.iter()
                    .find(|p| p.base_address == *spi_base)
                    .map(|p| p.name.to_ascii_uppercase())
                    .unwrap_or_else(|| format!("SPI{}", spi_base & 0xFFFF));
                eprintln!("[EMU]        → {} (SPI routing)", spi_peripheral);
                let spi_bus = SpiBus::new(Box::new(device));
                let cs_pin = Some(GpioPin::new(cs_port, cs.pin));
                let dc_pin = Some(GpioPin::new(dc_port, dc.pin));
                bus_ctx.register_spi(spi_peripheral, spi_bus, cs_pin, dc_pin);
            }
            PeripheralConfig::St7789Fsmc { width, height, fsmc_base } => {
                eprintln!("[EMU]   [{}] ST7789(FSMC) {}x{} base 0x{:08x}", idx, width, height, fsmc_base);
                let device = St7789::new(
                    *width, *height, *fsmc_base,
                    PinMapping { port: "A".into(), pin: 0 },
                    PinMapping { port: "A".into(), pin: 0 },
                    None,
                    String::new(), false, true,
                );
                let fsmc_bus = rsemu_core::FsmcBus::new(Box::new(device));
                bus_ctx.register_fsmc(*fsmc_base, 0x0100_0000, fsmc_bus);
            }
            PeripheralConfig::Led { id, pin, active_low } => {
                let led_id = id.clone().unwrap_or_else(|| format!("{}{}", pin.port.to_ascii_uppercase(), pin.pin));
                eprintln!("[EMU]   [{}] LED {} on P{}{} (active_low={})", idx, led_id, pin.port, pin.pin, active_low);
                let port = pin.port.chars().next().unwrap_or('A').to_ascii_uppercase();
                let app_clone = app.clone();
                let led = Led::new(led_id, pin.clone(), *active_low)
                    .with_callback(Box::new(move |led_id, on| {
                        app_clone.emit("led-changed", LedChangedPayload {
                            id: led_id.to_string(),
                            on,
                        }).ok();
                    }));
                bus_ctx.register_gpio(
                    GpioPin::new(port, pin.pin),
                    Box::new(led),
                );
            }
            PeripheralConfig::Uart { usart, .. } => {
                eprintln!("[EMU]   [{}] UART {}", idx, usart);
                uart_peripherals.push(usart.clone());
            }
            PeripheralConfig::Button { id, pin } => {
                eprintln!("[EMU]   [{}] Button {} on P{}{}", idx, id, pin.port, pin.pin);
            }
            PeripheralConfig::Ssd1306I2c {
                width,
                height,
                i2c,
                address,
            } => {
                let (w, h) = normalize_ssd1306_size(*width, *height);
                eprintln!(
                    "[EMU]   [{}] SSD1306(I2C) {}x{} {} addr=0x{:02x}",
                    idx, w, h, i2c, address
                );
                let i2c_peripheral = i2c.to_ascii_uppercase();
                let device = Ssd1306I2c::new(w, h, i2c.clone(), *address);
                let i2c_bus = I2cBus::new(Box::new(device));
                bus_ctx.register_i2c(i2c_peripheral, i2c_bus);
            }
            PeripheralConfig::Custom { type_name, params } => {
                eprintln!("[EMU]   [{}] Custom peripheral '{}' ({} params) — skipped, no handler",
                    idx, type_name, params.len());
            }
        }
    }

    let has_display = bus_ctx.has_bus_devices();
    eprintln!("[EMU] Peripherals initialized: has_display={}, {} UARTs",
        has_display, uart_peripherals.len());

    // Wire up IRQ callback: bus devices push to shared queue, main loop flushes
    {
        let irq_queue = std::sync::Arc::clone(machine.pending_irqs());
        bus_ctx.set_irq_callbacks(&move || {
            let q = irq_queue.clone();
            Box::new(move |irq: u8| {
                if let Ok(mut guard) = q.lock() {
                    guard.push(irq);
                }
            })
        });
    }

    // Realtime + UnlockedRender:
    // - CPU loop runs as fast as possible
    // - SysTick time is injected from host wall-clock
    // - UI rendering is wall-clock throttled
    machine.set_step_driven_systick(false);
    machine.set_step_driven_timers(false);
    machine.set_systick_reload_scaling(false);
    let mut wallclock_systick = WallClockSystickDriver::new();
    let mut wallclock_timers = WallClockTimerDriver::new();
    let mut step_pacer = Some(WallClockStepPacer::new(core_clock_hz, systick_reload_divider));
    let mut step_batch = StepBatchController::new(1_000);
    let mut can_disable_pacer_for_display = has_display;
    let mut clocks = RccClockModel::new(core_clock_hz, hsi_hz, has_pllcfgr);
    eprintln!("[EMU] Realtime+UnlockedRender: core={}Hz, systick_div={}", core_clock_hz, systick_reload_divider);
    eprintln!("[EMU] CPU realtime step pacing enabled");

    app.emit("sim-status", SimStatusPayload { steps: 0, running: true, error: None }).ok();
    eprintln!("[EMU] === Starting main loop ===");

    let mut steps = 0u64;
    let mut serial_cursor = 0usize;
    let mut mmio_cursor = 0usize;
    let mut last_log_steps = 0u64;
    let log_interval = 100_000u64;
    let step_trace_enabled = matches!(
        std::env::var("RSEMU_GUI_STEP_TRACE").ok().as_deref(),
        Some("1") | Some("true") | Some("TRUE") | Some("True")
    );

    // Throttling: avoid overwhelming frontend/event bridge.
    let stream_interval = if has_display {
        Duration::from_millis(8)
    } else {
        Duration::from_millis(2)
    };
    let mut next_stream_deadline = Instant::now();
    let steps_emit_interval = Duration::from_millis(100);
    let mut next_steps_emit_deadline = Instant::now();

    // UART batching: collect bytes and send in batches
    let mut uart_batch: Vec<(Arc<str>, u8)> = Vec::new();
    let uart_batch_size = 64;

    // Realtime + UnlockedRender: keep real-time pacing, but emit frames by wall-clock.
    let frame_interval = Duration::from_millis(16);
    let mut next_frame_deadline = Instant::now();

    let mut perf_window_start = Instant::now();
    let mut perf_steps = 0u64;
    let mut perf_step_calls = 0u64;
    let mut perf_frames = 0u64;
    let mut perf_mmio = 0u64;
    let mut perf_serial = 0u64;
    let mut perf_step_ns = 0u128;
    let mut perf_event_ns = 0u128;
    let mut perf_frame_encode_ns = 0u128;

    loop {
        // Drain all pending control messages
        loop {
            match control_rx.try_recv() {
                Ok(ControlMsg::Stop) | Err(TryRecvError::Disconnected) => {
                    flush_uart_batch(&app, &mut uart_batch);
                    app.emit("sim-status", SimStatusPayload { steps, running: false, error: None }).ok();
                    return;
                }
                Ok(ControlMsg::InjectGpio { port, pin, high }) => {
                    if let Some(ch) = port.chars().next() {
                        match gpio_idr_addr(ch, target_peripherals) {
                            Ok(addr) => {
                                let b0 = machine.read8(addr).unwrap_or(0);
                                let b1 = machine.read8(addr + 1).unwrap_or(0);
                                let b2 = machine.read8(addr + 2).unwrap_or(0);
                                let b3 = machine.read8(addr + 3).unwrap_or(0);
                                let mut idr = u32::from_le_bytes([b0, b1, b2, b3]);
                                if high { idr |= 1u32 << pin; } else { idr &= !(1u32 << pin); }
                                let bytes = idr.to_le_bytes();
                                let _ = machine.write8(addr, bytes[0]);
                                let _ = machine.write8(addr + 1, bytes[1]);
                                let _ = machine.write8(addr + 2, bytes[2]);
                                let _ = machine.write8(addr + 3, bytes[3]);
                            }
                            Err(e) => eprintln!("[EMU] InjectGpio: {e}"),
                        }
                    }
                }
                Ok(ControlMsg::SendUart { peripheral, bytes }) => {
                    for byte in bytes {
                        let _ = machine.usart_push_rx_byte(&peripheral, byte);
                    }
                }
                Err(TryRecvError::Empty) => break,
            }
        }

        let step_begin = Instant::now();
        // Step the CPU
        match step_batch.step_cpu(&mut machine) {
            Ok(ran) => {
                perf_step_ns = perf_step_ns.saturating_add(step_begin.elapsed().as_nanos());
                perf_steps = perf_steps.saturating_add(ran as u64);
                perf_step_calls = perf_step_calls.saturating_add(1);
                steps += ran as u64;
                if let Some(pacer) = step_pacer.as_mut() {
                    pacer.on_steps(ran);
                }
                wallclock_systick.tick(
                    &mut machine,
                    clocks.core_clock_hz,
                    systick_reload_divider,
                );
                wallclock_timers.tick(&mut machine, clocks.core_clock_hz);
                machine.flush_pending_irqs();

                if Instant::now() >= next_stream_deadline {
                    let event_begin = Instant::now();
                    let ev_stats = process_events(
                        &mut machine,
                        &mut serial_cursor,
                        &mut mmio_cursor,
                        &mut bus_ctx,
                        &uart_peripherals,
                        has_display,
                        &app,
                        &mut clocks,
                        &mut uart_batch,
                        uart_batch_size,
                        frame_interval,
                        &mut next_frame_deadline,
                    );
                    perf_event_ns = perf_event_ns.saturating_add(event_begin.elapsed().as_nanos());
                    perf_serial = perf_serial.saturating_add(ev_stats.serial_events as u64);
                    perf_mmio = perf_mmio.saturating_add(ev_stats.mmio_events as u64);
                    perf_frame_encode_ns =
                        perf_frame_encode_ns.saturating_add(ev_stats.frame_encode_ns);
                    if ev_stats.frame_emitted {
                        perf_frames = perf_frames.saturating_add(1);
                    }
                    if can_disable_pacer_for_display
                        && (ev_stats.display_activity || ev_stats.frame_emitted)
                        && step_pacer.is_some()
                    {
                        eprintln!(
                            "[EMU] Display activity detected (spi/frame); disabling CPU step pacing"
                        );
                        step_pacer = None;
                        can_disable_pacer_for_display = false;
                    }
                    next_stream_deadline = Instant::now() + stream_interval;
                }

                // Periodic logging
                if step_trace_enabled && steps - last_log_steps >= log_interval {
                    eprintln!("[EMU] Steps: {}, PC: 0x{:08x}, Serial: {} bytes, MMIO: {} events",
                        steps, machine.cpu().program_counter(),
                        machine.serial_output().len(), machine.mmio_writes().len());
                    last_log_steps = steps;
                }

                if perf_window_start.elapsed() >= Duration::from_secs(1) {
                    let elapsed_secs = perf_window_start.elapsed().as_secs_f64().max(1e-6);
                    let elapsed_ns = (elapsed_secs * 1_000_000_000.0).max(1.0);
                    let step_pct = (perf_step_ns as f64 / elapsed_ns) * 100.0;
                    let event_pct = (perf_event_ns as f64 / elapsed_ns) * 100.0;
                    let frame_encode_pct = (perf_frame_encode_ns as f64 / elapsed_ns) * 100.0;
                    eprintln!(
                        "[EMU][PERF] steps/s={:.2}M step={:.1}% events={:.1}% frame-encode={:.1}% frames/s={:.1} mmio/s={:.0} serial/s={:.0} calls/s={:.0}",
                        (perf_steps as f64 / elapsed_secs) / 1_000_000.0,
                        step_pct,
                        event_pct,
                        frame_encode_pct,
                        perf_frames as f64 / elapsed_secs,
                        perf_mmio as f64 / elapsed_secs,
                        perf_serial as f64 / elapsed_secs,
                        perf_step_calls as f64 / elapsed_secs
                    );
                    perf_window_start = Instant::now();
                    perf_steps = 0;
                    perf_step_calls = 0;
                    perf_frames = 0;
                    perf_mmio = 0;
                    perf_serial = 0;
                    perf_step_ns = 0;
                    perf_event_ns = 0;
                    perf_frame_encode_ns = 0;
                }

                if Instant::now() >= next_steps_emit_deadline {
                    app.emit("sim-steps", steps).ok();
                    next_steps_emit_deadline = Instant::now() + steps_emit_interval;
                }
            }
            Err(e) => {
                eprintln!("[EMU] ERROR in step_cpu: {}", e);
                app.emit("sim-status", SimStatusPayload { steps, running: false, error: Some(e.clone()) }).ok();
                return;
            }
        }
    }
}

fn flush_uart_batch(app: &AppHandle, batch: &mut Vec<(Arc<str>, u8)>) {
    if batch.is_empty() {
        return;
    }
    // Group by peripheral and emit one event per peripheral per flush.
    let mut by_peripheral: std::collections::HashMap<String, Vec<u8>> =
        std::collections::HashMap::new();
    for (peripheral, byte) in batch.drain(..) {
        by_peripheral.entry(peripheral.to_string()).or_default().push(byte);
    }
    for (peripheral, bytes) in by_peripheral {
        app.emit("uart-output", UartOutputPayload { peripheral, bytes }).ok();
    }
}

/// Dispatch events through BusContext.
fn process_events<C: CpuCore>(
    machine: &mut Machine<C>,
    serial_cursor: &mut usize,
    mmio_cursor: &mut usize,
    bus_ctx: &mut BusContext,
    uart_peripherals: &[String],
    has_display: bool,
    app: &AppHandle,
    clocks: &mut RccClockModel,
    uart_batch: &mut Vec<(Arc<str>, u8)>,
    uart_batch_size: usize,
    frame_interval: Duration,
    next_frame_deadline: &mut Instant,
) -> EventProcessStats {
    let mut stats = EventProcessStats::default();

    // ── Serial / UART output (batched) ───────────────────────────────────
    let serial = machine.serial_output();
    stats.serial_events = serial.len().saturating_sub(*serial_cursor);
    for event in &serial[*serial_cursor..] {
        let is_tracked = uart_peripherals.is_empty()
            || uart_peripherals.iter().any(|u| u.eq_ignore_ascii_case(&event.peripheral));
        if is_tracked {
            uart_batch.push((event.peripheral.clone(), event.byte));
        }
    }
    *serial_cursor = serial.len();

    // Flush UART batch only when it reaches the batch size threshold.
    // The previous `|| !uart_batch.is_empty()` condition was defeating batching
    // by flushing on every tick regardless of size.
    if uart_batch.len() >= uart_batch_size {
        flush_uart_batch(app, uart_batch);
    }

    // ── MMIO write events → BusContext routing ──────────────────────────
    let mmio = machine.mmio_writes();
    stats.mmio_events = mmio.len().saturating_sub(*mmio_cursor);
    for event in &mmio[*mmio_cursor..] {
        let bus_stats = bus_ctx.dispatch_mmio(event);
        if has_display && bus_stats.display_activity {
            stats.display_activity = true;
        }

        // RCC/STK clock model
        if event.peripheral.eq_ignore_ascii_case("RCC")
            || event.peripheral.eq_ignore_ascii_case("STK")
        {
            let _ = clocks.apply_mmio(event);
        }
    }
    *mmio_cursor = mmio.len();

    // ── Display frame (wall-clock throttled, render unlocked from step count) ──
    if Instant::now() >= *next_frame_deadline {
        *next_frame_deadline = Instant::now() + frame_interval;
        if has_display {
            for frame in bus_ctx.poll_frames() {
                stats.frame_emitted = true;
                let frame_encode_begin = Instant::now();
                let data = encode_rgba_frame_base64(&frame.pixels);
                stats.frame_encode_ns = stats
                    .frame_encode_ns
                    .saturating_add(frame_encode_begin.elapsed().as_nanos());
                app.emit("display-frame", DisplayFramePayload {
                    width: frame.width,
                    height: frame.height,
                    data,
                }).ok();
            }
        }
    }

    machine.clear_outputs();
    *serial_cursor = 0;
    *mmio_cursor = 0;

    stats
}

fn encode_rgba_frame_base64(frame: &[u32]) -> String {
    if cfg!(target_endian = "little") {
        let raw = unsafe {
            std::slice::from_raw_parts(
                frame.as_ptr() as *const u8,
                std::mem::size_of_val(frame),
            )
        };
        return base64::engine::general_purpose::STANDARD.encode(raw);
    }
    let raw: Vec<u8> = frame.iter().flat_map(|&px| px.to_le_bytes()).collect();
    base64::engine::general_purpose::STANDARD.encode(&raw)
}
