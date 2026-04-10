use rsemu_core::{MachineBusInterface, MmioWriteEvent};
use serde::{Deserialize, Serialize};
use std::fmt::Debug;

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
    #[serde(rename = "st7789")]
    St7789 {
        width: u16,
        height: u16,
        spi_base: u64,
        cs: PinMapping,
        dc: PinMapping,
        res: Option<PinMapping>,
        #[serde(default)]
        dump_frames: bool,
        #[serde(default = "default_output_dir")]
        output_dir: String,
    },
    #[serde(rename = "led")]
    Led {
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
}

fn default_output_dir() -> String {
    "/tmp/rsemu-frames".to_string()
}

fn default_true() -> bool {
    true
}

fn default_ssd1306_addr() -> u8 {
    0x3c
}

pub trait Peripheral: Send + Debug {
    fn name(&self) -> &str;
    fn on_mmio_write(&mut self, machine: &dyn MachineBusInterface, event: &MmioWriteEvent);
    fn update(&mut self, _machine: &dyn MachineBusInterface) {}
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any;
}
