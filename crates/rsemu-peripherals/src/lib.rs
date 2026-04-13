use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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
    #[serde(rename = "potentiometer")]
    Potentiometer {
        id: String,
        pin: PinMapping,
        #[serde(default = "default_adc1")]
        adc: String,
        /// "horizontal" or "vertical"
        #[serde(default = "default_horizontal")]
        orientation: String,
    },
    #[serde(rename = "joystick")]
    Joystick {
        id: String,
        pin_x: PinMapping,
        pin_y: PinMapping,
        #[serde(default = "default_adc1")]
        adc_x: String,
        #[serde(default = "default_adc1")]
        adc_y: String,
    },
    /// Escape hatch: allows board.toml to declare peripheral types not
    /// yet known as first-class variants. The `params` map is passed
    /// through verbatim — the app layer can use it to instantiate custom
    /// peripheral implementations without recompiling this crate.
    #[serde(rename = "custom")]
    Custom {
        /// User-defined type name (e.g. "my_sensor").
        type_name: String,
        /// Arbitrary key-value parameters from TOML/JSON.
        #[serde(default)]
        params: HashMap<String, String>,
    },
}

fn default_true() -> bool {
    true
}

fn default_ssd1306_addr() -> u8 {
    0x3c
}

fn default_adc1() -> String {
    "ADC1".to_string()
}

fn default_horizontal() -> String {
    "horizontal".to_string()
}
