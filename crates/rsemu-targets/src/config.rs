use rsemu_core::{ArchitectureId, CpuType, MemoryRegion, MemoryRegionKind, TargetSpec};
use rsemu_svd::parse_svd;
use serde::Deserialize;

#[derive(Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub enum CpuTypeConfig {
    CortexM3,
    CortexM4,
}

#[derive(Deserialize, Clone)]
#[serde(rename_all = "lowercase")]
pub enum MemoryKindConfig {
    Flash,
    Ram,
    Peripheral,
    System,
}

#[derive(Deserialize, Clone)]
pub struct TargetConfig {
    pub id: String,
    pub name: String,
    pub cpu_type: CpuTypeConfig,
    pub hsi_hz: u32,
    pub core_clock_hz: u32,
    pub has_pllcfgr: bool,
    pub vector_table_base: String,
    pub systick_reload_divider: u32,
    pub svd_file: String,
    pub memory_map: Vec<MemoryRegionConfig>,
    #[serde(default)]
    pub rcc_patches: Vec<RccPatch>,
}

#[derive(Deserialize, Clone)]
pub struct MemoryRegionConfig {
    pub name: String,
    pub start: String,
    pub end: String,
    pub kind: MemoryKindConfig,
}

#[derive(Deserialize, Clone)]
pub struct RccPatch {
    pub register: String,
    pub or_value: String,
}

impl TargetConfig {
    pub fn build_target_spec(&self, svd_xml: &str) -> Result<TargetSpec, String> {
        let mut peripherals = parse_svd(svd_xml)
            .map_err(|e| format!("SVD parse error: {e}"))?
            .peripherals;

        // Apply rcc_patches
        for patch in &self.rcc_patches {
            let or_val = u64::from(
                parse_hex(&patch.or_value)
                    .map_err(|e| format!("rcc_patches: invalid or_value '{}': {e}", patch.or_value))?,
            );
            for p in peripherals.iter_mut() {
                if p.name == "RCC" {
                    for r in p.registers.iter_mut() {
                        if r.name == patch.register {
                            r.reset_value |= or_val;
                        }
                    }
                }
            }
        }

        let memory_map = self
            .memory_map
            .iter()
            .map(|m| {
                Ok(MemoryRegion {
                    name: m.name.clone(),
                    range: parse_hex(&m.start)?..parse_hex(&m.end)?,
                    kind: match m.kind {
                        MemoryKindConfig::Flash => MemoryRegionKind::Flash,
                        MemoryKindConfig::Ram => MemoryRegionKind::Ram,
                        MemoryKindConfig::Peripheral => MemoryRegionKind::Peripheral,
                        MemoryKindConfig::System => MemoryRegionKind::System,
                    },
                })
            })
            .collect::<Result<Vec<_>, String>>()?;

        let cpu_type = match self.cpu_type {
            CpuTypeConfig::CortexM3 => CpuType::CortexM3,
            CpuTypeConfig::CortexM4 => CpuType::CortexM4,
        };

        let vector_table_base = parse_hex(&self.vector_table_base)
            .map_err(|e| format!("invalid vector_table_base '{}': {e}", self.vector_table_base))?;

        Ok(TargetSpec {
            name: self.name.clone(),
            architecture: ArchitectureId::ArmV7M,
            cpu_type,
            vector_table_base,
            core_clock_hz: self.core_clock_hz,
            hsi_hz: self.hsi_hz,
            has_pllcfgr: self.has_pllcfgr,
            systick_reload_divider: self.systick_reload_divider,
            memory_map,
            peripherals,
        })
    }
}

fn parse_hex(s: &str) -> Result<u64, String> {
    u64::from_str_radix(s.trim().trim_start_matches("0x").trim_start_matches("0X"), 16)
        .map_err(|e| format!("invalid hex '{}': {e}", s))
}
