import type { PeripheralType, PeripheralConfig, CanvasItem, PinMapping } from "../../types/peripheral";
import type { BoardInfo } from "../../types/board";
import type { ComponentType } from "react";

// ── Module definition interface ───────────────────────────────────────────────

export interface PeripheralModuleDef {
  type: PeripheralType;
  label: string;
  color: string;
  textColor: string;
  icon: string;
  PinConfig: ComponentType<PinConfigProps>;
  Widget: ComponentType<{ config: any }>;
}

export interface PinConfigProps {
  item: CanvasItem;
  board: BoardInfo;
  onSave: (instanceId: string, config: PeripheralConfig) => void;
  onClose: () => void;
}

// ── Shared helpers ────────────────────────────────────────────────────────────

export function makePinOptions(board: BoardInfo): { value: string; label: string }[] {
  return board.gpio_ports.flatMap((port) =>
    Array.from({ length: 16 }, (_, pin) => ({
      value: `${port}:${pin}`,
      label: `P${port}${pin}`,
    }))
  );
}

export function parsePin(value: string): PinMapping {
  const [port, pin] = value.split(":");
  return { port, pin: parseInt(pin, 10) };
}

// ── Registry ──────────────────────────────────────────────────────────────────

import { st7789SpiDef, st7789FsmcDef } from "./st7789/def";
import { ssd1306Def } from "./ssd1306/def";
import { ledDef } from "./led/def";
import { buttonDef } from "./button/def";
import { uartDef } from "./uart/def";

const allModules: PeripheralModuleDef[] = [
  st7789SpiDef,
  st7789FsmcDef,
  ssd1306Def,
  ledDef,
  buttonDef,
  uartDef,
];

export const typeToDef = new Map(allModules.map((m) => [m.type, m]));
export const typeToWidget = new Map(allModules.map((m) => [m.type, m.Widget]));
export const typeToPinConfig = new Map(allModules.map((m) => [m.type, m.PinConfig]));

export const paletteEntries = allModules.map((m) => ({
  type: m.type,
  label: m.label,
  color: m.color,
  textColor: m.textColor,
  icon: m.icon,
}));

// Backward compat: "st7789" was the old type before the SPI/FSMC split
typeToDef.set("st7789" as PeripheralType, st7789SpiDef);

export function getPeripheralDef(type: PeripheralType): PeripheralModuleDef | undefined {
  return typeToDef.get(type);
}
