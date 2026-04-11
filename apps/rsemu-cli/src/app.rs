use crate::cli::CliArgs;
use minifb::{Key, Scale, Window, WindowOptions};
use rsemu_core::cpu::armv7em::CortexM4;
use rsemu_core::cpu::armv7m::CortexM3;
use rsemu_core::{
    BusContext, CpuCore, CpuType, FirmwareLoader, GpioPin, Machine, RccClockModel,
    SpiBus, StepBatchController, TargetSpec, gpio_port_letter,
};
use rsemu_peripherals::display::St7789;
use rsemu_peripherals::led::Led;
use rsemu_peripherals::{PeripheralConfig, PinMapping};
use rsemu_targets::TargetRegistry;
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

    // Resolve configs/ and svds/ dirs: search from board.toml parent, then walk up to project root
    let board_dir = Path::new(&args.board_path)
        .parent()
        .unwrap_or(Path::new("."));
    let project_root = find_project_root(board_dir);
    let configs_dir = project_root.join("configs");
    let svds_dir = project_root.join("svds");
    let registry = TargetRegistry::from_dirs(&configs_dir, &svds_dir)?;

    let target_id = board
        .target
        .as_deref()
        .unwrap_or("stm32f103")
        .to_ascii_lowercase();
    let mut target = registry.load(&target_id)?;

    if let Some(addr) = board.load_addr {
        target.vector_table_base = addr as u64;
    }
    let firmware_path = resolve_path(&args.board_path, board.firmware.as_deref());
    let cycle_scale = board.cycle_scale.unwrap_or(1).max(1);

    match target.cpu_type {
        CpuType::CortexM4 => {
            let machine = Machine::new(CortexM4::new()?, target.clone());
            run_with_cpu(machine, target, &args, &board, firmware_path, cycle_scale)
        }
        CpuType::CortexM3 => {
            let machine = Machine::new(CortexM3::new()?, target.clone());
            run_with_cpu(machine, target, &args, &board, firmware_path, cycle_scale)
        }
    }
}

#[derive(Debug, Deserialize)]
struct BoardConfig {
    target: Option<String>,
    firmware: Option<String>,
    load_addr: Option<u32>,
    cycle_scale: Option<u32>,
    #[serde(default)]
    peripherals: Vec<PeripheralConfig>,
}

#[derive(Debug, Clone)]
struct UartTerminalBinding {
    usart: String,
    tx: PinMapping,
    rx: PinMapping,
}

fn load_board_config(path: &str) -> Result<BoardConfig, String> {
    let raw = fs::read_to_string(path).map_err(|e| format!("read board config failed ({path}): {e}"))?;
    toml::from_str(&raw).map_err(|e| format!("parse board config failed ({path}): {e}"))
}

/// Walk up from `start` to find a directory containing `configs/`.
fn find_project_root(start: &Path) -> PathBuf {
    let mut dir = if start.is_absolute() {
        start.to_path_buf()
    } else {
        std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")).join(start)
    };
    dir = dir.canonicalize().unwrap_or(dir);
    loop {
        if dir.join("configs").is_dir() {
            return dir;
        }
        if !dir.pop() {
            break;
        }
    }
    // Fallback: use start dir
    start.to_path_buf()
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

        // ── Build BusContext ────────────────────────────────────────────────
        let mut bus_ctx = BusContext::new();
        let mut uart_bindings: Vec<UartTerminalBinding> = Vec::new();
        let mut st7789_window_size: Option<(usize, usize)> = None;

        for periph in &board.peripherals {
            match periph {
                PeripheralConfig::St7789Spi {
                    width,
                    height,
                    spi_base,
                    cs,
                    dc,
                    res,
                } => {
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
                        String::new(),
                        false,
                        preview_enabled,
                    );
                    let cs_port = cs.port.chars().next().unwrap_or('A').to_ascii_uppercase();
                    let dc_port = dc.port.chars().next().unwrap_or('A').to_ascii_uppercase();
                    let spi_peripheral = target.peripherals.iter()
                        .find(|p| p.base_address == *spi_base)
                        .map(|p| p.name.to_ascii_uppercase())
                        .unwrap_or_else(|| format!("SPI{}", spi_base & 0xFFFF));
                    info!("bus.register ST7789 → {} (CS=P{}{}, DC=P{}{})",
                        spi_peripheral, cs_port, cs.pin, dc_port, dc.pin);
                    let spi_bus = SpiBus::new(Box::new(device));
                    let cs_pin = Some(GpioPin::new(cs_port, cs.pin));
                    let dc_pin = Some(GpioPin::new(dc_port, dc.pin));
                    bus_ctx.register_spi(spi_peripheral, spi_bus, cs_pin, dc_pin);
                    if st7789_window_size.is_none() {
                        st7789_window_size = Some((usize::from(panel_width), usize::from(panel_height)));
                    }
                }
                PeripheralConfig::Uart {
                    usart,
                    tx,
                    rx,
                } => {
                    if let (Some(tx), Some(rx)) = (tx, rx) {
                        uart_bindings.push(UartTerminalBinding {
                            usart: usart.clone(),
                            tx: tx.clone(),
                            rx: rx.clone(),
                        });
                    }
                }
                PeripheralConfig::Led {
                    id,
                    pin,
                    active_low,
                } => {
                    let led_id = id
                        .clone()
                        .unwrap_or_else(|| format!("{}{}", pin.port.to_ascii_uppercase(), pin.pin));
                    let port = pin.port.chars().next().unwrap_or('A').to_ascii_uppercase();
                    let led = Led::new(led_id.clone(), pin.clone(), *active_low);
                    bus_ctx.register_gpio(
                        GpioPin::new(port, pin.pin),
                        Box::new(led),
                    );
                    info!("bus.register LED {} → P{}{}", led_id, port, pin.pin);
                }
                _ => {}
            }
        }

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
        let mut clocks = RccClockModel::new(target.core_clock_hz, target.hsi_hz, target.has_pllcfgr);
        let mut step_batch = StepBatchController::new(1000);
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

            match step_batch.step_cpu(&mut machine) {
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
                            &mut bus_ctx,
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
                &mut bus_ctx,
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

/// Dispatch events through BusContext.
fn stream_new_events<C: CpuCore>(
    machine: &mut Machine<C>,
    serial_cursor: &mut usize,
    mmio_cursor: &mut usize,
    serial_lines: &mut BTreeMap<String, Vec<u8>>,
    clocks: &mut RccClockModel,
    pacer: &mut RealtimePacer,
    bus_ctx: &mut BusContext,
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
            let line = serial_lines.entry(event.peripheral.to_string()).or_default();
            line.push(event.byte);
            if event.byte == b'\n' {
                let text = String::from_utf8_lossy(line);
                println!("serial.{} = {:?}", event.peripheral, text);
                line.clear();
            }
        }
    }
    *serial_cursor = serial_events.len();

    // ── MMIO write events → BusContext routing ──────────────────────────
    let mmio_events = machine.mmio_writes();
    for event in &mmio_events[*mmio_cursor..] {
        let _stats = bus_ctx.dispatch_mmio(event);

        // Log GPIO events (non-uart mode)
        if !has_uart_terminals && event.peripheral.starts_with("GPIO") {
            println!("{}", format_gpio_event(event));
        }

        // RCC clock model
        if event.peripheral.starts_with("RCC")
            && let Some(new_core_hz) = clocks.apply_mmio(event)
        {
            pacer.set_core_clock_hz(new_core_hz);
        }
    }
    let _ = bus_ctx; // suppress unused warning if no devices
    *mmio_cursor = mmio_events.len();

    // ── Display frame capture ───────────────────────────────────────────
    if let Some(window) = display_gui.as_mut() {
        for frame in bus_ctx.poll_frames() {
            let _ = window.present(frame.width, frame.height, &frame.pixels);
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
}

fn format_gpio_event(event: &rsemu_core::MmioWriteEvent) -> String {
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
