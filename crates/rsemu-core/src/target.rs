use std::collections::BTreeMap;
use std::ops::Range;

use crate::cpu::{ArchitectureId, CpuCore};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryRegion {
    pub name: String,
    pub range: Range<u64>,
    pub kind: MemoryRegionKind,
}

impl MemoryRegion {
    pub fn contains(&self, addr: u64) -> bool {
        self.range.contains(&addr)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MemoryRegionKind {
    Flash,
    Ram,
    Peripheral,
    System,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegisterSpec {
    pub name: String,
    pub address: u64,
    pub width_bits: u32,
    pub reset_value: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeripheralSpec {
    pub name: String,
    pub base_address: u64,
    pub registers: Vec<RegisterSpec>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CpuType {
    CortexM3,
    CortexM4,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetSpec {
    pub name: String,
    pub architecture: ArchitectureId,
    pub cpu_type: CpuType,
    pub vector_table_base: u64,
    pub core_clock_hz: u32,
    pub hsi_hz: u32,
    pub has_pllcfgr: bool,
    pub systick_reload_divider: u32,
    pub memory_map: Vec<MemoryRegion>,
    pub peripherals: Vec<PeripheralSpec>,
}

impl CpuType {
    /// Construct a boxed CPU core for this type.
    /// Adding a new CPU variant only requires updating this method + the enum.
    pub fn make_cpu(&self) -> Result<Box<dyn CpuCore>, String> {
        match self {
            CpuType::CortexM3 => {
                let cpu = crate::cpu::armv7m::CortexM3::new()?;
                Ok(Box::new(cpu))
            }
            CpuType::CortexM4 => {
                let cpu = crate::cpu::armv7em::CortexM4::new()?;
                Ok(Box::new(cpu))
            }
        }
    }
}

#[derive(Debug, Default)]
pub struct RegisterValueSet {
    values: BTreeMap<u64, u64>,
}

impl RegisterValueSet {
    pub fn new(spec: &[PeripheralSpec]) -> Self {
        let mut values = BTreeMap::new();
        for peripheral in spec {
            for register in &peripheral.registers {
                values.insert(register.address, register.reset_value);
            }
        }
        Self { values }
    }

    pub fn read8(&self, addr: u64) -> Option<u8> {
        self.values.get(&addr).map(|value| *value as u8)
    }

    pub fn write8(&mut self, addr: u64, value: u8) -> bool {
        match self.values.get_mut(&addr) {
            Some(slot) => {
                *slot = value as u64;
                true
            }
            None => false,
        }
    }
}
