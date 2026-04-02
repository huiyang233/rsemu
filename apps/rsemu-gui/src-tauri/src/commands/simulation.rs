use std::sync::{mpsc, Mutex};
use tauri::{AppHandle, State};

use crate::emulator::run_emulator;
use crate::state::{ControlMsg, SimConfig, SimState};

#[tauri::command]
pub async fn start_simulation(
    app: AppHandle,
    state: State<'_, Mutex<SimState>>,
    config: SimConfig,
) -> Result<(), String> {
    let mut guard = state.lock().map_err(|e| e.to_string())?;

    // Stop any previously running simulation
    if let Some(tx) = guard.control_tx.take() {
        let _ = tx.send(ControlMsg::Stop);
    }
    if let Some(handle) = guard.thread.take() {
        let _ = handle.join();
    }

    let (tx, rx) = mpsc::channel::<ControlMsg>();
    guard.control_tx = Some(tx);

    let app_clone = app.clone();
    let handle = std::thread::spawn(move || {
        run_emulator(config, app_clone, rx);
    });
    guard.thread = Some(handle);

    Ok(())
}

#[tauri::command]
pub fn stop_simulation(state: State<'_, Mutex<SimState>>) -> Result<(), String> {
    let mut guard = state.lock().map_err(|e| e.to_string())?;
    if let Some(tx) = guard.control_tx.take() {
        let _ = tx.send(ControlMsg::Stop);
    }
    Ok(())
}

#[tauri::command]
pub fn inject_gpio(
    state: State<'_, Mutex<SimState>>,
    port: String,
    pin: u8,
    high: bool,
) -> Result<(), String> {
    let guard = state.lock().map_err(|e| e.to_string())?;
    if let Some(tx) = &guard.control_tx {
        tx.send(ControlMsg::InjectGpio { port, pin, high }).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub fn send_uart(
    state: State<'_, Mutex<SimState>>,
    peripheral: String,
    bytes: Vec<u8>,
) -> Result<(), String> {
    let guard = state.lock().map_err(|e| e.to_string())?;
    if let Some(tx) = &guard.control_tx {
        tx.send(ControlMsg::SendUart { peripheral, bytes }).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub async fn open_firmware_dialog(app: AppHandle) -> Option<String> {
    use tauri_plugin_dialog::DialogExt;
    let path = app
        .dialog()
        .file()
        .add_filter("Firmware binary", &["bin"])
        .blocking_pick_file();
    path.map(|p| p.to_string())
}
