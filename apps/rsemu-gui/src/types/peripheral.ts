export interface PinMapping {
  port: string; // "A" | "B" | "C" | ...
  pin: number;  // 0-15
}

export type PeripheralType =
  | "st7789_spi"
  | "st7789_fsmc"
  | "ssd1306_i2c"
  | "led"
  | "button"
  | "uart";

// ── Per-peripheral config payloads (sent to Rust backend) ────────────────────

export interface St7789SpiConfig {
  type: "st7789_spi";
  width: number;
  height: number;
  spi_base: number;
  cs: PinMapping;
  dc: PinMapping;
  res?: PinMapping;
}

export interface St7789FsmcConfig {
  type: "st7789_fsmc";
  width: number;
  height: number;
  fsmc_base: number;
}

export interface LedConfig {
  type: "led";
  id?: string;
  pin: PinMapping;
  active_low: boolean;
}

export interface Ssd1306I2cConfig {
  type: "ssd1306_i2c";
  width: number;
  height: number;
  i2c: string;
  address: number;
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
  | St7789SpiConfig
  | St7789FsmcConfig
  | Ssd1306I2cConfig
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
