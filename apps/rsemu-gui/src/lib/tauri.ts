import { invoke } from "@tauri-apps/api/core";
import { listen, UnlistenFn } from "@tauri-apps/api/event";
import type { BoardInfo } from "../types/board";
import type { PeripheralConfig } from "../types/peripheral";
import type { AppPreferences } from "../types/project";
import type {
  SimStatusPayload,
  LedChangedPayload,
  DisplayFramePayload,
  UartOutputPayload,
} from "../types/simulation";

// ── Commands ─────────────────────────────────────────────────────────────────

export async function getBoards(): Promise<BoardInfo[]> {
  return invoke<BoardInfo[]>("get_boards");
}

export interface SimConfig {
  board: string;
  firmware_path: string;
  peripherals: PeripheralConfig[];
}

export async function startSimulation(config: SimConfig): Promise<void> {
  return invoke("start_simulation", { config });
}

export async function stopSimulation(): Promise<void> {
  return invoke("stop_simulation");
}

export async function injectGpio(port: string, pin: number, high: boolean): Promise<void> {
  return invoke("inject_gpio", { port, pin, high });
}

export async function injectAdc(
  peripheral: string,
  channel: number,
  value: number,
): Promise<void> {
  return invoke("inject_adc", { peripheral, channel, value });
}

/**
 * Compute the STM32 ADC channel number for a GPIO pin.
 * PA0-PA7 → CH0-7, PB0-PB1 → CH8-9, PC0-PC5 → CH10-15.
 * Returns 0 for any unrecognised pin (fails silently — emulator will log the error).
 */
export function pinToAdcChannel(pin: { port: string; pin: number }): number {
  const port = pin.port.toUpperCase();
  if (port === "A" && pin.pin < 8) return pin.pin;
  if (port === "B" && pin.pin < 2) return 8 + pin.pin;
  if (port === "C" && pin.pin < 6) return 10 + pin.pin;
  return 0;
}

export async function sendUart(peripheral: string, bytes: number[]): Promise<void> {
  return invoke("send_uart", { peripheral, bytes });
}

export async function openFirmwareDialog(): Promise<string | null> {
  return invoke<string | null>("open_firmware_dialog");
}

export async function getAppPreferences(): Promise<AppPreferences> {
  return invoke<AppPreferences>("get_app_preferences");
}

export async function rememberProject(path: string): Promise<AppPreferences> {
  return invoke<AppPreferences>("remember_project", { path });
}

export async function forgetProject(path: string): Promise<AppPreferences> {
  return invoke<AppPreferences>("forget_project", { path });
}

export async function clearLastProject(): Promise<AppPreferences> {
  return invoke<AppPreferences>("clear_last_project");
}

export async function openProjectDialog(): Promise<string | null> {
  return invoke<string | null>("open_project_dialog");
}

export async function saveProjectDialog(): Promise<string | null> {
  return invoke<string | null>("save_project_dialog");
}

export async function readProjectFile(path: string): Promise<string> {
  return invoke<string>("read_project_file", { path });
}

export async function writeProjectFile(path: string, content: string): Promise<void> {
  return invoke("write_project_file", { path, content });
}

// ── Module-level event bus ────────────────────────────────────────────────────
// Listeners are registered once at module load time (not inside React effects),
// so React StrictMode double-invocation cannot create duplicate subscriptions.

type Handler<T> = (payload: T) => void;

class EventBus<T> {
  private handlers: Set<Handler<T>> = new Set();
  private unlisten: UnlistenFn | null = null;

  constructor(private eventName: string) {
    // Register the single Tauri listener immediately when module loads
    listen<T>(eventName, (e) => {
      this.handlers.forEach((h) => h(e.payload));
    }).then((fn) => {
      this.unlisten = fn;
    });
  }

  subscribe(handler: Handler<T>): () => void {
    this.handlers.add(handler);
    return () => this.handlers.delete(handler);
  }
}

// One bus per event type — created once when this module is first imported
export const simStatusBus    = new EventBus<SimStatusPayload>("sim-status");
export const ledChangedBus   = new EventBus<LedChangedPayload>("led-changed");
export const displayFrameBus = new EventBus<DisplayFramePayload>("display-frame");
export const uartOutputBus   = new EventBus<UartOutputPayload>("uart-output");
export const simStepsBus     = new EventBus<number>("sim-steps");

// Keep old function signatures for backward compat (used in DisplayWidget)
export function onDisplayFrame(handler: (p: DisplayFramePayload) => void): Promise<UnlistenFn> {
  const unsub = displayFrameBus.subscribe(handler);
  return Promise.resolve(unsub);
}
