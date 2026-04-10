use std::sync::{mpsc, Mutex};
use tauri::{AppHandle, State};

use crate::emulator::run_emulator;
use crate::state::{ControlMsg, SimConfig, SimState};
use rsemu_targets::TargetRegistry;

#[tauri::command]
pub async fn start_simulation(
    app: AppHandle,
    state: State<'_, Mutex<SimState>>,
    registry: State<'_, TargetRegistry>,
    config: SimConfig,
) -> Result<(), String> {
    let target = registry.load(&config.board)?;

    // Take out old handles before acquiring lock for new thread setup,
    // so that join() is called outside the lock and cannot deadlock.
    let (old_tx, old_handle) = {
        let mut guard = state.lock().map_err(|e| e.to_string())?;
        (guard.control_tx.take(), guard.thread.take())
    };
    if let Some(tx) = old_tx {
        let _ = tx.send(ControlMsg::Stop);
    }
    if let Some(handle) = old_handle {
        let _ = handle.join();
    }

    let mut guard = state.lock().map_err(|e| e.to_string())?;
    let (tx, rx) = mpsc::channel::<ControlMsg>();
    guard.control_tx = Some(tx);

    let app_clone = app.clone();
    let handle = std::thread::spawn(move || {
        run_emulator(target, config, app_clone, rx);
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
