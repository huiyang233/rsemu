import { invoke } from "@tauri-apps/api/core";
import { listen, UnlistenFn } from "@tauri-apps/api/event";
import type { BoardInfo } from "../types/board";
import type { PeripheralConfig } from "../types/peripheral";
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

export async function sendUart(peripheral: string, bytes: number[]): Promise<void> {
  return invoke("send_uart", { peripheral, bytes });
}

export async function openFirmwareDialog(): Promise<string | null> {
  return invoke<string | null>("open_firmware_dialog");
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
