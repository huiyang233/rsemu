use base64::Engine;
use rsemu_core::cpu::armv7em::CortexM4;
use rsemu_core::cpu::armv7m::CortexM3;
use tauri::Emitter;

// Bundled SVD files — embedded at compile time so the user never needs to supply them.
const SVD_F103: &str = include_str!("../svd/stm32f103.svd");
const SVD_F407: &str = include_str!("../svd/stm32f407.svd");
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
    let is_f407 = config.board == "stm32f407";
    let target = match if is_f407 {
        f407::load_target(Some(SVD_F407))
    } else {
        f103::load_target(Some(SVD_F103))
    } {
        Ok(t) => t,
        Err(e) => {
            app.emit("sim-status", SimStatusPayload { steps: 0, running: false, error: Some(e) }).ok();
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
    let mut machine = Machine::new(cpu, target);

    // Load firmware
    let fw = match FirmwareLoader::load_file(&config.firmware_path, 0x0800_0000) {
        Ok(fw) => fw,
        Err(e) => {
            app.emit("sim-status", SimStatusPayload { steps: 0, running: false, error: Some(e) }).ok();
            return;
        }
    };
    if let Err(e) = machine.load_firmware(&fw) {
        app.emit("sim-status", SimStatusPayload { steps: 0, running: false, error: Some(e) }).ok();
        return;
    }
    if let Err(e) = machine.reset_cpu() {
        app.emit("sim-status", SimStatusPayload { steps: 0, running: false, error: Some(e) }).ok();
        return;
    }

    // Build peripheral list and LED trackers
    let mut peripherals: Vec<Box<dyn Peripheral>> = Vec::new();
    let mut led_trackers: Vec<LedTracker> = Vec::new();
    let mut uart_peripherals: Vec<String> = Vec::new();
    let mut display_size: Option<(u16, u16)> = None;

    for pc in &config.peripherals {
        match pc {
            GuiPeripheralConfig::St7789 { width, height, spi_base, cs, dc, res } => {
                peripherals.push(Box::new(St7789::new(
                    *width, *height, *spi_base,
                    cs.clone(), dc.clone(), res.clone(),
                    String::new(), false, false,
                )));
                if display_size.is_none() {
                    display_size = Some((*width, *height));
                }
            }
            GuiPeripheralConfig::Led { id, pin, active_low } => {
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
                uart_peripherals.push(usart.clone());
            }
            GuiPeripheralConfig::Button { .. } => {} // input handled via inject_gpio command
        }
    }

    app.emit("sim-status", SimStatusPayload { steps: 0, running: true, error: None }).ok();

    let mut steps = 0u64;
    let mut serial_cursor = 0usize;
    let mut mmio_cursor = 0usize;

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
                if steps % 128 < ran as u64 {
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
                    );
                }
            }
            Err(e) => {
                app.emit("sim-status", SimStatusPayload { steps, running: false, error: Some(e) }).ok();
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
) {
    // ── Serial / UART output ──────────────────────────────────────────────
    let serial = machine.serial_output();
    for event in &serial[*serial_cursor..] {
        let is_tracked = uart_peripherals.is_empty()
            || uart_peripherals.iter().any(|u| u.eq_ignore_ascii_case(&event.peripheral));
        if is_tracked {
            app.emit("uart-output", UartOutputPayload {
                peripheral: event.peripheral.clone(),
                byte: event.byte,
            }).ok();
        }
    }
    *serial_cursor = serial.len();

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

    // ── Display frame ─────────────────────────────────────────────────────
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

    machine.clear_outputs();
    *serial_cursor = 0;
    *mmio_cursor = 0;

    // Periodic step counter update
    app.emit("sim-steps", steps).ok();
}
