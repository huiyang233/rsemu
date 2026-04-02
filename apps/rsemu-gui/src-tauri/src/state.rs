use rsemu_peripherals::PinMapping;
use serde::Deserialize;
use std::sync::mpsc::Sender;
use std::thread::JoinHandle;

/// Messages sent from Tauri commands to the running emulator thread.
#[derive(Debug)]
pub enum ControlMsg {
    Stop,
    InjectGpio { port: String, pin: u8, high: bool },
    SendUart { peripheral: String, bytes: Vec<u8> },
}

/// Shared Tauri-managed state for the emulator.
#[derive(Default)]
pub struct SimState {
    pub control_tx: Option<Sender<ControlMsg>>,
    pub thread: Option<JoinHandle<()>>,
}

/// Configuration sent from the frontend when starting a simulation.
#[derive(Debug, Clone, Deserialize)]
pub struct SimConfig {
    pub board: String,
    pub firmware_path: String,
    pub peripherals: Vec<GuiPeripheralConfig>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
pub enum GuiPeripheralConfig {
    #[serde(rename = "st7789")]
    St7789 {
        width: u16,
        height: u16,
        spi_base: u64,
        cs: PinMapping,
        dc: PinMapping,
        res: Option<PinMapping>,
    },
    #[serde(rename = "led")]
    Led {
        id: String,
        pin: PinMapping,
        #[serde(default = "default_true")]
        active_low: bool,
    },
    #[serde(rename = "uart")]
    Uart {
        usart: String,
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
