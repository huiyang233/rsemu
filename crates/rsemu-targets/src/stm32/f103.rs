use rsemu_core::{ArchitectureId, MemoryRegion, MemoryRegionKind, TargetSpec};
use rsemu_svd::parse_svd;

pub fn load_target(svd_xml: Option<&str>) -> Result<TargetSpec, String> {
    let xml = svd_xml.ok_or_else(|| "missing SVD: pass --svd path/to/stm32f103.svd".to_string())?;
    let peripherals = parse_svd(xml)?.peripherals;

    Ok(TargetSpec {
        name: "STM32F103".to_string(),
        architecture: ArchitectureId::ArmV7M,
        vector_table_base: 0x0800_0000,
        core_clock_hz: 8_000_000,
        systick_reload_divider: 1024,
        memory_map: vec![
            MemoryRegion {
                name: "flash".to_string(),
                range: 0x0800_0000..0x0810_0000,
                kind: MemoryRegionKind::Flash,
            },
            MemoryRegion {
                name: "sram".to_string(),
                range: 0x2000_0000..0x2001_0000,
                kind: MemoryRegionKind::Ram,
            },
            MemoryRegion {
                name: "peripherals".to_string(),
                range: 0x4000_0000..0x5000_0000,
                kind: MemoryRegionKind::Peripheral,
            },
            MemoryRegion {
                name: "system".to_string(),
                range: 0xE000_0000..0xE010_0000,
                kind: MemoryRegionKind::System,
            },
        ],
        peripherals,
    })
}
