use base64::Engine;
use rsemu_core::cpu::armv7em::CortexM4;
use rsemu_core::cpu::armv7m::CortexM3;
use tauri::Emitter;
use std::thread;
use std::time::{Duration, Instant};

// Bundled SVD files — embedded at compile time so the user never needs to supply them.
const SVD_F103: &str = include_str!("../svd/stm32f103.svd");
const SVD_F407: &str = include_str!("../svd/stm32f407.svd");

// ── RealtimePacer: slows emulation to real-time clock speed ─────────────────

struct RealtimePacer {
    start: Instant,
    expected_elapsed_nanos: u128,
    nanos_per_step: Option<u128>,
    step_count: u64,
    cycles_per_step: u32,
    checkpoint_interval: u64,
    enabled: bool,
}

impl RealtimePacer {
    fn new(core_clock_hz: u32, cycles_per_step: u32) -> Self {
        let nanos_per_step = if core_clock_hz == 0 || cycles_per_step == 0 {
            None
        } else {
            Some(
                (u128::from(cycles_per_step) * 1_000_000_000u128)
                    .div_ceil(u128::from(core_clock_hz)),
            )
        };

        Self {
            start: Instant::now(),
            expected_elapsed_nanos: 0,
            nanos_per_step,
            step_count: 0,
            cycles_per_step,
            checkpoint_interval: 64,
            enabled: true,
        }
    }

    fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    /// Returns extra SysTick ticks to inject if we're lagging behind real-time.
    fn on_steps(&mut self, n: u32) -> u64 {
        if !self.enabled {
            return 0;
        }
        self.step_count = self.step_count.saturating_add(n as u64);
        if let Some(nanos_per_step) = self.nanos_per_step {
            self.expected_elapsed_nanos =
                self.expected_elapsed_nanos.saturating_add(nanos_per_step * n as u128);
        }
        if self.step_count % self.checkpoint_interval >= n as u64 {
            return 0;
        }
        if self.nanos_per_step.is_none() {
            return 0;
        }
        let expected = self.expected_elapsed_nanos;
        let elapsed = self.start.elapsed().as_nanos();
        if expected <= elapsed {
            // We're lagging: inject extra SysTick ticks to catch up
            let nanos_per_step = self.nanos_per_step.unwrap_or(0);
            if nanos_per_step == 0 {
                return 0;
            }
            let lag = elapsed - expected;
            let extra_ticks = lag / nanos_per_step;
            if extra_ticks > 0 {
                let gained = extra_ticks.saturating_mul(nanos_per_step);
                self.expected_elapsed_nanos = self.expected_elapsed_nanos.saturating_add(gained);
                return u64::try_from(extra_ticks).unwrap_or(u64::MAX);
            }
            return 0;
        }
        // We're ahead: sleep to slow down to real-time
        let remaining = expected - elapsed;
        let sleep_nanos = remaining.min(Duration::MAX.as_nanos());
        let sleep_nanos_u64 = u64::try_from(sleep_nanos).unwrap_or(u64::MAX);
        thread::sleep(Duration::from_nanos(sleep_nanos_u64));
        0
    }
}
use rsemu_core::{CpuCore, FirmwareLoader, Machine, MmioWriteEvent};
use rsemu_peripherals::display::St7789;
use rsemu_peripherals::Peripheral;
use rsemu_targets::stm32::{f103, f407};
use serde::Serialize;
use std::sync::mpsc::{Receiver, TryRecvError};
use tauri::AppHandle;

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

// ── LED state tracker ────────────────────────────────────────────────────────

struct LedTracker {
    id: String,
    port: String,
    pin: u8,
    active_low: bool,
    level_high: bool,
    last_on: Option<bool>,
}

impl LedTracker {
    fn port_matches(&self, peripheral: &str) -> bool {
        let p = peripheral.trim().to_ascii_uppercase();
        let wanted = self.port.trim().to_ascii_uppercase();
        p == format!("GPIO{wanted}") || p == wanted
    }

    fn on_state(&self) -> bool {
        if self.active_low { !self.level_high } else { self.level_high }
    }

    /// Returns true if the visible state changed.
    fn process_mmio(&mut self, event: &MmioWriteEvent) -> bool {
        if !self.port_matches(&event.peripheral) {
            return false;
        }
        if event.register.eq_ignore_ascii_case("ODR") {
            self.level_high = ((event.value >> self.pin) & 1) != 0;
        } else if event.register.eq_ignore_ascii_case("BSRR") {
            let set = ((event.value >> self.pin) & 1) != 0;
            let reset = ((event.value >> (self.pin + 16)) & 1) != 0;
            if set { self.level_high = true; }
            else if reset { self.level_high = false; }
            else { return false; }
        } else {
            return false;
        }
        let on = self.on_state();
        if self.last_on == Some(on) { return false; }
        self.last_on = Some(on);
        true
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
    target: rsemu_core::TargetSpec,
    config: SimConfig,
    app: AppHandle,
    control_rx: Receiver<ControlMsg>,
    is_f407: bool,
) {
    // Extract values needed for pacer before moving target into Machine
    let core_clock_hz = target.core_clock_hz;
    let systick_reload_divider = target.systick_reload_divider;

    let mut machine = Machine::new(cpu, target);

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

    // Build peripheral list and LED trackers
    let mut peripherals: Vec<Box<dyn Peripheral>> = Vec::new();
    let mut led_trackers: Vec<LedTracker> = Vec::new();
    let mut uart_peripherals: Vec<String> = Vec::new();
    let mut display_size: Option<(u16, u16)> = None;

    eprintln!("[EMU] Initializing peripherals...");
    for (idx, pc) in config.peripherals.iter().enumerate() {
        match pc {
            GuiPeripheralConfig::St7789 { width, height, spi_base, cs, dc, res } => {
                eprintln!("[EMU]   [{}] ST7789 {}x{} SPI base 0x{:08x}", idx, width, height, spi_base);
                eprintln!("[EMU]        CS: P{}{}, DC: P{}{}", cs.port, cs.pin, dc.port, dc.pin);
                peripherals.push(Box::new(St7789::new(
                    *width, *height, *spi_base,
                    cs.clone(), dc.clone(), res.clone(),
                    String::new(), false, true, // preview_enabled=true for debugging
                )));
                if display_size.is_none() {
                    display_size = Some((*width, *height));
                }
            }
            GuiPeripheralConfig::Led { id, pin, active_low } => {
                eprintln!("[EMU]   [{}] LED {} on P{}{} (active_low={})", idx, id, pin.port, pin.pin, active_low);
                led_trackers.push(LedTracker {
                    id: id.clone(),
                    port: pin.port.clone(),
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
        }
    }
    eprintln!("[EMU] Peripherals initialized: {} total", peripherals.len() + led_trackers.len() + uart_peripherals.len());

    // Initialize real-time pacer to make delays work correctly
    let base_emu_cycles_per_step = ((u64::from(systick_reload_divider) * 3) / 5).max(1);
    let emu_cycles_per_step = base_emu_cycles_per_step as u32;
    let mut pacer = RealtimePacer::new(core_clock_hz, emu_cycles_per_step);
    pacer.set_enabled(true); // Enable real-time pacing
    eprintln!("[EMU] RealtimePacer: {} Hz, {} cycles/step", core_clock_hz, emu_cycles_per_step);

    app.emit("sim-status", SimStatusPayload { steps: 0, running: true, error: None }).ok();
    eprintln!("[EMU] === Starting main loop ===");

    let mut steps = 0u64;
    let mut serial_cursor = 0usize;
    let mut mmio_cursor = 0usize;
    let mut last_log_steps = 0u64;
    let log_interval = 100_000u64;

    // Throttling: process events every N steps to avoid overwhelming the frontend
    let stream_interval_steps: u64 = 4096;
    let mut steps_since_stream = 0u64;

    // UART batching: collect bytes and send in batches
    let mut uart_batch: Vec<(String, u8)> = Vec::new();
    let uart_batch_size = 64; // Send batch every 64 bytes

    // Display frame throttling: max 10 fps
    let frame_interval_steps: u64 = 100_000; // ~10 fps at 1M steps/sec
    let mut steps_since_frame = 0u64;

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

        // Step the CPU
        match step_cpu_resilient(&mut machine) {
            Ok(ran) => {
                steps += ran as u64;
                steps_since_stream += ran as u64;
                steps_since_frame += ran as u64;

                // Real-time pacing: sleep or inject extra SysTick ticks
                let extra_systick_ticks = pacer.on_steps(ran);
                if extra_systick_ticks > 0 {
                    machine.advance_systick_ticks(extra_systick_ticks);
                }

                if steps_since_stream >= stream_interval_steps {
                    process_events(
                        &mut machine,
                        &mut serial_cursor,
                        &mut mmio_cursor,
                        &mut peripherals,
                        &mut led_trackers,
                        &uart_peripherals,
                        display_size,
                        &app,
                        steps,
                        &mut uart_batch,
                        uart_batch_size,
                        steps_since_frame >= frame_interval_steps,
                    );
                    steps_since_stream = 0;
                    if steps_since_frame >= frame_interval_steps {
                        steps_since_frame = 0;
                    }
                }

                // Periodic logging
                if steps - last_log_steps >= log_interval {
                    eprintln!("[EMU] Steps: {}, PC: 0x{:08x}, Serial: {} bytes, MMIO: {} events",
                        steps, machine.cpu().program_counter(),
                        machine.serial_output().len(), machine.mmio_writes().len());
                    last_log_steps = steps;
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

fn step_cpu_resilient<C: CpuCore>(machine: &mut Machine<C>) -> Result<u32, String> {
    let mut last_err = None;
    for batch in [1000usize, 10_000, 1_000, 100, 10, 1] {
        match machine.step_cpu(batch) {
            Ok(ran) => return Ok(ran),
            Err(e) => {
                if !e.contains(": MAP") && !e.contains(" MAP") {
                    return Err(e);
                }
                last_err = Some(e);
            }
        }
    }
    Err(last_err.unwrap_or_else(|| "cpu.step failed".into()))
}

fn process_events<C: CpuCore>(
    machine: &mut Machine<C>,
    serial_cursor: &mut usize,
    mmio_cursor: &mut usize,
    peripherals: &mut Vec<Box<dyn Peripheral>>,
    led_trackers: &mut Vec<LedTracker>,
    uart_peripherals: &[String],
    display_size: Option<(u16, u16)>,
    app: &AppHandle,
    steps: u64,
    uart_batch: &mut Vec<(String, u8)>,
    uart_batch_size: usize,
    send_frame: bool,
) {
    // ── Serial / UART output (batched) ───────────────────────────────────
    let serial = machine.serial_output();
    for event in &serial[*serial_cursor..] {
        let is_tracked = uart_peripherals.is_empty()
            || uart_peripherals.iter().any(|u| u.eq_ignore_ascii_case(&event.peripheral));
        if is_tracked {
            // Filter out ANSI escape sequences (simple heuristic)
            // ANSI codes start with ESC (0x1B) followed by '['
            // We'll strip them on the frontend side for now
            uart_batch.push((event.peripheral.clone(), event.byte));
        }
    }
    *serial_cursor = serial.len();

    // Send batch if full
    if uart_batch.len() >= uart_batch_size {
        // Debug: print raw bytes to confirm no duplication at source
        let raw: String = uart_batch.iter()
            .map(|(_, b)| if *b >= 0x20 && *b < 0x7f { *b as char } else { '·' })
            .collect();
        eprintln!("[EMU] UART batch ({} bytes): {:?}", uart_batch.len(), raw);
        for (peripheral, byte) in uart_batch.drain(..) {
            app.emit("uart-output", UartOutputPayload { peripheral, byte }).ok();
        }
    }

    // ── MMIO write events ────────────────────────────────────────────────
    let mmio = machine.mmio_writes();
    for event in &mmio[*mmio_cursor..] {
        // Dispatch to peripherals (e.g., St7789 SPI decoder)
        for p in peripherals.iter_mut() {
            p.on_mmio_write(machine, event);
        }
        // Update LED state trackers
        for led in led_trackers.iter_mut() {
            if led.process_mmio(event) {
                app.emit("led-changed", LedChangedPayload {
                    id: led.id.clone(),
                    on: led.on_state(),
                }).ok();
            }
        }
    }
    *mmio_cursor = mmio.len();

    // ── Display frame (throttled) ───────────────────────────────────────────
    if send_frame {
        if let Some((w, h)) = display_size {
            for p in peripherals.iter_mut() {
                if let Some(st) = p.as_any_mut().downcast_mut::<St7789>() {
                    if let Some(frame) = st.latest_frame() {
                        let raw: Vec<u8> = frame.iter()
                            .flat_map(|&px| px.to_le_bytes())
                            .collect();
                        let data = base64::engine::general_purpose::STANDARD.encode(&raw);
                        app.emit("display-frame", DisplayFramePayload { width: w, height: h, data }).ok();
                    }
                }
            }
        }
    }

    machine.clear_outputs();
    *serial_cursor = 0;
    *mmio_cursor = 0;

    // Periodic step counter update
    app.emit("sim-steps", steps).ok();
}
