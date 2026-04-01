use crate::cli::CliArgs;
use minifb::{Key, Scale, Window, WindowOptions};
use rsemu_core::cpu::armv7m::CortexM3;
use rsemu_core::{CpuCore, FirmwareLoader, Machine, MmioWriteEvent};
use rsemu_targets::stm32::{f103, f407};
use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::Write as IoWrite;
use std::thread;
use std::time::{Duration, Instant};
use tracing::{debug, info, warn};

const ENABLE_ST7789_CAPTURE: bool = true;

pub fn run() -> Result<(), String> {
    let args = CliArgs::parse()?;
    let mut target = match args.target.as_deref() {
        Some("STM32F407") | Some("f407") | Some("stm32f407") => f407::load_target(args.svd_xml.as_deref())?,
        _ => f103::load_target(args.svd_xml.as_deref())?,
    };
    if let Some(addr) = args.load_addr {
        target.vector_table_base = addr as u64;
    }
    let mut machine = Machine::new(CortexM3::new(), target.clone());
    let has_firmware = args.firmware_path.is_some();

    if let Some(firmware_path) = args.firmware_path.as_deref() {
        let load_addr = args.load_addr.unwrap_or(0x0800_0000);
        let firmware = FirmwareLoader::load_file(firmware_path, load_addr as u64)?;
        machine.load_firmware(&firmware)?;
        machine.reset_cpu()?;
    }

    info!("target: {}", target.name);
    info!("architecture: {:?}", target.architecture);
    info!("cpu: {}", machine.cpu().architecture().name());
    info!("cpu.backend: {:?}", machine.cpu().backend());
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

    if let Some(first_register) = target
        .peripherals
        .iter()
        .flat_map(|peripheral| peripheral.registers.iter())
        .next()
    {
        let value = machine.read8(first_register.address)?;
        info!(
            "probe register {} @ 0x{:08x} => 0x{:02x}",
            first_register.name, first_register.address, value
        );
    }

    if has_firmware {
        let base_emu_cycles_per_step = ((u64::from(target.systick_reload_divider) * 3) / 5).max(1);
        let emu_cycles_per_step =
            (base_emu_cycles_per_step.saturating_mul(u64::from(args.cycle_scale))).min(u64::from(u32::MAX))
                as u32;
        info!("cpu.sp = 0x{:08x}", machine.cpu().stack_pointer());
        info!("cpu.pc = 0x{:08x}", machine.cpu().program_counter());
        info!(
            "clock.core = {} Hz, emu_step = {} cycles (scale x{})",
            target.core_clock_hz, emu_cycles_per_step, args.cycle_scale
        );

        let mut steps = 0u64;
        let mut last_error = None;
        let mut recent_pcs = Vec::with_capacity(32);
        let mut serial_cursor = 0usize;
        let mut mmio_cursor = 0usize;
        let mut st7789 = St7789Capture::new(
            240,
            320,
            "/tmp/rsemu-st7789",
            args.dump_frames,
            args.display_gui,
        );
        let mut serial_lines: BTreeMap<String, Vec<u8>> = BTreeMap::new();
        let mut pacer = RealtimePacer::new(target.core_clock_hz, emu_cycles_per_step);
        pacer.set_enabled(!args.fast_mode);
        let mut clocks = RccClockModel::new(target.core_clock_hz);
        let mut gpio_log_limiter = GpioLogLimiter::new(Duration::from_millis(100));
        let heartbeat_interval = if args.fast_mode { 250_000u64 } else { 50_000u64 };
        let mut next_heartbeat_step = heartbeat_interval;
        let stream_interval_steps: u64 = if args.fast_mode { 4096 } else { 64 };
        let mut steps_since_stream = 0u64;
        let mut display_gui = if args.display_gui {
            match DisplayWindow::new("rsemu ST7789", 240, 320) {
                Ok(window) => Some(window),
                Err(err) => {
                    info!("display.gui = disabled ({err})");
                    None
                }
            }
        } else {
            None
        };
        loop {
            if args.max_steps.is_some_and(|max| steps >= max) {
                break;
            }

            let pc = machine.cpu().program_counter();
            if let (Some(start), Some(end)) = (args.trace_start, args.trace_end) {
                if (start as u64..=end as u64).contains(&pc) {
                    let regs = machine.cpu().registers();
                    debug!(
                        "trace.step pc=0x{pc:08x} r0=0x{:08x} r1=0x{:08x} r2=0x{:08x} r3=0x{:08x} r4=0x{:08x} r5=0x{:08x} r6=0x{:08x} r7=0x{:08x} r8=0x{:08x} r9=0x{:08x} r10=0x{:08x} r11=0x{:08x} r12=0x{:08x} sp=0x{:08x} lr=0x{:08x}",
                        regs[0],
                        regs[1],
                        regs[2],
                        regs[3],
                        regs[4],
                        regs[5],
                        regs[6],
                        regs[7],
                        regs[8],
                        regs[9],
                        regs[10],
                        regs[11],
                        regs[12],
                        regs[13],
                        regs[14]
                    );
                }
            }
            if recent_pcs.len() == 32 {
                recent_pcs.remove(0);
            }
            recent_pcs.push(pc);

            match machine.step_cpu() {
                Ok(()) => {
                    steps += 1;
                    steps_since_stream = steps_since_stream.saturating_add(1);
                    if steps_since_stream >= stream_interval_steps {
                        stream_new_events(
                            &mut machine,
                            &mut serial_cursor,
                            &mut mmio_cursor,
                            &mut serial_lines,
                            &mut clocks,
                            &mut pacer,
                            &mut st7789,
                            &mut gpio_log_limiter,
                            &mut display_gui,
                        );
                        steps_since_stream = 0;
                    }
                    let extra_systick_ticks = pacer.on_step();
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

        if steps_since_stream > 0 {
            stream_new_events(
                &mut machine,
                &mut serial_cursor,
                &mut mmio_cursor,
                &mut serial_lines,
                &mut clocks,
                &mut pacer,
                &mut st7789,
                &mut gpio_log_limiter,
                &mut display_gui,
            );
        }

        flush_partial_serial_lines(&serial_lines);

        info!("cpu.steps = {}", steps);
        info!("cpu.pc.final = 0x{:08x}", machine.cpu().program_counter());
        if let Some(err) = last_error {
            info!("cpu.step = {err}");
        } else {
            info!("cpu.step = ok");
        }

        if let Some(summary) = summarize_clock_writes(machine.mmio_writes()) {
            info!("{summary}");
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

fn stream_new_events(
    machine: &mut Machine<CortexM3>,
    serial_cursor: &mut usize,
    mmio_cursor: &mut usize,
    serial_lines: &mut BTreeMap<String, Vec<u8>>,
    clocks: &mut RccClockModel,
    pacer: &mut RealtimePacer,
    st7789: &mut St7789Capture,
    gpio_log_limiter: &mut GpioLogLimiter,
    display_gui: &mut Option<DisplayWindow>,
) {
    let serial_events = machine.serial_output();
    for event in &serial_events[*serial_cursor..] {
        let line = serial_lines.entry(event.peripheral.clone()).or_default();
        line.push(event.byte);
        if event.byte == b'\n' {
            let text = String::from_utf8_lossy(line);
            info!("serial.{} = {:?}", event.peripheral, text);
            line.clear();
        }
    }
    *serial_cursor = serial_events.len();

    let mmio_events = machine.mmio_writes();
    for event in &mmio_events[*mmio_cursor..] {
        if ENABLE_ST7789_CAPTURE {
            if let Some(path) = st7789.apply_mmio(event) {
                debug!("display.st7789.frame = {}", path);
            }
        }
        if is_usart_data_write(event) {
            debug!(
                "mmio.{}.{} @ 0x{:08x} <= 0x{:08x} ({}-bit)",
                event.peripheral,
                event.register,
                event.addr,
                event.value,
                event.width as u32 * 8
            );
        } else if event.peripheral.starts_with("GPIO")
            && gpio_log_limiter.should_log(&event.peripheral, &event.register)
        {
            if event.register.eq_ignore_ascii_case("ODR") {
                debug!(
                    "mmio.{}.{} @ 0x{:08x} <= 0x{:08x} ({}-bit)",
                    event.peripheral,
                    event.register,
                    event.addr,
                    event.value,
                    event.width as u32 * 8
                );
            } else {
                info!(
                    "mmio.{}.{} @ 0x{:08x} <= 0x{:08x} ({}-bit)",
                    event.peripheral,
                    event.register,
                    event.addr,
                    event.value,
                    event.width as u32 * 8
                );
            }
        } else {
            debug!(
                "mmio.{}.{} @ 0x{:08x} <= 0x{:08x} ({}-bit)",
                event.peripheral,
                event.register,
                event.addr,
                event.value,
                event.width as u32 * 8
            );
        }
        if let Some(new_core_hz) = clocks.apply_mmio(event) {
            pacer.set_core_clock_hz(new_core_hz);
            info!(
                "clock.core.dynamic = {} Hz (RCC SW={}, PLLSRC={}, PLLMUL=x{}, HPRE=/{}; CR=0x{:08x} CFGR=0x{:08x})",
                new_core_hz,
                clocks.sw_source_name(),
                clocks.pll_source_name(),
                clocks.pll_mul_factor(),
                clocks.ahb_prescaler(),
                clocks.cr,
                clocks.cfgr
            );
        }
        let is_systick_reload = (event.peripheral == "SYST" && event.register == "RVR")
            || (event.peripheral == "STK" && event.register.contains("LOAD"));
        if is_systick_reload {
            info!(
                "clock.systick.reload = {} cycles (wrap every {} ticks)",
                event.value & 0x00FF_FFFF,
                (event.value & 0x00FF_FFFF) + 1
            );
        }
    }
    *mmio_cursor = mmio_events.len();

    if ENABLE_ST7789_CAPTURE {
        if let Some((width, height, pixels)) = st7789.take_latest_frame_argb() {
            if let Some(window) = display_gui.as_mut() {
                if let Err(err) = window.present(width, height, &pixels) {
                    info!("display.gui = disabled ({err})");
                    *display_gui = None;
                }
            }
        }

        if let Some(window) = display_gui.as_mut()
            && !window.tick()
        {
            info!("display.gui = closed");
            *display_gui = None;
        }
    }

    machine.clear_outputs();
    *serial_cursor = 0;
    *mmio_cursor = 0;
}

fn flush_partial_serial_lines(serial_lines: &BTreeMap<String, Vec<u8>>) {
    for (peripheral, bytes) in serial_lines {
        if bytes.is_empty() {
            continue;
        }
        let text = String::from_utf8_lossy(bytes);
        info!("serial.{peripheral} = {:?} (partial)", text);
    }
}

fn summarize_clock_writes(events: &[MmioWriteEvent]) -> Option<String> {
    let mut apb2enr = None;
    let mut cfgr = None;
    for event in events {
        if event.peripheral == "RCC" && event.register == "APB2ENR" {
            apb2enr = Some(event.value);
        }
        if event.peripheral == "RCC" && event.register == "CFGR" {
            cfgr = Some(event.value);
        }
    }
    if apb2enr.is_none() && cfgr.is_none() {
        return None;
    }
    let apb2_bits = apb2enr
        .map(|v| format!("0x{v:08x}"))
        .unwrap_or_else(|| "n/a".to_string());
    let cfgr_bits = cfgr
        .map(|v| format!("0x{v:08x}"))
        .unwrap_or_else(|| "n/a".to_string());
    Some(format!(
        "clock.rcc.summary = CFGR={cfgr_bits}, APB2ENR={apb2_bits}"
    ))
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
        if self.presents <= 3 || self.presents.is_multiple_of(30) {
            debug!("display.gui.present #{} ({}x{})", self.presents, width, height);
        }
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

struct GpioLogLimiter {
    interval: Duration,
    last_log: BTreeMap<(String, String), Instant>,
}

impl GpioLogLimiter {
    fn new(interval: Duration) -> Self {
        Self {
            interval,
            last_log: BTreeMap::new(),
        }
    }

    fn should_log(&mut self, peripheral: &str, register: &str) -> bool {
        let now = Instant::now();
        let key = (peripheral.to_string(), register.to_string());
        match self.last_log.get(&key) {
            Some(last) if now.duration_since(*last) < self.interval => false,
            _ => {
                self.last_log.insert(key, now);
                true
            }
        }
    }
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

    fn on_step(&mut self) -> u64 {
        if !self.enabled {
            return 0;
        }
        self.step_count = self.step_count.saturating_add(1);
        if let Some(nanos_per_step) = self.nanos_per_step {
            self.expected_elapsed_nanos =
                self.expected_elapsed_nanos.saturating_add(nanos_per_step);
        }
        if self.step_count % self.checkpoint_interval != 0 {
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

#[derive(Debug, Clone)]
struct RccClockModel {
    cr: u32,
    cfgr: u32,
    hsi_hz: u32,
    hse_hz: u32,
    core_clock_hz: u32,
}

impl RccClockModel {
    fn new(initial_core_hz: u32) -> Self {
        Self {
            cr: 0x0000_0083,
            cfgr: 0x0000_0000,
            hsi_hz: 8_000_000,
            hse_hz: 8_000_000,
            core_clock_hz: initial_core_hz.max(1),
        }
    }

    fn apply_mmio(&mut self, event: &MmioWriteEvent) -> Option<u32> {
        if event.peripheral != "RCC" {
            return None;
        }

        let reg = event.register.to_ascii_uppercase();
        match reg.as_str() {
            "CR" => {
                self.cr = merge_mmio_write(self.cr, event);
            }
            "CFGR" => {
                self.cfgr = merge_mmio_write(self.cfgr, event);
            }
            _ => return None,
        }

        let prev = self.core_clock_hz;
        self.core_clock_hz = self.compute_core_clock_hz().max(1);
        if self.core_clock_hz != prev {
            Some(self.core_clock_hz)
        } else {
            None
        }
    }

    fn compute_core_clock_hz(&self) -> u32 {
        let sysclk = match self.cfgr & 0x3 {
            0b00 => self.hsi_hz,
            0b01 => self.hse_hz,
            0b10 => self.pll_output_hz(),
            _ => self.hsi_hz,
        };
        sysclk / self.ahb_prescaler().max(1)
    }

    fn pll_output_hz(&self) -> u32 {
        let src_hz = if (self.cfgr >> 16) & 0x1 == 0 {
            self.hsi_hz / 2
        } else {
            let hse_div = if (self.cfgr >> 17) & 0x1 == 0 { 1 } else { 2 };
            self.hse_hz / hse_div
        };
        src_hz.saturating_mul(self.pll_mul_factor())
    }

    fn pll_mul_factor(&self) -> u32 {
        match (self.cfgr >> 18) & 0xF {
            0..=13 => ((self.cfgr >> 18) & 0xF) + 2,
            14 | 15 => 16,
            _ => 2,
        }
    }

    fn ahb_prescaler(&self) -> u32 {
        match (self.cfgr >> 4) & 0xF {
            0..=7 => 1,
            8 => 2,
            9 => 4,
            10 => 8,
            11 => 16,
            12 => 64,
            13 => 128,
            14 => 256,
            15 => 512,
            _ => 1,
        }
    }

    fn sw_source_name(&self) -> &'static str {
        match self.cfgr & 0x3 {
            0b00 => "HSI",
            0b01 => "HSE",
            0b10 => "PLL",
            _ => "UNKNOWN",
        }
    }

    fn pll_source_name(&self) -> &'static str {
        if (self.cfgr >> 16) & 0x1 == 0 {
            "HSI/2"
        } else if (self.cfgr >> 17) & 0x1 == 0 {
            "HSE"
        } else {
            "HSE/2"
        }
    }
}

fn is_usart_data_write(event: &MmioWriteEvent) -> bool {
    event.peripheral.starts_with("USART") && event.register.to_ascii_uppercase().starts_with("DR")
}

fn is_spi_data_write(event: &MmioWriteEvent) -> bool {
    event.peripheral.starts_with("SPI") && event.register.to_ascii_uppercase().starts_with("DR")
}

fn merge_mmio_write(current: u32, event: &MmioWriteEvent) -> u32 {
    match event.width {
        1 => {
            let byte_offset = ((event.addr as usize) & 0x3) as u32;
            let shift = byte_offset * 8;
            let mask = !(0xFFu32 << shift);
            (current & mask) | ((event.value & 0xFF) << shift)
        }
        2 => {
            let half_offset = (((event.addr as usize) & 0x2) >> 1) as u32;
            let shift = half_offset * 16;
            let mask = !(0xFFFFu32 << shift);
            (current & mask) | ((event.value & 0xFFFF) << shift)
        }
        _ => event.value,
    }
}

#[derive(Debug)]
struct St7789Capture {
    width: u16,
    height: u16,
    framebuffer: Vec<u16>,
    output_dir: String,
    dump_frames: bool,
    frame_id: u32,
    gpio_odr: BTreeMap<String, u32>,
    current_cmd: Option<u8>,
    params: Vec<u8>,
    window_x0: u16,
    window_x1: u16,
    window_y0: u16,
    window_y1: u16,
    cursor_x: u16,
    cursor_y: u16,
    pixel_hi: Option<u8>,
    active_spi_addr: Option<u64>,
    sample_count: u64,
    event_index: u64,
    recent_spi_event_index: Option<u64>,
    wiring: Option<St7789Wiring>,
    pin_stats: BTreeMap<(String, u8), PinStats>,
    pending_falling: BTreeMap<(String, u8), u64>,
    latest_frame_argb: Option<Vec<u32>>,
    preview_argb: Vec<u32>,
    preview_enabled: bool,
    ramwr_pixels_written: u32,
    preview_every_pixels: u32,
    preview_log_every_pixels: u32,
    frame_log_every: u32,
    ramwr_dropped_bytes: u64,
    spi_bytes_seen: u64,
    command_count: u64,
    ignored_data_bytes: u64,
    non_black_pixels: u32,
}

impl St7789Capture {
    fn new(width: u16, height: u16, output_dir: &str, dump_frames: bool, preview_enabled: bool) -> Self {
        if dump_frames {
            let _ = fs::create_dir_all(output_dir);
        }
        Self {
            width,
            height,
            framebuffer: vec![0; usize::from(width) * usize::from(height)],
            output_dir: output_dir.to_string(),
            dump_frames,
            frame_id: 0,
            gpio_odr: BTreeMap::new(),
            current_cmd: None,
            params: Vec::new(),
            window_x0: 0,
            window_x1: width.saturating_sub(1),
            window_y0: 0,
            window_y1: height.saturating_sub(1),
            cursor_x: 0,
            cursor_y: 0,
            pixel_hi: None,
            active_spi_addr: None,
            sample_count: 0,
            event_index: 0,
            recent_spi_event_index: None,
            wiring: None,
            pin_stats: BTreeMap::new(),
            pending_falling: BTreeMap::new(),
            latest_frame_argb: None,
            preview_argb: if preview_enabled {
                vec![0; usize::from(width) * usize::from(height)]
            } else {
                Vec::new()
            },
            preview_enabled,
            ramwr_pixels_written: 0,
            preview_every_pixels: 1024,
            preview_log_every_pixels: 65536,
            frame_log_every: 4,
            ramwr_dropped_bytes: 0,
            spi_bytes_seen: 0,
            command_count: 0,
            ignored_data_bytes: 0,
            non_black_pixels: 0,
        }
    }

    fn apply_mmio(&mut self, event: &MmioWriteEvent) -> Option<String> {
        self.event_index = self.event_index.saturating_add(1);
        if event.peripheral.starts_with("GPIO") && event.register.eq_ignore_ascii_case("ODR") {
            return self.on_gpio_write(event);
        }
        if !is_spi_data_write(event) {
            return None;
        }
        let spi_addr = event.addr;
        let byte = (event.value & 0xFF) as u8;
        if self.active_spi_addr.is_none() {
            self.active_spi_addr = Some(spi_addr);
            info!("display.st7789.spi_candidate = 0x{spi_addr:08x}");
        }
        if self.active_spi_addr != Some(spi_addr) {
            return None;
        }

        if self.wiring.is_none() {
            self.observe_spi_sample();
            self.try_detect_wiring();
            return None;
        }

        let wiring = self.wiring.as_ref()?;
        let cs_low = self.gpio_pin(&wiring.cs_port, wiring.cs_pin) == 0;
        if !cs_low {
            return None;
        }
        let dc_high = self.gpio_pin(&wiring.dc_port, wiring.dc_pin) == 1;
        self.spi_bytes_seen = self.spi_bytes_seen.saturating_add(1);
        if dc_high {
            self.on_data(byte)
        } else {
            self.on_command(byte);
            None
        }
    }

    fn on_gpio_write(&mut self, event: &MmioWriteEvent) -> Option<String> {
        let key = event.peripheral.clone();
        let old_odr = self.gpio_odr.get(&key).copied().unwrap_or(0);
        let new_odr = merge_mmio_write(old_odr, event);
        self.gpio_odr.insert(key.clone(), new_odr);

        let mut frame = None;
        for pin in 0..16 {
            let old = ((old_odr >> pin) & 1) as u8;
            let new = ((new_odr >> pin) & 1) as u8;
            if old == new {
                continue;
            }
            let stats = self.pin_stats.entry((key.clone(), pin)).or_default();
            stats.transitions = stats.transitions.saturating_add(1);
            if old == 1 && new == 0 {
                self.pending_falling
                    .insert((key.clone(), pin), self.event_index);
            } else if old == 0
                && new == 1
                && self
                    .recent_spi_event_index
                    .is_some_and(|idx| self.event_index.saturating_sub(idx) <= 8)
            {
                stats.rising_after_spi = stats.rising_after_spi.saturating_add(1);
            }
            if let Some(wiring) = self.wiring.as_ref()
                && wiring.cs_port == key
                && wiring.cs_pin == pin
                && old == 0
                && new == 1
                && self.current_cmd == Some(0x2C)
            {
                if self.ramwr_pixels_written > 0 {
                    frame = self.emit_frame().ok();
                } else {
                    debug!("display.st7789.frame.skip = cs-rise before pixel data");
                }
            }
        }
        frame
    }

    fn observe_spi_sample(&mut self) {
        self.sample_count = self.sample_count.saturating_add(1);
        self.recent_spi_event_index = Some(self.event_index);
        let ports: Vec<(String, u32)> =
            self.gpio_odr.iter().map(|(k, v)| (k.clone(), *v)).collect();
        for (port, odr) in ports {
            for pin in 0..16 {
                let value = ((odr >> pin) & 1) as u8;
                let entry = self.pin_stats.entry((port.clone(), pin)).or_default();
                if value == 0 {
                    entry.low_on_spi = entry.low_on_spi.saturating_add(1);
                } else {
                    entry.high_on_spi = entry.high_on_spi.saturating_add(1);
                }
                if value == 0
                    && self
                        .pending_falling
                        .remove(&(port.clone(), pin))
                        .is_some_and(|idx| self.event_index.saturating_sub(idx) <= 8)
                {
                    entry.falling_before_spi = entry.falling_before_spi.saturating_add(1);
                }
            }
        }
    }

    fn try_detect_wiring(&mut self) {
        if self.wiring.is_some() || self.sample_count < 8 {
            return;
        }

        let mut cs_candidate: Option<((String, u8), u64)> = None;
        for (pin, stats) in &self.pin_stats {
            let total = stats.low_on_spi + stats.high_on_spi;
            if total < 12 || stats.low_on_spi <= stats.high_on_spi || stats.falling_before_spi == 0
            {
                continue;
            }
            let score = stats.low_on_spi * 5
                + stats.falling_before_spi * 12
                + stats.rising_after_spi * 10
                + stats.transitions;
            if cs_candidate.as_ref().is_none_or(|(_, best)| score > *best) {
                cs_candidate = Some((pin.clone(), score));
            }
        }
        let Some((cs_pin, _)) = cs_candidate else {
            return;
        };

        let mut dc_candidate: Option<((String, u8), u64)> = None;
        for (pin, stats) in &self.pin_stats {
            if *pin == cs_pin {
                continue;
            }
            let total = stats.low_on_spi + stats.high_on_spi;
            if total < 12
                || stats.low_on_spi == 0
                || stats.high_on_spi == 0
                || stats.transitions < 2
            {
                continue;
            }
            let balance = stats.low_on_spi.min(stats.high_on_spi);
            let score = stats.transitions * 8 + balance + stats.rising_after_spi;
            if dc_candidate.as_ref().is_none_or(|(_, best)| score > *best) {
                dc_candidate = Some((pin.clone(), score));
            }
        }
        let Some((dc_pin, _)) = dc_candidate else {
            return;
        };

        let Some(spi_addr) = self.active_spi_addr else {
            return;
        };
        self.wiring = Some(St7789Wiring {
            spi: format!("0x{spi_addr:08x}"),
            cs_port: cs_pin.0,
            cs_pin: cs_pin.1,
            dc_port: dc_pin.0,
            dc_pin: dc_pin.1,
        });
        self.current_cmd = None;
        self.params.clear();
        self.pixel_hi = None;
        self.ramwr_pixels_written = 0;
        if let Some(wiring) = &self.wiring {
            info!(
                "display.st7789.wiring = spi={}, cs={}.{}, dc={}.{}",
                wiring.spi, wiring.cs_port, wiring.cs_pin, wiring.dc_port, wiring.dc_pin
            );
        }
    }

    fn gpio_pin(&self, peripheral: &str, pin: u8) -> u8 {
        let odr = self.gpio_odr.get(peripheral).copied().unwrap_or(0);
        ((odr >> pin) & 1) as u8
    }

    fn on_command(&mut self, cmd: u8) {
        self.current_cmd = Some(cmd);
        self.params.clear();
        self.pixel_hi = None;
        self.command_count = self.command_count.saturating_add(1);
        if matches!(cmd, 0x2A | 0x2B | 0x2C) {
            info!(
                "display.st7789.cmd = 0x{cmd:02x} (spi_bytes={}, cmd_count={})",
                self.spi_bytes_seen, self.command_count
            );
        } else if self.command_count <= 16 || self.command_count.is_multiple_of(128) {
            debug!(
                "display.st7789.cmd.raw = 0x{cmd:02x} (cmd_count={})",
                self.command_count
            );
        }
        if cmd == 0x2C {
            self.cursor_x = self.window_x0;
            self.cursor_y = self.window_y0;
            self.ramwr_pixels_written = 0;
        }
    }

    fn on_data(&mut self, byte: u8) -> Option<String> {
        match self.current_cmd {
            Some(0x2A) => {
                self.params.push(byte);
                if self.params.len() == 4 {
                    self.window_x0 = u16::from_be_bytes([self.params[0], self.params[1]]);
                    self.window_x1 = u16::from_be_bytes([self.params[2], self.params[3]]);
                    info!(
                        "display.st7789.window.x = {}..{}",
                        self.window_x0, self.window_x1
                    );
                }
                None
            }
            Some(0x2B) => {
                self.params.push(byte);
                if self.params.len() == 4 {
                    self.window_y0 = u16::from_be_bytes([self.params[0], self.params[1]]);
                    self.window_y1 = u16::from_be_bytes([self.params[2], self.params[3]]);
                    info!(
                        "display.st7789.window.y = {}..{}",
                        self.window_y0, self.window_y1
                    );
                }
                None
            }
            Some(0x2C) => self.on_ramwr_data(byte),
            _ => {
                self.ignored_data_bytes = self.ignored_data_bytes.saturating_add(1);
                if self.ignored_data_bytes <= 8 || self.ignored_data_bytes.is_multiple_of(512) {
                    debug!(
                        "display.st7789.data.ignored byte=0x{byte:02x} cmd={:?} ignored={}",
                        self.current_cmd, self.ignored_data_bytes
                    );
                }
                None
            }
        }
    }

    fn on_ramwr_data(&mut self, byte: u8) -> Option<String> {
        let window_w = u32::from(self.window_x1.saturating_sub(self.window_x0).saturating_add(1));
        let window_h = u32::from(self.window_y1.saturating_sub(self.window_y0).saturating_add(1));
        let window_pixels = window_w.saturating_mul(window_h);
        if window_pixels > 0 && self.ramwr_pixels_written >= window_pixels {
            self.ramwr_dropped_bytes = self.ramwr_dropped_bytes.saturating_add(1);
            if self.ramwr_dropped_bytes == 1 || self.ramwr_dropped_bytes.is_multiple_of(65_536) {
                warn!(
                    "display.st7789.ramwr.overflow window={}x{} pixels={} dropping_extra_bytes={}",
                    window_w, window_h, window_pixels, self.ramwr_dropped_bytes
                );
            }
            self.pixel_hi = None;
            return None;
        }

        if self.pixel_hi.is_none() {
            self.pixel_hi = Some(byte);
            return None;
        }

        let hi = self.pixel_hi.take().unwrap_or(0);
        let pixel = u16::from_be_bytes([hi, byte]);
        let x = self.cursor_x.min(self.width.saturating_sub(1));
        let y = self.cursor_y.min(self.height.saturating_sub(1));
        let idx = usize::from(y) * usize::from(self.width) + usize::from(x);
        if idx < self.framebuffer.len() {
            let old = self.framebuffer[idx];
            if old != pixel {
                self.framebuffer[idx] = pixel;
                if old == 0 && pixel != 0 {
                    self.non_black_pixels = self.non_black_pixels.saturating_add(1);
                }
                if self.preview_enabled {
                    self.preview_argb[idx] = if pixel == 0 {
                        0
                    } else {
                        let rgb = rgb565_to_rgb888(pixel);
                        ((u32::from(rgb[0])) << 16) | ((u32::from(rgb[1])) << 8) | u32::from(rgb[2])
                    };
                }
            }
        }
        self.ramwr_pixels_written = self.ramwr_pixels_written.saturating_add(1);
        if self.ramwr_pixels_written <= 4 || self.ramwr_pixels_written.is_multiple_of(4096) {
            debug!(
                "display.st7789.ramwr.pixel_count = {}",
                self.ramwr_pixels_written
            );
        }
        if self.preview_enabled
            && self
                .ramwr_pixels_written
                .is_multiple_of(self.preview_every_pixels)
        {
            self.latest_frame_argb = Some(self.preview_argb.clone());
            if self
                .ramwr_pixels_written
                .is_multiple_of(self.preview_log_every_pixels)
            {
                info!(
                    "display.st7789.preview pixels={} non_black={}",
                    self.ramwr_pixels_written, self.non_black_pixels
                );
            }
        }

        if self.cursor_x < self.window_x1 {
            self.cursor_x = self.cursor_x.saturating_add(1);
            return None;
        }
        self.cursor_x = self.window_x0;
        if self.cursor_y < self.window_y1 {
            self.cursor_y = self.cursor_y.saturating_add(1);
            return None;
        }
        self.cursor_y = self.window_y0;
        self.emit_frame().ok()
    }

    fn emit_frame(&mut self) -> Result<String, String> {
        let frame_seq = self.frame_id;
        self.frame_id = self.frame_id.wrapping_add(1);
        if self.preview_enabled {
            self.latest_frame_argb = Some(self.preview_argb.clone());
        }

        let path = format!("{}/frame_{:06}.ppm", self.output_dir, frame_seq);
        if self.dump_frames {
            let mut file = File::create(&path).map_err(|e| format!("create ppm failed: {e}"))?;
            let header = format!("P6\n{} {}\n255\n", self.width, self.height);
            file.write_all(header.as_bytes())
                .map_err(|e| format!("write ppm header failed: {e}"))?;
            for pixel in &self.framebuffer {
                let rgb = rgb565_to_rgb888(*pixel);
                file.write_all(&rgb)
                    .map_err(|e| format!("write ppm pixel failed: {e}"))?;
            }
        }
        if self.non_black_pixels == 0 {
            warn!(
                "display.st7789.frame.black frame={} pixels={} (likely missing RAMWR data)",
                frame_seq, self.ramwr_pixels_written
            );
        } else if frame_seq.is_multiple_of(self.frame_log_every) {
            info!(
                "display.st7789.frame.stats frame={} pixels={} non_black={}",
                frame_seq, self.ramwr_pixels_written, self.non_black_pixels
            );
        } else {
            debug!(
                "display.st7789.frame.stats frame={} pixels={} non_black={}",
                frame_seq, self.ramwr_pixels_written, self.non_black_pixels
            );
        }
        if self.dump_frames {
            Ok(path)
        } else {
            Ok(format!("frame_{frame_seq:06}"))
        }
    }

    fn take_latest_frame_argb(&mut self) -> Option<(u16, u16, Vec<u32>)> {
        self.latest_frame_argb
            .take()
            .map(|pixels| (self.width, self.height, pixels))
    }

}

#[derive(Debug, Clone)]
struct St7789Wiring {
    spi: String,
    cs_port: String,
    cs_pin: u8,
    dc_port: String,
    dc_pin: u8,
}

#[derive(Debug, Default, Clone, Copy)]
struct PinStats {
    low_on_spi: u64,
    high_on_spi: u64,
    transitions: u64,
    falling_before_spi: u64,
    rising_after_spi: u64,
}

fn rgb565_to_rgb888(pixel: u16) -> [u8; 3] {
    let r5 = ((pixel >> 11) & 0x1F) as u8;
    let g6 = ((pixel >> 5) & 0x3F) as u8;
    let b5 = (pixel & 0x1F) as u8;
    let r8 = (r5 << 3) | (r5 >> 2);
    let g8 = (g6 << 2) | (g6 >> 4);
    let b8 = (b5 << 3) | (b5 >> 2);
    [r8, g8, b8]
}

#[cfg(test)]
mod tests {
    use super::RccClockModel;
    use rsemu_core::MmioWriteEvent;

    fn mmio(peripheral: &str, register: &str, addr: u64, width: u8, value: u32) -> MmioWriteEvent {
        MmioWriteEvent {
            peripheral: peripheral.to_string(),
            register: register.to_string(),
            addr,
            width,
            value,
        }
    }

    #[test]
    fn rcc_cfgr_full_write_updates_core_clock() {
        let mut model = RccClockModel::new(8_000_000);
        // SW=PLL(0b10), HPRE=/1(0), PLLSRC=HSE(1), PLLMUL=x9(bits 0b0111).
        let cfgr = 0x001D_0002u32;
        let changed = model.apply_mmio(&mmio("RCC", "CFGR", 0x4002_1004, 4, cfgr));
        assert_eq!(changed, Some(72_000_000));
    }

    #[test]
    fn rcc_cfgr_byte_writes_update_core_clock() {
        let mut model = RccClockModel::new(8_000_000);
        // byte0: SW=PLL
        let _ = model.apply_mmio(&mmio("RCC", "CFGR", 0x4002_1004, 1, 0x02));
        // byte2: PLLSRC=HSE, PLLMUL=x9
        let changed = model.apply_mmio(&mmio("RCC", "CFGR", 0x4002_1006, 1, 0x1D));
        assert_eq!(changed, Some(72_000_000));
    }
}
