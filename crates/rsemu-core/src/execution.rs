use crate::cpu::CpuCore;
use crate::target::PeripheralSpec;
use crate::Machine;

/// Adaptive CPU step batch controller with retry and auto-growth.
///
/// On MAP/EXCEPTION errors, retries with smaller batches.
/// On successful full-batch runs, gradually grows batch size.
pub struct StepBatchController {
    current: usize,
    grow_success: u8,
}

impl StepBatchController {
    pub fn new(initial: usize) -> Self {
        Self {
            current: normalize_batch(initial),
            grow_success: 0,
        }
    }

    pub fn step_cpu<C: CpuCore>(&mut self, machine: &mut Machine<C>) -> Result<u32, String> {
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
    if v >= 10_000 { 10_000 }
    else if v >= 5_000 { 5_000 }
    else if v >= 1_000 { 1_000 }
    else if v >= 100 { 100 }
    else if v >= 10 { 10 }
    else { 1 }
}

fn next_smaller_batch(v: usize) -> usize {
    if v > 5_000 { 5_000 }
    else if v > 1_000 { 1_000 }
    else if v > 100 { 100 }
    else if v > 10 { 10 }
    else { 1 }
}

fn next_larger_batch(v: usize) -> usize {
    if v < 10 { 10 }
    else if v < 100 { 100 }
    else if v < 1_000 { 1_000 }
    else if v < 5_000 { 5_000 }
    else { 10_000 }
}

/// Extract the port letter from a GPIO peripheral name (e.g. "GPIOA" → 'A').
/// Returns None if the name doesn't start with "GPIO".
pub fn gpio_port_letter(name: &str) -> Option<char> {
    if name.starts_with("GPIO") {
        name.chars().nth(4)
    } else {
        None
    }
}

/// Compute the GPIO IDR register address for a given port.
/// Derives the GPIO base address and IDR register offset from the SVD peripheral list.
pub fn gpio_idr_addr(port: char, peripherals: &[PeripheralSpec]) -> Result<u64, String> {
    let idx = port.to_ascii_uppercase() as u64 - b'A' as u64;
    let gpioa = peripherals
        .iter()
        .find(|p| p.name == "GPIOA")
        .ok_or_else(|| "GPIOA not found in target spec".to_string())?;
    let idr = gpioa
        .registers
        .iter()
        .find(|r| r.name == "IDR")
        .ok_or_else(|| "IDR register not found in GPIOA".to_string())?;
    let idr_offset = idr.address - gpioa.base_address;
    Ok(gpioa.base_address + idx * 0x400 + idr_offset)
}
