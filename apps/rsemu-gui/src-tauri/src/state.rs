use rsemu_peripherals::PeripheralConfig;
use serde::Deserialize;
use std::sync::mpsc::Sender;
use std::thread::JoinHandle;

/// Messages sent from Tauri commands to the running emulator thread.
#[derive(Debug)]
pub enum ControlMsg {
    Stop,
    InjectGpio { port: String, pin: u8, high: bool },
    SendUart { peripheral: String, bytes: Vec<u8> },
    InjectAdc { peripheral: String, channel: u8, value: u16 },
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
    pub peripherals: Vec<PeripheralConfig>,
}
