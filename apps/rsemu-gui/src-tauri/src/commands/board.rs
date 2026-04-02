use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct BoardInfo {
    pub id: String,
    pub name: String,
    pub description: String,
    pub flash_kb: u32,
    pub ram_kb: u32,
    pub gpio_ports: Vec<String>,
    pub spi_peripherals: Vec<BusPeripheralInfo>,
    pub usart_peripherals: Vec<BusPeripheralInfo>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BusPeripheralInfo {
    pub name: String,
    pub base: u64,
}

#[tauri::command]
pub fn get_boards() -> Vec<BoardInfo> {
    vec![
        BoardInfo {
            id: "stm32f103".into(),
            name: "STM32F103 (Cortex-M3)".into(),
            description: "64 KB Flash, 20 KB SRAM, 8 MHz".into(),
            flash_kb: 64,
            ram_kb: 20,
            gpio_ports: vec!["A", "B", "C", "D", "E"].into_iter().map(String::from).collect(),
            spi_peripherals: vec![
                BusPeripheralInfo { name: "SPI1".into(), base: 0x4001_3000 },
                BusPeripheralInfo { name: "SPI2".into(), base: 0x4000_3800 },
            ],
            usart_peripherals: vec![
                BusPeripheralInfo { name: "USART1".into(), base: 0x4001_3800 },
                BusPeripheralInfo { name: "USART2".into(), base: 0x4000_4400 },
                BusPeripheralInfo { name: "USART3".into(), base: 0x4000_4800 },
            ],
        },
        BoardInfo {
            id: "stm32f407".into(),
            name: "STM32F407 (Cortex-M4)".into(),
            description: "1 MB Flash, 128 KB SRAM + 64 KB CCM, 16 MHz".into(),
            flash_kb: 1024,
            ram_kb: 192,
            gpio_ports: vec!["A", "B", "C", "D", "E", "F", "G", "H", "I"]
                .into_iter().map(String::from).collect(),
            spi_peripherals: vec![
                BusPeripheralInfo { name: "SPI1".into(), base: 0x4001_3000 },
                BusPeripheralInfo { name: "SPI2".into(), base: 0x4000_3800 },
                BusPeripheralInfo { name: "SPI3".into(), base: 0x4000_3C00 },
            ],
            usart_peripherals: vec![
                BusPeripheralInfo { name: "USART1".into(), base: 0x4001_1000 },
                BusPeripheralInfo { name: "USART2".into(), base: 0x4000_4400 },
                BusPeripheralInfo { name: "USART3".into(), base: 0x4000_4800 },
                BusPeripheralInfo { name: "UART4".into(),  base: 0x4000_4C00 },
                BusPeripheralInfo { name: "UART5".into(),  base: 0x4000_5000 },
                BusPeripheralInfo { name: "USART6".into(), base: 0x4001_1400 },
            ],
        },
    ]
}
