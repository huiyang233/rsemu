pub mod armv7m;
pub mod armv7em;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArchitectureId {
    ArmV7M,
    Unknown(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ArchitectureMemoryConfig {
    pub flash_alias_base: Option<u64>,
    pub periph_bitband_base: Option<u64>,
    pub periph_bitband_alias_start: Option<u64>,
    pub periph_bitband_alias_end: Option<u64>,
}

pub trait CpuArchitecture {
    fn id(&self) -> ArchitectureId;
    fn name(&self) -> &'static str;
    fn reset_vector_bits(&self) -> u8;
    fn memory_config(&self) -> ArchitectureMemoryConfig {
        ArchitectureMemoryConfig::default()
    }
}

pub trait CpuCore {
    fn step(&mut self, bus: &mut dyn crate::bus::SystemBus, max_steps: usize) -> Result<u32, String>;
    fn reset(
        &mut self,
        bus: &mut dyn crate::bus::SystemBus,
        vector_table_base: u64,
        memory_regions: &[crate::target::MemoryRegion],
    ) -> Result<(), String>;
    fn architecture(&self) -> &dyn CpuArchitecture;
    fn program_counter(&self) -> u64;
    fn stack_pointer(&self) -> u64;
    fn in_exception(&self) -> bool {
        false
    }
    fn enter_exception(
        &mut self,
        _bus: &mut dyn crate::bus::SystemBus,
        _vector_table_base: u64,
        _exception_number: u16,
    ) -> Result<bool, String> {
        Ok(false)
    }
}
