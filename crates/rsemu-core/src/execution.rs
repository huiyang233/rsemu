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

/// Map a GPIO pin to its STM32 ADC channel number.
///
/// Standard mapping (same for F1/F4):
/// - PA0-PA7 → CH 0-7
/// - PB0-PB1 → CH 8-9
/// - PC0-PC5 → CH 10-15
/// - Others  → Err
pub fn pin_to_adc_channel(port: char, pin: u8) -> Result<u8, String> {
    match port.to_ascii_uppercase() {
        'A' if pin <= 7 => Ok(pin),
        'B' if pin <= 1 => Ok(8 + pin),
        'C' if pin <= 5 => Ok(10 + pin),
        _ => Err(format!(
            "GPIO{}{} has no ADC channel mapping",
            port.to_ascii_uppercase(),
            pin
        )),
    }
}

/// Look up the SR and DR register addresses for a named ADC peripheral.
///
/// Returns `(sr_addr, dr_addr)` from the SVD peripheral list.
/// The lookup is case-insensitive on the peripheral name.
pub fn adc_sr_dr_addrs(
    peripheral_name: &str,
    peripherals: &[PeripheralSpec],
) -> Result<(u64, u64), String> {
    let adc = peripherals
        .iter()
        .find(|p| p.name.eq_ignore_ascii_case(peripheral_name))
        .ok_or_else(|| format!("ADC peripheral '{}' not found in target spec", peripheral_name))?;

    let sr = adc
        .registers
        .iter()
        .find(|r| r.name == "SR")
        .ok_or_else(|| format!("SR register not found in {}", adc.name))?;
    let dr = adc
        .registers
        .iter()
        .find(|r| r.name == "DR")
        .ok_or_else(|| format!("DR register not found in {}", adc.name))?;

    Ok((sr.address, dr.address))
}

/// Look up the SR, DR, SQR3, and CR2 register addresses for a named ADC peripheral.
///
/// Returns `(sr_addr, dr_addr, sqr3_addr, cr2_addr)`.
/// SQR3 bits [4:0] contain the first conversion channel (SQ1).
pub fn adc_regs_addrs(
    peripheral_name: &str,
    peripherals: &[PeripheralSpec],
) -> Result<(u64, u64, u64, u64), String> {
    let adc = peripherals
        .iter()
        .find(|p| p.name.eq_ignore_ascii_case(peripheral_name))
        .ok_or_else(|| format!("ADC peripheral '{}' not found in target spec", peripheral_name))?;

    let sr = adc
        .registers
        .iter()
        .find(|r| r.name == "SR")
        .ok_or_else(|| format!("SR register not found in {}", adc.name))?;
    let dr = adc
        .registers
        .iter()
        .find(|r| r.name == "DR")
        .ok_or_else(|| format!("DR register not found in {}", adc.name))?;
    let sqr3 = adc
        .registers
        .iter()
        .find(|r| r.name == "SQR3")
        .ok_or_else(|| format!("SQR3 register not found in {}", adc.name))?;
    let cr2 = adc
        .registers
        .iter()
        .find(|r| r.name == "CR2")
        .ok_or_else(|| format!("CR2 register not found in {}", adc.name))?;

    Ok((sr.address, dr.address, sqr3.address, cr2.address))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::target::RegisterSpec;

    /// Helper: create a PeripheralSpec named "ADC1" with SR at `base` and DR at `base + 0x4C`.
    fn make_adc(base: u64) -> PeripheralSpec {
        PeripheralSpec {
            name: "ADC1".to_string(),
            base_address: base,
            registers: vec![
                RegisterSpec {
                    name: "SR".to_string(),
                    address: base,
                    width_bits: 32,
                    reset_value: 0,
                },
                RegisterSpec {
                    name: "DR".to_string(),
                    address: base + 0x4C,
                    width_bits: 32,
                    reset_value: 0,
                },
            ],
        }
    }

    #[test]
    fn test_pin_to_adc_channel_pa() {
        assert_eq!(pin_to_adc_channel('A', 0).unwrap(), 0);
        assert_eq!(pin_to_adc_channel('A', 7).unwrap(), 7);
    }

    #[test]
    fn test_pin_to_adc_channel_pb() {
        assert_eq!(pin_to_adc_channel('B', 0).unwrap(), 8);
        assert_eq!(pin_to_adc_channel('B', 1).unwrap(), 9);
    }

    #[test]
    fn test_pin_to_adc_channel_pc() {
        assert_eq!(pin_to_adc_channel('C', 0).unwrap(), 10);
        assert_eq!(pin_to_adc_channel('C', 5).unwrap(), 15);
    }

    #[test]
    fn test_pin_to_adc_channel_invalid() {
        assert!(pin_to_adc_channel('D', 0).is_err());
        assert!(pin_to_adc_channel('B', 2).is_err());
    }

    #[test]
    fn test_adc_sr_dr_addrs() {
        let base: u64 = 0x4001_2400;
        let peripherals = vec![make_adc(base)];
        let (sr, dr) = adc_sr_dr_addrs("ADC1", &peripherals).unwrap();
        assert_eq!(sr, base);
        assert_eq!(dr, base + 0x4C);
    }

    #[test]
    fn test_adc_sr_dr_addrs_case_insensitive() {
        let base: u64 = 0x4001_2400;
        let peripherals = vec![make_adc(base)];
        let (sr, dr) = adc_sr_dr_addrs("adc1", &peripherals).unwrap();
        assert_eq!(sr, base);
        assert_eq!(dr, base + 0x4C);
    }

    #[test]
    fn test_adc_sr_dr_addrs_not_found() {
        let peripherals = vec![make_adc(0x4001_2400)];
        assert!(adc_sr_dr_addrs("ADC3", &peripherals).is_err());
    }
}
