use rsemu_core::{ArchitectureId, MemoryRegion, MemoryRegionKind, TargetSpec};
use rsemu_svd::parse_svd;

pub fn load_target(svd_xml: Option<&str>) -> Result<TargetSpec, String> {
    let xml = svd_xml.ok_or_else(|| "missing SVD: pass --svd path/to/stm32f407.svd".to_string())?;
    let mut peripherals = parse_svd(xml)?.peripherals;

    // Patch RCC reset values to have RDY bits set
    for p in peripherals.iter_mut() {
        if p.name == "RCC" {
            for r in p.registers.iter_mut() {
                if r.name == "CR" {
                    r.reset_value |= 0x02020002; // HSIRDY, HSERDY, PLLRDY
                }
                if r.name == "CSR" {
                    r.reset_value |= 0x00000002; // LSIRDY
                }
                if r.name == "BDCR" {
                    r.reset_value |= 0x00000002; // LSERDY
                }
            }
        }
    }

    Ok(TargetSpec {
        name: "STM32F407".to_string(),
        architecture: ArchitectureId::ArmV7M,
        vector_table_base: 0x0800_0000,
        core_clock_hz: 16_000_000,
        systick_reload_divider: 1024,
        memory_map: vec![
            MemoryRegion {
                name: "flash".to_string(),
                range: 0x0800_0000..0x0810_0000,
                kind: MemoryRegionKind::Flash,
            },
            MemoryRegion {
                name: "sram".to_string(),
                range: 0x2000_0000..0x2002_0000,
                kind: MemoryRegionKind::Ram,
            },
            MemoryRegion {
                name: "ccm".to_string(),
                range: 0x1000_0000..0x1001_0000,
                kind: MemoryRegionKind::Ram,
            },
            MemoryRegion {
                name: "peripherals".to_string(),
                range: 0x4000_0000..0x6000_0000,
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
