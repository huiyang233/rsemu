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

// ── Event listeners ───────────────────────────────────────────────────────────

export function onSimStatus(handler: (p: SimStatusPayload) => void): Promise<UnlistenFn> {
  return listen<SimStatusPayload>("sim-status", (e) => handler(e.payload));
}

export function onLedChanged(handler: (p: LedChangedPayload) => void): Promise<UnlistenFn> {
  return listen<LedChangedPayload>("led-changed", (e) => handler(e.payload));
}

export function onDisplayFrame(handler: (p: DisplayFramePayload) => void): Promise<UnlistenFn> {
  return listen<DisplayFramePayload>("display-frame", (e) => handler(e.payload));
}

export function onUartOutput(handler: (p: UartOutputPayload) => void): Promise<UnlistenFn> {
  return listen<UartOutputPayload>("uart-output", (e) => handler(e.payload));
}

export function onSimSteps(handler: (steps: number) => void): Promise<UnlistenFn> {
  return listen<number>("sim-steps", (e) => handler(e.payload));
}
