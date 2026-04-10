use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use rsemu_core::cpu::armv7em::CortexM4;
use rsemu_core::{CpuCore, FirmwareLoader, I2cSlave, Machine};
use rsemu_peripherals::ssd1306::Ssd1306I2c;
use rsemu_targets::stm32::f407;

const SVD_F407: &str = include_str!("../svd/stm32f407.svd");

fn find_ssd1306_firmware() -> PathBuf {
    let base = Path::new(env!("CARGO_MANIFEST_DIR"));
    base.join("../../../stm32f4xx-hal/target/thumbv7em-none-eabihf/release/examples/ssd1306-i2c-f407")
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

/// Minimal I2C master state machine for test routing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum I2cPhase {
    Idle,
    AwaitAddress,
    MasterWrite,
}

#[test]
#[ignore = "manual integration test; requires built stm32f4xx-hal ssd1306-i2c-f407 firmware"]
fn f407_ssd1306_i2c_emits_non_empty_frames() {
    let firmware = find_ssd1306_firmware();
    assert!(
        firmware.exists(),
        "ssd1306 firmware not found: {}. build with: `cd stm32f4xx-hal && cargo build --release --features stm32f407 --example ssd1306-i2c-f407`",
        firmware.display()
    );

    let target = f407::load_target(Some(SVD_F407)).expect("load f407 target failed");
    let mut machine = Machine::new(CortexM4::new(), target);
    let fw = FirmwareLoader::load_file(&firmware, 0x0800_0000).expect("load firmware failed");
    machine.load_firmware(&fw).expect("map firmware failed");
    machine.reset_cpu().expect("reset cpu failed");

    let mut oled = Ssd1306I2c::new(128, 64, "I2C1".into(), 0x3c);
    let mut i2c_phase = I2cPhase::Idle;
    let mut frames = 0u64;
    let mut max_lit_pixels = 0usize;
    let deadline = Instant::now() + Duration::from_secs(4);

    while Instant::now() < deadline {
        let _ = step_cpu_resilient(&mut machine).expect("cpu step failed");

        let mmio_events = machine.mmio_writes();
        for event in mmio_events {
            if !event.peripheral.eq_ignore_ascii_case("I2C1") {
                continue;
            }
            if event.register.eq_ignore_ascii_case("CR1") {
                if (event.value & (1 << 8)) != 0 {
                    i2c_phase = I2cPhase::AwaitAddress;
                }
                if (event.value & (1 << 9)) != 0 {
                    if i2c_phase != I2cPhase::Idle {
                        oled.stop();
                    }
                    i2c_phase = I2cPhase::Idle;
                }
            } else if event.register.eq_ignore_ascii_case("DR") {
                let byte = (event.value & 0xFF) as u8;
                match i2c_phase {
                    I2cPhase::AwaitAddress => {
                        let addr7 = byte >> 1;
                        let read = (byte & 1) != 0;
                        if oled.address(addr7, read) && !read {
                            i2c_phase = I2cPhase::MasterWrite;
                        } else {
                            i2c_phase = I2cPhase::Idle;
                        }
                    }
                    I2cPhase::MasterWrite => {
                        oled.write_byte(byte);
                    }
                    _ => {}
                }
            }
        }

        if let Some(frame) = oled.latest_frame() {
            frames = frames.saturating_add(1);
            let lit = frame.iter().filter(|&&px| px != 0xFF00_0000).count();
            if lit > max_lit_pixels {
                max_lit_pixels = lit;
            }
            if frames >= 2 && max_lit_pixels > 32 {
                break;
            }
        }

        machine.clear_outputs();
    }

    eprintln!(
        "[ssd1306] frames={}, max_lit_pixels={}",
        frames, max_lit_pixels
    );

    assert!(frames > 0, "expected at least one SSD1306 frame");
    assert!(
        max_lit_pixels > 0,
        "expected SSD1306 frame to contain lit pixels"
    );
}
