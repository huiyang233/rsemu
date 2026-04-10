use crate::MmioWriteEvent;

/// RCC clock model — tracks STM32F1/F4 clock tree from MMIO events.
///
/// Extracted from CLI/GUI duplicated implementations.
#[derive(Debug, Clone)]
pub struct RccClockModel {
    is_f407: bool,
    cr: u32,
    cfgr: u32,
    pllcfgr: u32,
    systick_load: u32,
    hsi_hz: u32,
    hse_hz: u32,
    pub core_clock_hz: u32,
}

impl RccClockModel {
    pub fn new(initial_core_hz: u32, is_f407: bool) -> Self {
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

    pub fn apply_mmio(&mut self, event: &MmioWriteEvent) -> Option<u32> {
        if let Some(hz) = self.apply_systick_load_hint(event) {
            return Some(hz);
        }
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
            "PLLCFGR" if self.is_f407 => {
                self.pllcfgr = merge_mmio_write(self.pllcfgr, event);
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
                if hsi_ready { self.hsi_hz } else { self.hse_hz }
            }
            0b01 => {
                if hse_ready { self.hse_hz } else { self.hsi_hz }
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
