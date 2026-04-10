use rsemu_core::target::{MemoryRegionKind, TargetSpec};
use rsemu_targets::TargetRegistry;
use serde::Serialize;
use tauri::State;

#[derive(Debug, Clone, Serialize)]
pub struct BoardInfo {
    pub id: String,
    pub name: String,
    pub description: String,
    pub flash_kb: u32,
    pub ram_kb: u32,
    pub gpio_ports: Vec<String>,
    pub spi_peripherals: Vec<BusPeripheralInfo>,
    pub i2c_peripherals: Vec<BusPeripheralInfo>,
    pub usart_peripherals: Vec<BusPeripheralInfo>,
    pub fsmc_peripherals: Vec<BusPeripheralInfo>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BusPeripheralInfo {
    pub name: String,
    pub base: u64,
}

#[tauri::command]
pub fn get_boards(registry: State<'_, TargetRegistry>) -> Result<Vec<BoardInfo>, String> {
    let mut boards = Vec::new();
    for id in registry.list_ids() {
        let config = registry.get_config(id).unwrap();
        let target = registry.load(id)?;
        boards.push(board_info_from_target(id, &config.name, &target));
    }
    Ok(boards)
}

fn board_info_from_target(id: &str, name: &str, target: &TargetSpec) -> BoardInfo {
    let mut spi = Vec::new();
    let mut i2c = Vec::new();
    let mut usart = Vec::new();
    let mut gpio_ports = Vec::new();
    let mut fsmc = Vec::new();

    for p in &target.peripherals {
        let n = p.name.to_ascii_uppercase();
        if n.starts_with("SPI") {
            spi.push(BusPeripheralInfo { name: p.name.clone(), base: p.base_address });
        } else if n.starts_with("I2C") {
            i2c.push(BusPeripheralInfo { name: p.name.clone(), base: p.base_address });
        } else if n.starts_with("USART") || n.starts_with("UART") {
            usart.push(BusPeripheralInfo { name: p.name.clone(), base: p.base_address });
        } else if n.starts_with("GPIO") {
            // GPIOA → "A", GPIOB → "B", ...
            if let Some(letter) = p.name.chars().nth(4) {
                if letter.is_ascii_alphabetic() {
                    gpio_ports.push(letter.to_string());
                }
            }
        } else if n == "FSMC" {
            fsmc.push(BusPeripheralInfo { name: p.name.clone(), base: p.base_address });
        }
    }

    // Sort for consistent ordering
    spi.sort_by_key(|p| p.base);
    i2c.sort_by_key(|p| p.base);
    usart.sort_by_key(|p| p.base);
    gpio_ports.sort();

    let flash_kb = kb_from_memory_map(target, MemoryRegionKind::Flash);
    let ram_kb = kb_from_memory_map(target, MemoryRegionKind::Ram);

    let description = format!(
        "{} KB Flash, {} KB SRAM, {} MHz",
        flash_kb,
        ram_kb,
        target.core_clock_hz / 1_000_000,
    );

    BoardInfo {
        id: id.to_string(),
        name: name.to_string(),
        description,
        flash_kb,
        ram_kb,
        gpio_ports,
        spi_peripherals: spi,
        i2c_peripherals: i2c,
        usart_peripherals: usart,
        fsmc_peripherals: fsmc,
    }
}

fn kb_from_memory_map(target: &TargetSpec, kind: MemoryRegionKind) -> u32 {
    target
        .memory_map
        .iter()
        .filter(|r| r.kind == kind)
        .map(|r| (r.range.end - r.range.start) / 1024)
        .sum::<u64>() as u32
}
