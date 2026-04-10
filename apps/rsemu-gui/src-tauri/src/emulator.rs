use base64::Engine;
use rsemu_core::cpu::armv7em::CortexM4;
use rsemu_core::cpu::armv7m::CortexM3;
use tauri::Emitter;
use std::time::{Duration, Instant};

// Bundled SVD files — embedded at compile time so the user never needs to supply them.
const SVD_F103: &str = include_str!("../svd/stm32f103.svd");
const SVD_F407: &str = include_str!("../svd/stm32f407.svd");

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

struct StepBatchController {
    current: usize,
    grow_success: u8,
}

impl StepBatchController {
    fn new(initial: usize) -> Self {
        Self {
            current: normalize_batch(initial),
            grow_success: 0,
        }
    }

    fn step_cpu<C: CpuCore>(&mut self, machine: &mut Machine<C>) -> Result<u32, String> {
        let mut batch = self.current;
        loop {
            match machine.step_cpu(batch) {
                Ok(ran) => {
                    self.current = batch;
                    if (ran as usize) >= batch {
                        self.grow_success = self.grow_success.saturating_add(1);
                        if self.grow_success >= 8 {
                            self.current = next_larger_batch(self.current);
                            self.grow_success = 0;
                        }
                    } else {
                        self.grow_success = 0;
                    }
                    return Ok(ran);
                }
                Err(err) => {
                    let retryable = err.contains(": MAP")
                        || err.contains(" MAP")
                        || err.contains("EXCEPTION");
                    if !retryable {
                        return Err(err);
                    }
                    self.grow_success = 0;
                    let lower = next_smaller_batch(batch);
                    if lower == batch {
                        return Err(err);
                    }
                    batch = lower;
                }
            }
        }
    }
}

fn normalize_batch(v: usize) -> usize {
    if v >= 10_000 {
        10_000
    } else if v >= 5_000 {
        5_000
    } else if v >= 1_000 {
        1_000
    } else if v >= 100 {
        100
    } else if v >= 10 {
        10
    } else {
        1
    }
}

fn next_smaller_batch(v: usize) -> usize {
    if v > 5_000 {
        5_000
    } else if v > 1_000 {
        1_000
    } else if v > 100 {
        100
    } else if v > 10 {
        10
    } else {
        1
    }
}

fn next_larger_batch(v: usize) -> usize {
    if v < 10 {
        10
    } else if v < 100 {
        100
    } else if v < 1_000 {
        1_000
    } else if v < 5_000 {
        5_000
    } else {
        10_000
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
    CpuCore, FirmwareLoader, GpioListener, GpioNotifier, GpioPin, I2cSlave, Machine,
    MmioWriteEvent, SpiSlave, TargetSpec,
};
use rsemu_peripherals::display::St7789;
use rsemu_peripherals::led::Led;
use rsemu_peripherals::ssd1306::Ssd1306I2c;
use rsemu_targets::stm32::{f103, f407};
use serde::Serialize;
use std::sync::mpsc::{Receiver, TryRecvError};
use tauri::AppHandle;

use crate::clock_model::RccClockModel;
use crate::state::{ControlMsg, GuiPeripheralConfig, SimConfig};

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
    /// ARGB pixels encoded as base64 (4 bytes per pixel, little-endian)
    data: String,
}

#[derive(Clone, Serialize)]
struct UartOutputPayload {
    peripheral: String,
    byte: u8,
}

#[derive(Default, Clone, Copy)]
struct EventProcessStats {
    frame_emitted: bool,
    display_activity: bool,
    serial_events: usize,
    mmio_events: usize,
    frame_encode_ns: u128,
}

// ── Bus-device context (replaces Vec<Box<dyn Peripheral>> + LedTracker) ─────

/// Tracks which SPI peripheral an ST7789 display is connected to.
struct St7789BusHandle {
    spi_peripheral: String, // e.g. "SPI1"
    cs_port: char,
    cs_pin: u8,
    dc_port: char,
    dc_pin: u8,
    device: St7789,
}

/// Tracks which I2C peripheral an SSD1306 OLED is connected to.
/// Contains a minimal I2C protocol state machine (START/STOP/address/data)
/// and routes events to the I2cSlave trait methods on the device.
struct Ssd1306BusHandle {
    i2c_peripheral: String, // e.g. "I2C1"
    device: Ssd1306I2c,
    i2c_phase: I2cPhase,
}

/// Minimal I2C master state for protocol routing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum I2cPhase {
    Idle,
    AwaitAddress,
    MasterWrite,
}

impl Ssd1306BusHandle {
    fn on_cr1_write(&mut self, value: u32) {
        // START condition
        if (value & (1 << 8)) != 0 {
            self.i2c_phase = I2cPhase::AwaitAddress;
        }
        // STOP condition
        if (value & (1 << 9)) != 0 {
            if self.i2c_phase != I2cPhase::Idle {
                self.device.stop();
            }
            self.i2c_phase = I2cPhase::Idle;
        }
    }

    fn on_dr_write(&mut self, byte: u8) {
        match self.i2c_phase {
            I2cPhase::AwaitAddress => {
                let addr7 = byte >> 1;
                let read = (byte & 1) != 0;
                if self.device.address(addr7, read) && !read {
                    self.i2c_phase = I2cPhase::MasterWrite;
                } else {
                    self.i2c_phase = I2cPhase::Idle;
                }
            }
            I2cPhase::MasterWrite => {
                self.device.write_byte(byte);
            }
            _ => {}
        }
    }
}

/// Tracks an LED connected to a GPIO pin, for Tauri event emission.
struct LedHandle {
    id: String,
    port: char,
    pin: u8,
    active_low: bool,
    level_high: bool,
    last_on: Option<bool>,
}

impl LedHandle {
    fn on_state(&self) -> bool {
        if self.active_low { !self.level_high } else { self.level_high }
    }

    /// Process an MMIO event. Returns Some(true/false) if LED state changed.
    fn process_mmio(&mut self, event: &MmioWriteEvent) -> Option<bool> {
        // Check port match — event peripheral is like "GPIOA" or "A"
        let port_matches = event.peripheral.eq_ignore_ascii_case(&format!("GPIO{}", self.port))
            || event.peripheral.eq_ignore_ascii_case(&self.port.to_string());
        if !port_matches {
            return None;
        }
        if event.register.eq_ignore_ascii_case("ODR") {
            self.level_high = ((event.value >> self.pin) & 1) != 0;
        } else if event.register.eq_ignore_ascii_case("BSRR") {
            let set = ((event.value >> self.pin) & 1) != 0;
            let reset = ((event.value >> (self.pin + 16)) & 1) != 0;
            if set { self.level_high = true; }
            else if reset { self.level_high = false; }
            else { return None; }
        } else {
            return None;
        }
        let on = self.on_state();
        if self.last_on == Some(on) {
            return None;
        }
        self.last_on = Some(on);
        Some(on)
    }
}

// ── GPIO IDR address helpers ─────────────────────────────────────────────────

fn gpio_idr_addr(port: char, is_f407: bool) -> u64 {
    let idx = port.to_ascii_uppercase() as u64 - b'A' as u64;
    if is_f407 {
        0x4002_0000 + idx * 0x400 + 0x10
    } else {
        0x4001_0800 + idx * 0x400 + 0x08
    }
}

fn gpio_port_letter(name: &str) -> Option<char> {
    if name.starts_with("GPIO") {
        name.chars().nth(4)
    } else if name.len() == 1 && name.chars().next().unwrap_or('\0').is_ascii_alphabetic() {
        name.chars().next()
    } else {
        None
    }
}

// ── Public entry point ───────────────────────────────────────────────────────

pub fn run_emulator(config: SimConfig, app: AppHandle, control_rx: Receiver<ControlMsg>) {
    eprintln!("[EMU] Starting emulator for board: {}", config.board);
    eprintln!("[EMU] Firmware: {}", config.firmware_path);
    eprintln!("[EMU] Peripherals: {} items", config.peripherals.len());
    for (i, p) in config.peripherals.iter().enumerate() {
        eprintln!("[EMU]   [{}] {:?}", i, p);
    }

    let is_f407 = config.board == "stm32f407";
    let target = match if is_f407 {
        f407::load_target(Some(SVD_F407))
    } else {
        f103::load_target(Some(SVD_F103))
    } {
        Ok(t) => {
            eprintln!("[EMU] Target loaded: {} ({} peripherals)", t.name, t.peripherals.len());
            t
        }
        Err(e) => {
            eprintln!("[EMU] ERROR loading target: {}", e);
            app.emit("sim-status", SimStatusPayload { steps: 0, running: false, error: Some(e.clone()) }).ok();
            return;
        }
    };

    if is_f407 {
        run_machine(CortexM4::new(), target, config, app, control_rx, true);
    } else {
        run_machine(CortexM3::new(), target, config, app, control_rx, false);
    }
}

// ── Generic machine runner ───────────────────────────────────────────────────

fn run_machine<C: CpuCore>(
    cpu: C,
    target: TargetSpec,
    config: SimConfig,
    app: AppHandle,
    control_rx: Receiver<ControlMsg>,
    is_f407: bool,
) {
    // Extract values needed for pacer before moving target into Machine
    let core_clock_hz = target.core_clock_hz;
    let systick_reload_divider = target.systick_reload_divider;

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

    // ── Build bus-device context ───────────────────────────────────────────
    let mut st7789_handles: Vec<St7789BusHandle> = Vec::new();
    let mut ssd1306_handles: Vec<Ssd1306BusHandle> = Vec::new();
    let mut led_handles: Vec<LedHandle> = Vec::new();
    let mut uart_peripherals: Vec<String> = Vec::new();
    let mut gpio_notifier = GpioNotifier::new();

    eprintln!("[EMU] Initializing peripherals...");
    for (idx, pc) in config.peripherals.iter().enumerate() {
        match pc {
            GuiPeripheralConfig::St7789 { width, height, spi_base, cs, dc, res } => {
                eprintln!("[EMU]   [{}] ST7789 {}x{} SPI base 0x{:08x}", idx, width, height, spi_base);
                eprintln!("[EMU]        CS: P{}{}, DC: P{}{}", cs.port, cs.pin, dc.port, dc.pin);
                let device = St7789::new(
                    *width, *height, *spi_base,
                    cs.clone(), dc.clone(), res.clone(),
                    String::new(), false, true, // preview_enabled=true for debugging
                );
                let cs_port = cs.port.chars().next().unwrap_or('A').to_ascii_uppercase();
                let dc_port = dc.port.chars().next().unwrap_or('A').to_ascii_uppercase();
                // Find which SPI peripheral by scanning target for SPI at spi_base
                let spi_peripheral = target.peripherals.iter()
                    .find(|p| p.base_address == *spi_base)
                    .map(|p| p.name.to_ascii_uppercase())
                    .unwrap_or_else(|| format!("SPI{}", spi_base & 0xFFFF));
                eprintln!("[EMU]        → {} (bus-device routing)", spi_peripheral);
                st7789_handles.push(St7789BusHandle {
                    spi_peripheral,
                    cs_port,
                    cs_pin: cs.pin,
                    dc_port,
                    dc_pin: dc.pin,
                    device,
                });
            }
            GuiPeripheralConfig::Led { id, pin, active_low } => {
                eprintln!("[EMU]   [{}] LED {} on P{}{} (active_low={})", idx, id, pin.port, pin.pin, active_low);
                let port = pin.port.chars().next().unwrap_or('A').to_ascii_uppercase();
                // Register LED with GpioNotifier for GPIO dispatch
                let led = Led::new(id.clone(), pin.clone(), *active_low);
                gpio_notifier.register(
                    GpioPin::new(port, pin.pin),
                    Box::new(led),
                );
                // Also keep a handle for Tauri event emission
                led_handles.push(LedHandle {
                    id: id.clone(),
                    port,
                    pin: pin.pin,
                    active_low: *active_low,
                    level_high: true,
                    last_on: None,
                });
            }
            GuiPeripheralConfig::Uart { usart } => {
                eprintln!("[EMU]   [{}] UART {}", idx, usart);
                uart_peripherals.push(usart.clone());
            }
            GuiPeripheralConfig::Button { id, pin } => {
                eprintln!("[EMU]   [{}] Button {} on P{}{}", idx, id, pin.port, pin.pin);
            }
            GuiPeripheralConfig::Ssd1306I2c {
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
                ssd1306_handles.push(Ssd1306BusHandle {
                    i2c_peripheral,
                    device,
                    i2c_phase: I2cPhase::Idle,
                });
            }
        }
    }

    let has_display = !st7789_handles.is_empty() || !ssd1306_handles.is_empty();
    eprintln!("[EMU] Peripherals initialized: {} ST7789, {} SSD1306, {} LEDs, {} UARTs",
        st7789_handles.len(), ssd1306_handles.len(), led_handles.len(), uart_peripherals.len());

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
    let mut clocks = RccClockModel::new(core_clock_hz, is_f407);
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
    let mut uart_batch: Vec<(String, u8)> = Vec::new();
    let uart_batch_size = 64; // Send batch every 64 bytes

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
                    app.emit("sim-status", SimStatusPayload { steps, running: false, error: None }).ok();
                    return;
                }
                Ok(ControlMsg::InjectGpio { port, pin, high }) => {
                    if let Some(ch) = port.chars().next() {
                        let addr = gpio_idr_addr(ch, is_f407);
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

                if Instant::now() >= next_stream_deadline {
                    let event_begin = Instant::now();
                    let ev_stats = process_events(
                        &mut machine,
                        &mut serial_cursor,
                        &mut mmio_cursor,
                        &mut st7789_handles,
                        &mut ssd1306_handles,
                        &mut gpio_notifier,
                        &mut led_handles,
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

/// Dispatch events through the bus-device architecture.
fn process_events<C: CpuCore>(
    machine: &mut Machine<C>,
    serial_cursor: &mut usize,
    mmio_cursor: &mut usize,
    st7789_handles: &mut [St7789BusHandle],
    ssd1306_handles: &mut [Ssd1306BusHandle],
    gpio_notifier: &mut GpioNotifier,
    led_handles: &mut [LedHandle],
    uart_peripherals: &[String],
    has_display: bool,
    app: &AppHandle,
    clocks: &mut RccClockModel,
    uart_batch: &mut Vec<(String, u8)>,
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

    // Flush UART batch on size threshold or periodic stream tick.
    if uart_batch.len() >= uart_batch_size || !uart_batch.is_empty() {
        for (peripheral, byte) in uart_batch.drain(..) {
            app.emit("uart-output", UartOutputPayload { peripheral, byte }).ok();
        }
    }

    // ── MMIO write events → bus routing ─────────────────────────────────
    let mmio = machine.mmio_writes();
    stats.mmio_events = mmio.len().saturating_sub(*mmio_cursor);
    for event in &mmio[*mmio_cursor..] {
        let is_gpio = event.peripheral.starts_with("GPIO")
            || (event.peripheral.len() == 1 && event.peripheral.chars().next().unwrap_or('\0').is_ascii_alphabetic());
        let is_spi_dr = event.peripheral.starts_with("SPI")
            && event.register.eq_ignore_ascii_case("DR");
        let is_i2c_dr = event.peripheral.starts_with("I2C")
            && event.register.eq_ignore_ascii_case("DR");
        let is_i2c_cr1 = event.peripheral.starts_with("I2C")
            && event.register.eq_ignore_ascii_case("CR1");

        // Track display activity for pacer decision
        if has_display && (is_spi_dr || is_i2c_dr) {
            stats.display_activity = true;
        }

        // Route SPI DR → ST7789 SpiSlave::transfer()
        if is_spi_dr {
            for handle in st7789_handles.iter_mut() {
                if event.peripheral.eq_ignore_ascii_case(&handle.spi_peripheral) {
                    let byte = (event.value & 0xFF) as u8;
                    let _miso = SpiSlave::transfer(&mut handle.device, byte);
                }
            }
        }

        // Route I2C CR1/DR → SSD1306 I2C protocol state machine
        if is_i2c_cr1 {
            for handle in ssd1306_handles.iter_mut() {
                if event.peripheral.eq_ignore_ascii_case(&handle.i2c_peripheral) {
                    handle.on_cr1_write(event.value);
                }
            }
        }
        if is_i2c_dr {
            for handle in ssd1306_handles.iter_mut() {
                if event.peripheral.eq_ignore_ascii_case(&handle.i2c_peripheral) {
                    let byte = (event.value & 0xFF) as u8;
                    handle.on_dr_write(byte);
                }
            }
        }

        // Route GPIO → GpioNotifier (LEDs via trait) + ST7789 CS/DC pins
        if is_gpio {
            let port = gpio_port_letter(&event.peripheral).unwrap_or('A');
            if event.register.eq_ignore_ascii_case("ODR") {
                let val16 = (event.value & 0xFFFF) as u16;
                // Dispatch to GpioNotifier (LEDs)
                gpio_notifier.notify_mask_diff(port, 0, val16);
                // Dispatch to ST7789 CS/DC pins
                for handle in st7789_handles.iter_mut() {
                    if port == handle.cs_port {
                        let high = ((event.value >> handle.cs_pin) & 1) != 0;
                        handle.device.chip_select(!high); // CS is active-low: pin LOW = selected
                    }
                    if port == handle.dc_port {
                        let high = ((event.value >> handle.dc_pin) & 1) != 0;
                        GpioListener::pin_changed(&mut handle.device, port, handle.dc_pin, high);
                    }
                }
            } else if event.register.eq_ignore_ascii_case("BSRR") {
                let set_mask = event.value & 0xFFFF;
                let rst_mask = (event.value >> 16) & 0xFFFF;
                // Dispatch individual pin changes to GpioNotifier
                for pin in 0..16u8 {
                    if (set_mask >> pin) & 1 != 0 {
                        gpio_notifier.notify(port, pin, true);
                    }
                    if (rst_mask >> pin) & 1 != 0 {
                        gpio_notifier.notify(port, pin, false);
                    }
                }
                // Dispatch to ST7789 CS/DC pins
                for handle in st7789_handles.iter_mut() {
                    let cs_set = (set_mask >> handle.cs_pin) & 1 != 0;
                    let cs_rst = (rst_mask >> handle.cs_pin) & 1 != 0;
                    // CS is active-low: BSRR set → pin HIGH → NOT selected; reset → pin LOW → selected
                    if cs_set { handle.device.chip_select(false); }
                    if cs_rst { handle.device.chip_select(true); }

                    let dc_set = (set_mask >> handle.dc_pin) & 1 != 0;
                    let dc_rst = (rst_mask >> handle.dc_pin) & 1 != 0;
                    if dc_set { GpioListener::pin_changed(&mut handle.device, port, handle.dc_pin, true); }
                    if dc_rst { GpioListener::pin_changed(&mut handle.device, port, handle.dc_pin, false); }
                }
            }
            // Update LED handles for Tauri event emission
            for led in led_handles.iter_mut() {
                if let Some(on) = led.process_mmio(event) {
                    app.emit("led-changed", LedChangedPayload {
                        id: led.id.clone(),
                        on,
                    }).ok();
                }
            }
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
            // ST7789 displays
            for handle in st7789_handles.iter_mut() {
                if let Some(frame) = handle.device.latest_frame() {
                    stats.frame_emitted = true;
                    let frame_encode_begin = Instant::now();
                    let data = encode_argb_frame_base64(&frame);
                    stats.frame_encode_ns = stats
                        .frame_encode_ns
                        .saturating_add(frame_encode_begin.elapsed().as_nanos());
                    app.emit("display-frame", DisplayFramePayload {
                        width: handle.device.width(),
                        height: handle.device.height(),
                        data,
                    }).ok();
                }
            }
            // SSD1306 displays
            for handle in ssd1306_handles.iter_mut() {
                if let Some(frame) = handle.device.latest_frame() {
                    stats.frame_emitted = true;
                    let frame_encode_begin = Instant::now();
                    let data = encode_argb_frame_base64(&frame);
                    stats.frame_encode_ns = stats
                        .frame_encode_ns
                        .saturating_add(frame_encode_begin.elapsed().as_nanos());
                    app.emit("display-frame", DisplayFramePayload {
                        width: handle.device.width(),
                        height: handle.device.height(),
                        data,
                    }).ok();
                }
            }
        }
    }

    machine.clear_outputs();
    *serial_cursor = 0;
    *mmio_cursor = 0;

    stats
}

fn encode_argb_frame_base64(frame: &[u32]) -> String {
    if cfg!(target_endian = "little") {
        // ARGB u32 pixels are already little-endian in memory on all
        // supported host targets (x86_64/aarch64), so encode in-place.
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
