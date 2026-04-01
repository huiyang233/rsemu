#[path = "armv7m_external.rs"]
mod armv7m_external;
#[path = "armv7m_legacy.rs"]
mod armv7m_legacy;

use crate::bus::SystemBus;
use crate::cpu::{CpuArchitecture, CpuCore};

pub use armv7m_legacy::{
    ArmV7MArchitecture, FLASH_ALIAS_BASE, PERIPH_BB_ALIAS_BASE, PERIPH_BB_ALIAS_END,
    PERIPH_BB_BASE,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArmCoreBackend {
    Legacy,
    External,
}

impl ArmCoreBackend {
    pub fn from_env() -> Self {
        match std::env::var("RSEMU_ARM_CORE") {
            Ok(value) => match value.to_ascii_lowercase().as_str() {
                "external" | "unicorn" | "third_party" => Self::External,
                _ => Self::Legacy,
            },
            Err(_) => Self::Legacy,
        }
    }
}

#[derive(Debug)]
pub enum CortexM3 {
    Legacy(armv7m_legacy::CortexM3),
    External(armv7m_external::CortexM3External),
}

impl Default for CortexM3 {
    fn default() -> Self {
        Self::new()
    }
}

impl CortexM3 {
    pub fn new() -> Self {
        Self::with_backend(ArmCoreBackend::from_env())
    }

    pub fn with_backend(backend: ArmCoreBackend) -> Self {
        match backend {
            ArmCoreBackend::Legacy => Self::Legacy(armv7m_legacy::CortexM3::new()),
            ArmCoreBackend::External => {
                Self::External(armv7m_external::CortexM3External::new())
            }
        }
    }

    pub fn backend(&self) -> ArmCoreBackend {
        match self {
            Self::Legacy(_) => ArmCoreBackend::Legacy,
            Self::External(_) => ArmCoreBackend::External,
        }
    }

    pub fn registers(&self) -> &[u32; 16] {
        match self {
            Self::Legacy(cpu) => cpu.registers(),
            Self::External(cpu) => cpu.registers(),
        }
    }

    pub fn thumb_state(&self) -> bool {
        match self {
            Self::Legacy(cpu) => cpu.thumb_state(),
            Self::External(cpu) => cpu.thumb_state(),
        }
    }
}

impl CpuCore for CortexM3 {
    fn step(&mut self, bus: &mut dyn SystemBus) -> Result<(), String> {
        match self {
            Self::Legacy(cpu) => cpu.step(bus),
            Self::External(cpu) => cpu.step(bus),
        }
    }

    fn reset(&mut self, bus: &mut dyn SystemBus, vector_table_base: u64) -> Result<(), String> {
        match self {
            Self::Legacy(cpu) => cpu.reset(bus, vector_table_base),
            Self::External(cpu) => cpu.reset(bus, vector_table_base),
        }
    }

    fn architecture(&self) -> &dyn CpuArchitecture {
        match self {
            Self::Legacy(cpu) => cpu.architecture(),
            Self::External(cpu) => cpu.architecture(),
        }
    }

    fn program_counter(&self) -> u64 {
        match self {
            Self::Legacy(cpu) => cpu.program_counter(),
            Self::External(cpu) => cpu.program_counter(),
        }
    }

    fn stack_pointer(&self) -> u64 {
        match self {
            Self::Legacy(cpu) => cpu.stack_pointer(),
            Self::External(cpu) => cpu.stack_pointer(),
        }
    }

    fn in_exception(&self) -> bool {
        match self {
            Self::Legacy(cpu) => cpu.in_exception(),
            Self::External(cpu) => cpu.in_exception(),
        }
    }

    fn enter_exception(
        &mut self,
        bus: &mut dyn SystemBus,
        vector_table_base: u64,
        exception_number: u16,
    ) -> Result<bool, String> {
        match self {
            Self::Legacy(cpu) => cpu.enter_exception(bus, vector_table_base, exception_number),
            Self::External(cpu) => cpu.enter_exception(bus, vector_table_base, exception_number),
        }
    }
}
