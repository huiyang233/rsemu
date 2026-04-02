export interface PinMapping {
  port: string; // "A" | "B" | "C" | ...
  pin: number;  // 0-15
}

export type PeripheralType = "st7789" | "led" | "button" | "uart";

// ── Per-peripheral config payloads (sent to Rust backend) ────────────────────

export interface St7789Config {
  type: "st7789";
  width: number;
  height: number;
  spi_base: number;
  cs: PinMapping;
  dc: PinMapping;
  res?: PinMapping;
}

export interface LedConfig {
  type: "led";
  id: string;
  pin: PinMapping;
  active_low: boolean;
}

export interface UartConfig {
  type: "uart";
  usart: string;
}

export interface ButtonConfig {
  type: "button";
  id: string;
  pin: PinMapping;
}

export type PeripheralConfig =
  | St7789Config
  | LedConfig
  | UartConfig
  | ButtonConfig;

// ── Canvas item (what the setup page tracks) ─────────────────────────────────

export interface CanvasItem {
  /** Unique instance ID (UUID) */
  instanceId: string;
  type: PeripheralType;
  position: { x: number; y: number };
  config: PeripheralConfig | null; // null until the user fills in pin assignments
}
