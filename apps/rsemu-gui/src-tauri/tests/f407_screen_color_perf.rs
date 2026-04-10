use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use base64::Engine;
use rsemu_core::{CpuCore, FirmwareLoader, GpioListener, Machine, MmioWriteEvent, SpiSlave};
use rsemu_core::cpu::armv7em::CortexM4;
use rsemu_peripherals::display::St7789;
use rsemu_peripherals::PinMapping;
use rsemu_targets::stm32::f407;

const SVD_F407: &str = include_str!("../svd/stm32f407.svd");

#[derive(Debug, Clone, Copy)]
struct Profile {
    name: &'static str,
    stream_interval_steps: u64,
    frame_interval_steps: u64,
    update_pacer_from_rcc: bool,
    encode_frame_to_base64: bool,
}

#[derive(Debug)]
struct PerfStats {
    profile: &'static str,
    steps: u64,
    elapsed: Duration,
    frames_pulled: u64,
}

impl PerfStats {
    fn steps_per_sec(&self) -> f64 {
        if self.elapsed.is_zero() {
            return 0.0;
        }
        self.steps as f64 / self.elapsed.as_secs_f64()
    }
}

fn find_screen_color_firmware() -> PathBuf {
    let base = Path::new(env!("CARGO_MANIFEST_DIR"));
    base.join("../../../stm32f4xx-hal/target/thumbv7em-none-eabihf/release/examples/screen-color")
}

fn find_rtthread_firmware() -> PathBuf {
    let base = Path::new(env!("CARGO_MANIFEST_DIR"));
    base.join("../../../firmware/rtthread.bin")
}

fn strip_ansi(input: &[u8]) -> String {
    let mut out = Vec::with_capacity(input.len());
    let mut i = 0usize;
    while i < input.len() {
        if input[i] == 0x1B {
            i += 1;
            if i < input.len() && input[i] == b'[' {
                i += 1;
                while i < input.len() {
                    let b = input[i];
                    i += 1;
                    if (0x40..=0x7E).contains(&b) {
                        break;
                    }
                }
                continue;
            }
            continue;
        }
        out.push(input[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn step_cpu_resilient<C: CpuCore>(machine: &mut Machine<C>) -> Result<u32, String> {
    let mut last_err = None;
    for batch in [1000usize, 10_000, 1_000, 100, 10, 1] {
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

#[derive(Debug, Clone)]
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
        std::thread::sleep(Duration::from_nanos(sleep_nanos_u64));
        0
    }
}

#[derive(Debug, Clone)]
struct RccClockModel {
    is_f407: bool,
    cr: u32,
    cfgr: u32,
    pllcfgr: u32,
    systick_load: u32,
    hsi_hz: u32,
    hse_hz: u32,
    core_clock_hz: u32,
}

impl RccClockModel {
    fn new(initial_core_hz: u32, is_f407: bool) -> Self {
        Self {
            is_f407,
            cr: 0x0000_0083,
            cfgr: 0x0000_0000,
            pllcfgr: 0x2400_3010,
            systick_load: 0,
            hsi_hz: if is_f407 { 16_000_000 } else { 8_000_000 },
            hse_hz: 8_000_000,
            core_clock_hz: initial_core_hz.max(1),
        }
    }

    fn apply_mmio(&mut self, event: &MmioWriteEvent) -> Option<u32> {
        if let Some(hz) = self.apply_systick_load_hint(event) {
            return Some(hz);
        }
        if event.peripheral != "RCC" {
            return None;
        }
        let reg = event.register.to_ascii_uppercase();
        match reg.as_str() {
            "CR" => self.cr = merge_mmio_write(self.cr, event),
            "CFGR" => self.cfgr = merge_mmio_write(self.cfgr, event),
            "PLLCFGR" if self.is_f407 => {
                self.pllcfgr = merge_mmio_write(self.pllcfgr, event)
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

    fn apply_systick_load_hint(&mut self, event: &MmioWriteEvent) -> Option<u32> {
        if !event.peripheral.eq_ignore_ascii_case("STK") {
            return None;
        }
        if !event.register.eq_ignore_ascii_case("LOAD") {
            return None;
        }
        self.systick_load = merge_mmio_write(self.systick_load, event) & 0x00FF_FFFF;
        let load = self.systick_load;
        if load < 50_000 {
            return None;
        }
        let inferred = load.saturating_add(1).saturating_mul(1_000);
        if !(24_000_000..=300_000_000).contains(&inferred) {
            return None;
        }
        if inferred <= self.core_clock_hz {
            return None;
        }
        self.core_clock_hz = inferred;
        Some(self.core_clock_hz)
    }

    fn compute_core_clock_hz(&self) -> u32 {
        if self.is_f407 {
            return self.compute_core_clock_hz_f407();
        }
        self.compute_core_clock_hz_f1()
    }

    fn compute_core_clock_hz_f1(&self) -> u32 {
        let sysclk = match self.cfgr & 0x3 {
            0b00 => self.hsi_hz,
            0b01 => self.hse_hz,
            0b10 => self.pll_output_hz_f1(),
            _ => self.hsi_hz,
        };
        sysclk / self.ahb_prescaler().max(1)
    }

    fn compute_core_clock_hz_f407(&self) -> u32 {
        let hsi_ready = (self.cr >> 1) & 1 == 1;
        let hse_ready = (self.cr >> 17) & 1 == 1;
        let pll_ready = (self.cr >> 25) & 1 == 1;
        let sysclk = match self.cfgr & 0x3 {
            0b00 => {
                if hsi_ready {
                    self.hsi_hz
                } else {
                    self.hse_hz
                }
            }
            0b01 => {
                if hse_ready {
                    self.hse_hz
                } else {
                    self.hsi_hz
                }
            }
            0b10 => {
                if pll_ready {
                    self.pll_output_hz_f407()
                } else if hsi_ready {
                    self.hsi_hz
                } else {
                    self.hse_hz
                }
            }
            _ => self.hsi_hz,
        };
        sysclk / self.ahb_prescaler().max(1)
    }

    fn pll_output_hz_f1(&self) -> u32 {
        let src_hz = if (self.cfgr >> 16) & 0x1 == 0 {
            self.hsi_hz / 2
        } else {
            let hse_div = if (self.cfgr >> 17) & 0x1 == 0 { 1 } else { 2 };
            self.hse_hz / hse_div
        };
        src_hz.saturating_mul(self.pll_mul_factor_f1())
    }

    fn pll_output_hz_f407(&self) -> u32 {
        let src_hz = if ((self.pllcfgr >> 22) & 0x1) == 1 {
            self.hse_hz as u64
        } else {
            self.hsi_hz as u64
        };
        let m = (self.pllcfgr & 0x3F).max(1) as u64;
        let n = ((self.pllcfgr >> 6) & 0x1FF).max(1) as u64;
        let p = match (self.pllcfgr >> 16) & 0x3 {
            0 => 2u64,
            1 => 4u64,
            2 => 6u64,
            _ => 8u64,
        };
        let vco_in = src_hz / m;
        let vco_out = vco_in.saturating_mul(n);
        (vco_out / p).clamp(1, u64::from(u32::MAX)) as u32
    }

    fn pll_mul_factor_f1(&self) -> u32 {
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

fn run_profile(firmware_path: &Path, profile: Profile, wall_time: Duration) -> Result<PerfStats, String> {
    let target = f407::load_target(Some(SVD_F407))?;
    let core_clock_hz = target.core_clock_hz;
    let systick_reload_divider = target.systick_reload_divider;

    // Find SPI peripheral name for routing
    let spi_peripheral = target.peripherals.iter()
        .find(|p| p.base_address == 0x4001_3000)
        .map(|p| p.name.to_ascii_uppercase())
        .unwrap_or_else(|| "SPI1".to_string());

    let mut machine = Machine::new(CortexM4::new(), target);

    let fw = FirmwareLoader::load_file(firmware_path, 0x0800_0000)?;
    machine.load_firmware(&fw)?;
    machine.reset_cpu()?;

    let mut st = St7789::new(
        240,
        240,
        0x4001_3000,
        PinMapping { port: "A".to_string(), pin: 4 },
        PinMapping { port: "A".to_string(), pin: 3 },
        Some(PinMapping { port: "A".to_string(), pin: 2 }),
        String::new(),
        false,
        true,
    );
    let cs_port = 'A';
    let cs_pin: u8 = 4;
    let dc_port = 'A';
    let dc_pin: u8 = 3;

    let base_emu_cycles_per_step = ((u64::from(systick_reload_divider) * 3) / 5).max(1);
    let emu_cycles_per_step = base_emu_cycles_per_step as u32;
    let mut pacer = RealtimePacer::new(core_clock_hz, emu_cycles_per_step);
    pacer.set_enabled(true);
    let mut rcc_model = RccClockModel::new(core_clock_hz, true);

    let mut steps = 0u64;
    let mut mmio_cursor = 0usize;
    let mut steps_since_stream = 0u64;
    let mut steps_since_frame = 0u64;
    let mut frames_pulled = 0u64;

    let start = Instant::now();
    while start.elapsed() < wall_time {
        let ran = step_cpu_resilient(&mut machine)?;
        steps = steps.saturating_add(ran as u64);
        steps_since_stream = steps_since_stream.saturating_add(ran as u64);
        steps_since_frame = steps_since_frame.saturating_add(ran as u64);

        let extra_ticks = pacer.on_steps(ran);
        if extra_ticks > 0 {
            machine.advance_systick_ticks(extra_ticks);
        }

        if steps_since_stream >= profile.stream_interval_steps {
            let mmio = machine.mmio_writes();
            for event in &mmio[mmio_cursor..] {
                // Route SPI DR → SpiSlave::transfer()
                if event.peripheral.eq_ignore_ascii_case(&spi_peripheral)
                    && event.register.eq_ignore_ascii_case("DR")
                {
                    let byte = (event.value & 0xFF) as u8;
                    let _miso = SpiSlave::transfer(&mut st, byte);
                }

                // Route GPIO → CS/DC pins
                if event.peripheral.starts_with("GPIO") {
                    let port = event.peripheral.chars().nth(4).unwrap_or('A');
                    if event.register.eq_ignore_ascii_case("ODR") {
                        if port == cs_port {
                            let high = ((event.value >> cs_pin) & 1) != 0;
                            st.chip_select(!high);
                        }
                        if port == dc_port {
                            let high = ((event.value >> dc_pin) & 1) != 0;
                            GpioListener::pin_changed(&mut st, port, dc_pin, high);
                        }
                    } else if event.register.eq_ignore_ascii_case("BSRR") {
                        let set_mask = event.value & 0xFFFF;
                        let rst_mask = (event.value >> 16) & 0xFFFF;
                        let cs_set = (set_mask >> cs_pin) & 1 != 0;
                        let cs_rst = (rst_mask >> cs_pin) & 1 != 0;
                        if cs_set { st.chip_select(false); }
                        if cs_rst { st.chip_select(true); }
                        let dc_set = (set_mask >> dc_pin) & 1 != 0;
                        let dc_rst = (rst_mask >> dc_pin) & 1 != 0;
                        if dc_set { GpioListener::pin_changed(&mut st, port, dc_pin, true); }
                        if dc_rst { GpioListener::pin_changed(&mut st, port, dc_pin, false); }
                    }
                }

                if profile.update_pacer_from_rcc {
                    if let Some(new_core_hz) = rcc_model.apply_mmio(event) {
                        pacer.set_core_clock_hz(new_core_hz);
                    }
                }
            }
            if steps_since_frame >= profile.frame_interval_steps {
                if let Some(frame) = st.latest_frame() {
                    frames_pulled = frames_pulled.saturating_add(1);
                    if profile.encode_frame_to_base64 {
                        let raw: Vec<u8> = frame.iter().flat_map(|&px| px.to_le_bytes()).collect();
                        let _encoded = base64::engine::general_purpose::STANDARD.encode(raw);
                    }
                }
                steps_since_frame = 0;
            }

            machine.clear_outputs();
            mmio_cursor = 0;
            steps_since_stream = 0;
        }
    }

    Ok(PerfStats {
        profile: profile.name,
        steps,
        elapsed: start.elapsed(),
        frames_pulled,
    })
}

#[derive(Debug, Clone)]
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

    fn tick<C: CpuCore>(&mut self, machine: &mut Machine<C>, core_hz: u32, divider: u32) {
        let now = Instant::now();
        let elapsed = now.saturating_duration_since(self.last).min(Duration::from_millis(50));
        self.last = now;
        let ticks_per_sec = (core_hz.max(1) as f64) / (divider.max(1) as f64);
        self.carry += elapsed.as_secs_f64() * ticks_per_sec;
        let whole = self.carry.floor() as u64;
        self.carry -= whole as f64;
        if whole > 0 {
            machine.advance_systick_ticks(whole);
        }
    }
}

#[test]
#[ignore = "manual perf comparison; requires built stm32f4xx-hal screen-color firmware"]
fn compare_f407_screen_color_gui_vs_cli_profiles() {
    let firmware = find_screen_color_firmware();
    assert!(
        firmware.exists(),
        "firmware not found at {}. build with: `cd stm32f4xx-hal && cargo build --release --features stm32f407 --example screen-color`",
        firmware.display()
    );

    let gui_profile = Profile {
        name: "gui-current",
        stream_interval_steps: 4096,
        frame_interval_steps: 100_000,
        update_pacer_from_rcc: false,
        encode_frame_to_base64: true,
    };
    let cli_like_profile = Profile {
        name: "cli-like",
        stream_interval_steps: 64,
        frame_interval_steps: 100_000,
        update_pacer_from_rcc: true,
        encode_frame_to_base64: false,
    };

    let wall_time = Duration::from_secs(3);
    let gui = run_profile(&firmware, gui_profile, wall_time).expect("gui profile failed");
    let cli = run_profile(&firmware, cli_like_profile, wall_time).expect("cli-like profile failed");

    eprintln!(
        "[perf] {}: steps={}, elapsed={:?}, steps/s={:.2}, frames={}",
        gui.profile,
        gui.steps,
        gui.elapsed,
        gui.steps_per_sec(),
        gui.frames_pulled
    );
    eprintln!(
        "[perf] {}: steps={}, elapsed={:?}, steps/s={:.2}, frames={}",
        cli.profile,
        cli.steps,
        cli.elapsed,
        cli.steps_per_sec(),
        cli.frames_pulled
    );
    eprintln!(
        "[perf] ratio cli/gui = {:.2}x",
        cli.steps_per_sec() / gui.steps_per_sec().max(1.0)
    );

    assert!(gui.steps > 0, "gui profile should make progress");
    assert!(cli.steps > 0, "cli-like profile should make progress");
}

#[test]
#[ignore = "manual integration/perf test using real firmware timing"]
fn f407_screen_color_realtime_unlocked_render_emits_frames() {
    let firmware = find_screen_color_firmware();
    assert!(
        firmware.exists(),
        "screen-color firmware not found: {}",
        firmware.display()
    );

    let target = f407::load_target(Some(SVD_F407)).expect("load f407 target failed");
    let core_clock_hz = target.core_clock_hz;
    let _systick_reload_divider = target.systick_reload_divider;
    let mut machine = Machine::new(CortexM4::new(), target);
    machine.set_step_driven_systick(false);
    machine.set_step_driven_timers(false);
    machine.set_systick_reload_scaling(false);
    let fw = FirmwareLoader::load_file(&firmware, 0x0800_0000).expect("load screen-color firmware failed");
    machine.load_firmware(&fw).expect("map firmware failed");
    machine.reset_cpu().expect("reset cpu failed");

    let mut st = St7789::new(
        240, 240, 0x4001_3000,
        PinMapping { port: "A".to_string(), pin: 4 },
        PinMapping { port: "A".to_string(), pin: 3 },
        Some(PinMapping { port: "A".to_string(), pin: 2 }),
        String::new(), false, true,
    );

    // Find SPI peripheral name for routing
    let spi_peripheral = "SPI1".to_string();
    let cs_port = 'A';
    let cs_pin: u8 = 4;
    let dc_port = 'A';
    let dc_pin: u8 = 3;

    let mut clocks = RccClockModel::new(core_clock_hz, true);
    let mut systick = WallClockSystickDriver::new();
    let mut mmio_cursor = 0usize;
    let mut steps = 0u64;
    let mut frames = 0u64;
    let mut next_stream = Instant::now();
    let stream_interval = Duration::from_millis(2);
    let start = Instant::now();

    while start.elapsed() < Duration::from_secs(8) {
        let ran = step_cpu_resilient(&mut machine).expect("step cpu failed");
        steps = steps.saturating_add(ran as u64);
        systick.tick(&mut machine, clocks.core_clock_hz, 1);

        if Instant::now() >= next_stream {
            let mmio = machine.mmio_writes();
            for event in &mmio[mmio_cursor..] {
                // Route SPI DR → SpiSlave::transfer()
                if event.peripheral.eq_ignore_ascii_case(&spi_peripheral)
                    && event.register.eq_ignore_ascii_case("DR")
                {
                    let byte = (event.value & 0xFF) as u8;
                    let _miso = SpiSlave::transfer(&mut st, byte);
                }

                // Route GPIO → CS/DC pins
                if event.peripheral.starts_with("GPIO") {
                    let port = event.peripheral.chars().nth(4).unwrap_or('A');
                    if event.register.eq_ignore_ascii_case("ODR") {
                        if port == cs_port {
                            let high = ((event.value >> cs_pin) & 1) != 0;
                            st.chip_select(!high);
                        }
                        if port == dc_port {
                            let high = ((event.value >> dc_pin) & 1) != 0;
                            GpioListener::pin_changed(&mut st, port, dc_pin, high);
                        }
                    } else if event.register.eq_ignore_ascii_case("BSRR") {
                        let set_mask = event.value & 0xFFFF;
                        let rst_mask = (event.value >> 16) & 0xFFFF;
                        let cs_set = (set_mask >> cs_pin) & 1 != 0;
                        let cs_rst = (rst_mask >> cs_pin) & 1 != 0;
                        if cs_set { st.chip_select(false); }
                        if cs_rst { st.chip_select(true); }
                        let dc_set = (set_mask >> dc_pin) & 1 != 0;
                        let dc_rst = (rst_mask >> dc_pin) & 1 != 0;
                        if dc_set { GpioListener::pin_changed(&mut st, port, dc_pin, true); }
                        if dc_rst { GpioListener::pin_changed(&mut st, port, dc_pin, false); }
                    }
                }

                let _ = clocks.apply_mmio(event);
            }
            if st.latest_frame().is_some() {
                frames = frames.saturating_add(1);
            }
            machine.clear_outputs();
            mmio_cursor = 0;
            next_stream = Instant::now() + stream_interval;
        }
    }

    let stats = PerfStats {
        profile: "realtime-unlocked-render",
        steps,
        elapsed: start.elapsed(),
        frames_pulled: frames,
    };

    eprintln!(
        "[screen] steps={}, elapsed={:?}, steps/s={:.2}, frames={}",
        stats.steps,
        stats.elapsed,
        stats.steps_per_sec(),
        stats.frames_pulled
    );

    assert!(stats.steps > 0, "simulation should make progress");
    assert!(stats.steps_per_sec() > 200_000.0, "steps/s should be far above paced mode");
    assert!(
        stats.frames_pulled > 0,
        "expected at least one display frame from screen-color in realtime mode"
    );
}

#[test]
#[ignore = "manual integration/perf test using rtthread delay semantics"]
fn f407_rtthread_led_delay_stays_effective_in_realtime() {
    let firmware = find_rtthread_firmware();
    assert!(
        firmware.exists(),
        "rtthread firmware not found: {}",
        firmware.display()
    );

    let target = f407::load_target(Some(SVD_F407)).expect("load f407 target failed");
    let core_clock_hz = target.core_clock_hz;
    let systick_reload_divider = target.systick_reload_divider;
    let mut machine = Machine::new(CortexM4::new(), target);

    let fw = FirmwareLoader::load_file(&firmware, 0x0800_0000).expect("load rtthread firmware failed");
    machine.load_firmware(&fw).expect("map rtthread firmware failed");
    machine.reset_cpu().expect("reset cpu failed");

    machine.set_step_driven_systick(false);
    machine.set_step_driven_timers(false);
    machine.set_systick_reload_scaling(false);
    let mut systick = WallClockSystickDriver::new();
    let mut clocks = RccClockModel::new(core_clock_hz, true);

    let mut mmio_cursor = 0usize;
    let mut line_buf: Vec<u8> = Vec::new();
    let mut led_on_times: Vec<Instant> = Vec::new();
    let mut last_count: Option<u64> = None;
    let mut total_steps = 0u64;
    let pace_start = Instant::now();
    let start = Instant::now();

    while start.elapsed() < Duration::from_secs(25) && led_on_times.len() < 8 {
        let ran = step_cpu_resilient(&mut machine).expect("step cpu failed");
        total_steps = total_steps.saturating_add(ran as u64);
        let target_steps_per_sec =
            (core_clock_hz.max(1) as f64) / (systick_reload_divider.max(1) as f64);
        let expected_secs = total_steps as f64 / target_steps_per_sec.max(1.0);
        let elapsed_secs = pace_start.elapsed().as_secs_f64();
        if expected_secs > elapsed_secs {
            std::thread::sleep(Duration::from_secs_f64(expected_secs - elapsed_secs));
        }
        systick.tick(&mut machine, clocks.core_clock_hz, 1);

        let serial = machine.serial_output();
        for ev in serial {
            line_buf.push(ev.byte);
            if ev.byte == b'\n' {
                let line = strip_ansi(&line_buf);
                if line.contains("led on") {
                    if let Some(idx) = line.find("count:") {
                        let tail = line[idx + "count:".len()..].trim();
                        let digits: String = tail
                            .chars()
                            .take_while(|c| c.is_ascii_digit())
                            .collect();
                        if let Ok(n) = digits.parse::<u64>() {
                            let is_new = last_count.is_none_or(|prev| n > prev);
                            if is_new {
                                led_on_times.push(Instant::now());
                                last_count = Some(n);
                            }
                        }
                    }
                }
                line_buf.clear();
            }
        }

        let mmio = machine.mmio_writes();
        for event in &mmio[mmio_cursor..] {
            let _ = clocks.apply_mmio(event);
        }
        machine.clear_outputs();
        mmio_cursor = 0;
    }
    assert!(
        led_on_times.len() >= 4,
        "expected at least 4 'led on' logs within timeout, got {}",
        led_on_times.len()
    );

    let mut intervals_ms: Vec<u64> = Vec::new();
    for w in led_on_times.windows(2) {
        intervals_ms.push(w[1].duration_since(w[0]).as_millis() as u64);
    }
    eprintln!("[rtthread] led-on log intervals ms: {:?}", intervals_ms);

    let stable_intervals = if intervals_ms.len() > 4 {
        intervals_ms.split_off(1)
    } else {
        intervals_ms.clone()
    };
    eprintln!("[rtthread] stable log intervals ms: {:?}", stable_intervals);

    // In realtime-unlocked mode, mdelay should remain close to real wall-clock behavior.
    let avg = stable_intervals.iter().sum::<u64>() as f64 / stable_intervals.len() as f64;
    assert!(
        (350.0..=700.0).contains(&avg),
        "expected average LED toggle interval near 500ms, got {:.1}ms",
        avg
    );
}
