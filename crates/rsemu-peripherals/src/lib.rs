use serde::{Deserialize, Serialize};

pub mod display;
pub mod led;
pub mod ssd1306;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PinMapping {
    pub port: String,
    pub pin: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum PeripheralConfig {
    #[serde(rename = "st7789_spi", alias = "st7789")]
    St7789Spi {
        width: u16,
        height: u16,
        spi_base: u64,
        cs: PinMapping,
        dc: PinMapping,
        res: Option<PinMapping>,
    },
    #[serde(rename = "st7789_fsmc")]
    St7789Fsmc {
        width: u16,
        height: u16,
        fsmc_base: u64,
    },
    #[serde(rename = "led")]
    Led {
        #[serde(default)]
        id: Option<String>,
        pin: PinMapping,
        #[serde(default = "default_true")]
        active_low: bool,
    },
    #[serde(rename = "ssd1306_i2c")]
    Ssd1306I2c {
        width: u16,
        height: u16,
        i2c: String,
        #[serde(default = "default_ssd1306_addr")]
        address: u8,
    },
    #[serde(rename = "uart")]
    Uart {
        usart: String,
        #[serde(default)]
        tx: Option<PinMapping>,
        #[serde(default)]
        rx: Option<PinMapping>,
    },
    #[serde(rename = "button")]
    Button {
        id: String,
        pin: PinMapping,
    },
}

fn default_true() -> bool {
    true
}

fn default_ssd1306_addr() -> u8 {
    0x3c
}
