use crate::cli::CliArgs;
use crate::clock_model::RccClockModel;
use minifb::{Key, Scale, Window, WindowOptions};
use rsemu_core::cpu::armv7em::CortexM4;
use rsemu_core::cpu::armv7m::CortexM3;
use rsemu_core::{
    CpuCore, FirmwareLoader, GpioListener, GpioNotifier, GpioPin, Machine, MmioWriteEvent,
    SpiSlave, TargetSpec,
};
use rsemu_peripherals::display::St7789;
use rsemu_peripherals::led::Led;
use rsemu_peripherals::PinMapping;
use rsemu_targets::stm32::{f103, f407};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;
use std::time::{Duration, Instant};
use tracing::{debug, info};

pub fn run() -> Result<(), String> {
    let args = CliArgs::parse()?;
    let board = load_board_config(&args.board_path)?;
    let svd_xml = read_optional_string(resolve_path(
        &args.board_path,
        board.svd.as_deref(),
    ))?;

    let is_f407 = matches!(
        board.target.as_deref(),
        Some("STM32F407") | Some("f407") | Some("stm32f407")
    );
    let mut target = match board.target.as_deref() {
        Some("STM32F407") | Some("f407") | Some("stm32f407") => f407::load_target(svd_xml.as_deref())?,
        _ => f103::load_target(svd_xml.as_deref())?,
    };

    if let Some(addr) = board.load_addr {
        target.vector_table_base = addr as u64;
    }
    let firmware_path = resolve_path(&args.board_path, board.firmware.as_deref());
    let cycle_scale = board.cycle_scale.unwrap_or(1).max(1);

    if is_f407 {
        let machine = Machine::new(CortexM4::new(), target.clone());
        run_with_cpu(machine, target, &args, &board, firmware_path, cycle_scale)
    } else {
        let machine = Machine::new(CortexM3::new(), target.clone());
        run_with_cpu(machine, target, &args, &board, firmware_path, cycle_scale)
    }
}

#[derive(Debug, Deserialize)]
struct BoardConfig {
    target: Option<String>,
    svd: Option<String>,
    firmware: Option<String>,
    load_addr: Option<u32>,
    cycle_scale: Option<u32>,
    #[serde(default)]
    peripherals: Vec<BoardPeripheralConfig>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
enum BoardPeripheralConfig {
    #[serde(rename = "st7789")]
    St7789 {
        #[allow(dead_code)]
        id: Option<String>,
        #[serde(default = "default_true")]
        enabled: bool,
        width: u16,
        height: u16,
        spi_base: u64,
        cs: PinMapping,
        dc: PinMapping,
        res: Option<PinMapping>,
        #[serde(default)]
        dump_frames: bool,
        #[serde(default = "default_output_dir")]
        output_dir: String,
    },
    #[serde(rename = "uart_terminal")]
    UartTerminal {
        #[allow(dead_code)]
        id: Option<String>,
        #[serde(default = "default_true")]
        enabled: bool,
        usart: String,
        tx: PinMapping,
        rx: PinMapping,
    },
    #[serde(rename = "led")]
    Led {
        id: Option<String>,
        #[serde(default = "default_true")]
        enabled: bool,
        pin: PinMapping,
        #[serde(default = "default_true")]
        active_low: bool,
    },
}

#[derive(Debug, Clone)]
struct UartTerminalBinding {
    usart: String,
    tx: PinMapping,
    rx: PinMapping,
}

fn default_true() -> bool {
    true
}

fn default_output_dir() -> String {
    "/tmp/rsemu-frames".to_string()
}

fn load_board_config(path: &str) -> Result<BoardConfig, String> {
    let raw = fs::read_to_string(path).map_err(|e| format!("read board config failed ({path}): {e}"))?;
    toml::from_str(&raw).map_err(|e| format!("parse board config failed ({path}): {e}"))
}

fn resolve_path(board_path: &str, maybe_rel: Option<&str>) -> Option<String> {
    let value = maybe_rel?;
    let value_path = Path::new(value);
    if value_path.is_absolute() {
        return Some(value.to_string());
    }
    let board_dir = Path::new(board_path).parent().unwrap_or_else(|| Path::new("."));
    let abs: PathBuf = board_dir.join(value_path);
    Some(abs.to_string_lossy().into_owned())
}

fn read_optional_string(path: Option<String>) -> Result<Option<String>, String> {
    let Some(path) = path else {
        return Ok(None);
    };
    let content = fs::read_to_string(&path).map_err(|e| format!("failed to read file {path}: {e}"))?;
    Ok(Some(content))
}

// ── Bus-device context (replaces Vec<Box<dyn Peripheral>>) ─────────────────

/// Tracks which SPI peripheral a ST7789 is connected to.
struct St7789BusHandle {
    spi_peripheral: String, // e.g. "SPI1"
    cs_port: char,
    cs_pin: u8,
    dc_port: char,
    dc_pin: u8,
    device: St7789,
}

fn run_with_cpu<C: CpuCore>(
    mut machine: Machine<C>,
    target: TargetSpec,
    args: &CliArgs,
    board: &BoardConfig,
    firmware_path: Option<String>,
    cycle_scale: u32,
) -> Result<(), String> {
    if let Some(firmware_path) = firmware_path.as_deref() {
        let load_addr = board.load_addr.unwrap_or(0x0800_0000);
        let firmware = FirmwareLoader::load_file(firmware_path, load_addr as u64)?;
        machine.load_firmware(&firmware)?;
        machine.reset_cpu()?;
    }

    info!("target: {}", target.name);
    info!("architecture: {:?}", target.architecture);
    info!("cpu: {}", machine.cpu().architecture().name());
    info!("memory regions: {}", target.memory_map.len());
    info!("peripherals: {}", target.peripherals.len());

    for peripheral in target.peripherals.iter().take(8) {
        info!(
            "  - {} @ 0x{:08x} ({} registers)",
            peripheral.name,
            peripheral.base_address,
            peripheral.registers.len()
        );
    }

    if firmware_path.is_some() {
        let base_emu_cycles_per_step = ((u64::from(target.systick_reload_divider) * 3) / 5).max(1);
        let emu_cycles_per_step = (base_emu_cycles_per_step * (u64::from(cycle_scale))).max(1) as u32;

        info!(
            "clock.core = {} Hz, emu_step = {} cycles (scale x{})",
            target.core_clock_hz, emu_cycles_per_step, cycle_scale
        );

        let mut steps = 0u64;
        let mut last_error = None;
        let mut recent_pcs = Vec::with_capacity(32);
        let mut serial_cursor = 0usize;
        let mut mmio_cursor = 0usize;

        // ── Build bus-device context ───────────────────────────────────────
        let mut st7789_handles: Vec<St7789BusHandle> = Vec::new();
        let mut uart_bindings: Vec<UartTerminalBinding> = Vec::new();
        let mut st7789_window_size: Option<(usize, usize)> = None;
        let mut gpio_notifier = GpioNotifier::new();
        // Track LED ids for event emission
        let mut led_ids: Vec<(String, char, u8)> = Vec::new();

        for periph in &board.peripherals {
            match periph {
                BoardPeripheralConfig::St7789 {
                    enabled,
                    width,
                    height,
                    spi_base,
                    cs,
                    dc,
                    res,
                    dump_frames,
                    output_dir,
                    ..
                } => {
                    if !enabled {
                        continue;
                    }
                    let preview_enabled = !args.no_gui;
                    let panel_width = *width;
                    let panel_height = *height;
                    let device = St7789::new(
                        panel_width,
                        panel_height,
                        *spi_base,
                        cs.clone(),
                        dc.clone(),
                        res.clone(),
                        output_dir.clone(),
                        *dump_frames || args.dump_frames,
                        preview_enabled,
                    );
                    let cs_port = cs.port.chars().next().unwrap_or('A').to_ascii_uppercase();
                    let dc_port = dc.port.chars().next().unwrap_or('A').to_ascii_uppercase();
                    // Find which SPI peripheral by scanning target for SPI at spi_base
                    let spi_peripheral = target.peripherals.iter()
                        .find(|p| p.base_address == *spi_base)
                        .map(|p| p.name.to_ascii_uppercase())
                        .unwrap_or_else(|| format!("SPI{}", spi_base & 0xFFFF));
                    info!("bus.register ST7789 → {} (CS=P{}{}, DC=P{}{})",
                        spi_peripheral, cs_port, cs.pin, dc_port, dc.pin);
                    st7789_handles.push(St7789BusHandle {
                        spi_peripheral,
                        cs_port,
                        cs_pin: cs.pin,
                        dc_port,
                        dc_pin: dc.pin,
                        device,
                    });
                    if st7789_window_size.is_none() {
                        st7789_window_size = Some((usize::from(panel_width), usize::from(panel_height)));
                    }
                }
                BoardPeripheralConfig::UartTerminal {
                    enabled,
                    usart,
                    tx,
                    rx,
                    ..
                } => {
                    if !enabled {
                        continue;
                    }
                    uart_bindings.push(UartTerminalBinding {
                        usart: usart.clone(),
                        tx: tx.clone(),
                        rx: rx.clone(),
                    });
                }
                BoardPeripheralConfig::Led {
                    id,
                    enabled,
                    pin,
                    active_low,
                } => {
                    if !enabled {
                        continue;
                    }
                    let led_id = id
                        .clone()
                        .unwrap_or_else(|| format!("{}{}", pin.port.to_ascii_uppercase(), pin.pin));
                    let port = pin.port.chars().next().unwrap_or('A').to_ascii_uppercase();
                    let led = Led::new(led_id.clone(), pin.clone(), *active_low);
                    gpio_notifier.register(
                        GpioPin::new(port, pin.pin),
                        Box::new(led),
                    );
                    led_ids.push((led_id.clone(), port, pin.pin));
                    info!("bus.register LED {} → P{}{}", led_id, port, pin.pin);
                }
            }
        }

        // Also register ST7789 as GpioListener for its CS and DC pins
        // (we do this via direct dispatch in the event loop since we need mutable access
        //  to the same St7789 object, and GpioNotifier can't hold a reference to it)

        let mut uart_terminals = Vec::new();
        let mut stdin_rx = Some(spawn_stdin_reader());
        let multi_uart = uart_bindings.len() > 1;
        for uart in &uart_bindings {
            info!(
                "uart.terminal = {} (TX P{}{}, RX P{}{})",
                uart.usart, uart.tx.port, uart.tx.pin, uart.rx.port, uart.rx.pin
            );
            match UartTerminalConsole::new(&uart.usart, stdin_rx.take(), multi_uart) {
                Ok(server) => uart_terminals.push(server),
                Err(err) => info!("uart.terminal.{} = disabled ({err})", uart.usart),
            }
        }
        if !uart_terminals.is_empty() {
            println!("--- Serial Console ---");
            println!("Input from keyboard will be sent to UART. Press Ctrl+C to exit.");
            println!("----------------------");
        }
        let active_uart_filters: Option<Vec<String>> = if uart_bindings.is_empty() {
            None
        } else {
            Some(uart_bindings.iter().map(|u| u.usart.clone()).collect())
        };

        let mut serial_lines: BTreeMap<String, Vec<u8>> = BTreeMap::new();
        let mut pacer = RealtimePacer::new(target.core_clock_hz, emu_cycles_per_step);
        pacer.set_enabled(!args.fast_mode);
        let is_f407_target = target.name.to_ascii_uppercase().contains("F407");
        let mut clocks = RccClockModel::new(target.core_clock_hz, is_f407_target);
        let heartbeat_interval = if args.fast_mode { 250_000u64 } else { 50_000u64 };
        let mut next_heartbeat_step = heartbeat_interval;
        let stream_interval_steps: u64 = if args.fast_mode { 4096 } else { 64 };
        let mut steps_since_stream = 0u64;
        let mut display_gui = if !args.no_gui {
            if let Some((width, height)) = st7789_window_size {
                match DisplayWindow::new("rsemu ST7789", width, height) {
                    Ok(window) => Some(window),
                    Err(err) => {
                        info!("display.gui = disabled ({err})");
                        None
                    }
                }
            } else {
                None
            }
        } else {
            None
        };
        loop {
            if args.max_steps.is_some_and(|max| steps >= max) {
                break;
            }

            match step_cpu_resilient(&mut machine, 1000) {
                Ok(ran) => {
                    steps += ran as u64;
                    steps_since_stream = steps_since_stream.saturating_add(ran as u64);

                    let pc = machine.cpu().program_counter();
                    if recent_pcs.len() >= 32 {
                        recent_pcs.remove(0);
                    }
                    recent_pcs.push(pc);

                    if steps_since_stream >= stream_interval_steps {
                        stream_new_events(
                            &mut machine,
                            &mut serial_cursor,
                            &mut mmio_cursor,
                            &mut serial_lines,
                            &mut clocks,
                            &mut pacer,
                            &mut st7789_handles,
                            &mut gpio_notifier,
                            &led_ids,
                            &mut uart_terminals,
                            active_uart_filters.as_deref(),
                            &mut display_gui,
                        );
                        steps_since_stream = 0;
                    }
                    let extra_systick_ticks = pacer.on_steps(ran);
                    if extra_systick_ticks > 0 {
                        machine.advance_systick_ticks(extra_systick_ticks);
                    }
                    if steps >= next_heartbeat_step {
                        info!(
                            "cpu.heartbeat steps={} pc=0x{:08x}",
                            steps,
                            machine.cpu().program_counter()
                        );
                        next_heartbeat_step = next_heartbeat_step.saturating_add(heartbeat_interval);
                    }
                }
                Err(err) => {
                    last_error = Some(err);
                    break;
                }
            }
        }

        if steps_since_stream > 0
            || !machine.serial_output().is_empty()
            || !machine.mmio_writes().is_empty()
        {
            stream_new_events(
                &mut machine,
                &mut serial_cursor,
                &mut mmio_cursor,
                &mut serial_lines,
                &mut clocks,
                &mut pacer,
                &mut st7789_handles,
                &mut gpio_notifier,
                &led_ids,
                &mut uart_terminals,
                active_uart_filters.as_deref(),
                &mut display_gui,
            );
        }

        if uart_terminals.is_empty() {
            flush_partial_serial_lines(&serial_lines);
        }
        if let Some(ref err) = last_error {
            eprintln!("cpu.step = error: {err}");
        } else {
            info!("cpu.steps = {steps}");
            info!("cpu.pc.final = 0x{:08x}", machine.cpu().program_counter());
            info!("cpu.step = ok");
        }
        let recent = recent_pcs
            .iter()
            .map(|pc| format!("0x{pc:08x}"))
            .collect::<Vec<_>>()
            .join(", ");
        debug!("trace.tail = [{recent}]");
    }

    Ok(())
}

fn step_cpu_resilient<C: CpuCore>(
    machine: &mut Machine<C>,
    preferred_batch: usize,
) -> Result<u32, String> {
    let mut last_err: Option<String> = None;
    for batch in [preferred_batch, 10_000, 1_000, 100, 10, 1] {
        match machine.step_cpu(batch) {
            Ok(ran) => return Ok(ran),
            Err(err) => {
                if !err.contains(": MAP") && !err.contains(" MAP") {
                    return Err(err);
                }
                last_err = Some(err);
            }
        }
    }
    Err(last_err.unwrap_or_else(|| "cpu.step failed".to_string()))
}

/// Dispatch events through the bus-device architecture.
fn stream_new_events<C: CpuCore>(
    machine: &mut Machine<C>,
    serial_cursor: &mut usize,
    mmio_cursor: &mut usize,
    serial_lines: &mut BTreeMap<String, Vec<u8>>,
    clocks: &mut RccClockModel,
    pacer: &mut RealtimePacer,
    st7789_handles: &mut [St7789BusHandle],
    gpio_notifier: &mut GpioNotifier,
    led_ids: &[(String, char, u8)],
    uart_terminals: &mut [UartTerminalConsole],
    active_uart_filters: Option<&[String]>,
    display_gui: &mut Option<DisplayWindow>,
) {
    let has_uart_terminals = !uart_terminals.is_empty();

    // ── Serial / UART output ────────────────────────────────────────────
    let serial_events = machine.serial_output();
    for event in &serial_events[*serial_cursor..] {
        if let Some(filters) = active_uart_filters
            && !filters.iter().any(|x| x.eq_ignore_ascii_case(&event.peripheral))
        {
            continue;
        }
        if has_uart_terminals {
            for term in uart_terminals.iter_mut() {
                if term.matches_peripheral(&event.peripheral) {
                    term.write_tx_byte(event.byte);
                }
            }
        } else {
            let line = serial_lines.entry(event.peripheral.clone()).or_default();
            line.push(event.byte);
            if event.byte == b'\n' {
                let text = String::from_utf8_lossy(line);
                println!("serial.{} = {:?}", event.peripheral, text);
                line.clear();
            }
        }
    }
    *serial_cursor = serial_events.len();

    // ── MMIO write events → bus routing ─────────────────────────────────
    let mmio_events = machine.mmio_writes();
    for event in &mmio_events[*mmio_cursor..] {
        let is_gpio = event.peripheral.starts_with("GPIO");
        let is_spi_dr = event.peripheral.starts_with("SPI")
            && event.register.eq_ignore_ascii_case("DR");

        // Route SPI DR → ST7789 SpiSlave::transfer()
        if is_spi_dr {
            for handle in st7789_handles.iter_mut() {
                if event.peripheral.eq_ignore_ascii_case(&handle.spi_peripheral) {
                    let byte = (event.value & 0xFF) as u8;
                    let _miso = SpiSlave::transfer(&mut handle.device, byte);
                    // MISO data is ignored for ST7789 (write-only display)
                }
            }
        }

        // Route GPIO → GpioNotifier (LEDs) + ST7789 CS/DC pins
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

                // Log GPIO events (non-uart mode)
                if !has_uart_terminals {
                    println!("{}", format_gpio_event(event));
                }
            }
        }

        // RCC clock model
        if event.peripheral.starts_with("RCC")
            && let Some(new_core_hz) = clocks.apply_mmio(event)
        {
            pacer.set_core_clock_hz(new_core_hz);
        }
    }
    *mmio_cursor = mmio_events.len();

    // ── Display frame capture ───────────────────────────────────────────
    if let Some(window) = display_gui.as_mut() {
        for handle in st7789_handles.iter_mut() {
            if let Some(frame) = handle.device.latest_frame() {
                let _ = window.present(handle.device.width(), handle.device.height(), &frame);
            }
        }
        if !window.tick() {
            info!("display.gui = closed");
            *display_gui = None;
        }
    }

    // ── UART RX from stdin ──────────────────────────────────────────────
    for term in uart_terminals.iter_mut() {
        let rx_bytes = term.read_rx_bytes();
        for byte in rx_bytes {
            let _ = machine.usart_push_rx_byte(term.peripheral_name(), byte);
        }
    }

    machine.clear_outputs();
    *serial_cursor = 0;
    *mmio_cursor = 0;

    // Suppress unused warning
    let _ = led_ids;
}

fn format_gpio_event(event: &MmioWriteEvent) -> String {
    let base = format!(
        "gpio.{}.{} @ 0x{:08x} <= 0x{:08x}",
        event.peripheral, event.register, event.addr, event.value
    );
    let Some(port) = gpio_port_letter(&event.peripheral) else {
        return base;
    };
    if event.register.eq_ignore_ascii_case("BSRR") {
        let set_mask = event.value & 0xFFFF;
        let rst_mask = (event.value >> 16) & 0xFFFF;
        let set_pins = mask_to_gpio_pins(port, set_mask);
        let rst_pins = mask_to_gpio_pins(port, rst_mask);
        return format!(
            "{base} (set: [{}], reset: [{}])",
            set_pins.join(", "),
            rst_pins.join(", ")
        );
    }
    if event.register.eq_ignore_ascii_case("ODR") {
        let high = mask_to_gpio_pins(port, event.value & 0xFFFF);
        return format!("{base} (high: [{}])", high.join(", "));
    }
    base
}

fn gpio_port_letter(name: &str) -> Option<char> {
    if !name.starts_with("GPIO") {
        return None;
    }
    name.chars().nth(4)
}

fn mask_to_gpio_pins(port: char, mask: u32) -> Vec<String> {
    let mut pins = Vec::new();
    for bit in 0..16 {
        if (mask & (1u32 << bit)) != 0 {
            pins.push(format!("P{port}{bit}"));
        }
    }
    pins
}

fn flush_partial_serial_lines(serial_lines: &BTreeMap<String, Vec<u8>>) {
    for (peripheral, bytes) in serial_lines {
        if bytes.is_empty() {
            continue;
        }
        let text = String::from_utf8_lossy(bytes);
        println!("serial.{peripheral} = {:?} (partial)", text);
    }
}

struct DisplayWindow {
    window: Window,
    width: usize,
    height: usize,
    front_buffer: Vec<u32>,
    last_tick: Instant,
    tick_interval: Duration,
    presents: u64,
}

impl DisplayWindow {
    fn new(title: &str, width: usize, height: usize) -> Result<Self, String> {
        let window = Window::new(
            title,
            width,
            height,
            WindowOptions {
                resize: false,
                scale: Scale::X2,
                ..WindowOptions::default()
            },
        )
        .map_err(|e| format!("create window failed: {e}"))?;
        Ok(Self {
            window,
            width,
            height,
            front_buffer: vec![0; width * height],
            last_tick: Instant::now(),
            tick_interval: Duration::from_millis(16),
            presents: 0,
        })
    }

    fn present(&mut self, width: u16, height: u16, pixels: &[u32]) -> Result<(), String> {
        if !self.window.is_open() || self.window.is_key_down(Key::Escape) {
            return Err("window closed".to_string());
        }
        if usize::from(width) != self.width || usize::from(height) != self.height {
            return Err(format!(
                "frame size {}x{} mismatches window {}x{}",
                width, height, self.width, self.height
            ));
        }
        self.front_buffer.clear();
        self.front_buffer.extend_from_slice(pixels);
        self.presents = self.presents.saturating_add(1);
        self.window
            .update_with_buffer(&self.front_buffer, self.width, self.height)
            .map_err(|e| format!("update window failed: {e}"))
    }

    fn tick(&mut self) -> bool {
        if self.last_tick.elapsed() < self.tick_interval {
            return true;
        }
        self.last_tick = Instant::now();
        let _ = self
            .window
            .update_with_buffer(&self.front_buffer, self.width, self.height);
        self.window.is_open() && !self.window.is_key_down(Key::Escape)
    }
}

struct UartTerminalConsole {
    peripheral: String,
    stdin_rx: Option<Receiver<u8>>,
    show_prefix: bool,
    at_line_start: bool,
}

impl UartTerminalConsole {
    fn new(
        peripheral: &str,
        stdin_rx: Option<Receiver<u8>>,
        show_prefix: bool,
    ) -> Result<Self, String> {
        Ok(Self {
            peripheral: peripheral.to_string(),
            stdin_rx,
            show_prefix,
            at_line_start: true,
        })
    }

    fn peripheral_name(&self) -> &str {
        &self.peripheral
    }

    fn matches_peripheral(&self, peripheral: &str) -> bool {
        self.peripheral.eq_ignore_ascii_case(peripheral)
    }

    fn write_tx_byte(&mut self, byte: u8) {
        let mut out = std::io::stdout();
        if self.show_prefix && self.at_line_start {
            let _ = write!(out, "[{}] ", self.peripheral);
        }
        let _ = out.write_all(&[byte]);
        self.at_line_start = byte == b'\n';
        if byte == b'\n' {
            let _ = out.flush();
        }
    }

    fn read_rx_bytes(&mut self) -> Vec<u8> {
        let mut out = Vec::new();
        let Some(rx) = self.stdin_rx.as_ref() else {
            return out;
        };
        loop {
            match rx.try_recv() {
                Ok(byte) => out.push(byte),
                Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => break,
            }
        }
        out
    }
}

fn spawn_stdin_reader() -> Receiver<u8> {
    let (tx, rx) = mpsc::channel::<u8>();
    thread::spawn(move || {
        let stdin = std::io::stdin();
        let mut lock = stdin.lock();
        let mut buf = [0u8; 1];
        loop {
            match lock.read(&mut buf) {
                Ok(0) => break,
                Ok(1) => {
                    if tx.send(buf[0]).is_err() {
                        break;
                    }
                }
                Ok(_) => {}
                Err(_) => break,
            }
        }
    });
    rx
}

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

    fn set_core_clock_hz(&mut self, core_clock_hz: u32) {
        self.nanos_per_step = if core_clock_hz == 0 || self.cycles_per_step == 0 {
            None
        } else {
            Some(
                (u128::from(self.cycles_per_step) * 1_000_000_000u128)
                    .div_ceil(u128::from(core_clock_hz)),
            )
        };
    }

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
        let remaining = expected - elapsed;
        let sleep_nanos = remaining.min(Duration::MAX.as_nanos());
        let sleep_nanos_u64 = u64::try_from(sleep_nanos).unwrap_or(u64::MAX);
        thread::sleep(Duration::from_nanos(sleep_nanos_u64));
        0
    }
}
