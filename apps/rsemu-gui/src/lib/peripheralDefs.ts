import { PeripheralType } from "../types/peripheral";

export interface PinDef {
  key: string;
  label: string;
  required: boolean;
}

export interface PeripheralTypeDef {
  type: PeripheralType;
  label: string;
  color: string;       // Tailwind bg color class
  textColor: string;   // Tailwind text color class
  pins: PinDef[];      // For ST7789 / Button / LED
  icon: string;        // emoji / text icon
}

export const PERIPHERAL_DEFS: PeripheralTypeDef[] = [
  {
    type: "st7789",
    label: "ST7789 Display",
    color: "bg-blue-900",
    textColor: "text-blue-200",
    icon: "🖥",
    pins: [
      { key: "cs",  label: "CS (Chip Select)", required: true },
      { key: "dc",  label: "DC (Data/Cmd)",    required: true },
      { key: "res", label: "RST (Reset)",       required: false },
    ],
  },
  {
    type: "led",
    label: "LED",
    color: "bg-yellow-900",
    textColor: "text-yellow-200",
    icon: "💡",
    pins: [
      { key: "pin", label: "GPIO Pin", required: true },
    ],
  },
  {
    type: "button",
    label: "Button",
    color: "bg-green-900",
    textColor: "text-green-200",
    icon: "🔘",
    pins: [
      { key: "pin", label: "GPIO Pin", required: true },
    ],
  },
  {
    type: "uart",
    label: "UART Terminal",
    color: "bg-purple-900",
    textColor: "text-purple-200",
    icon: "⌨",
    pins: [], // USART is selected by name, not pin mapping
  },
];

export function getPeripheralDef(type: PeripheralType): PeripheralTypeDef {
  return PERIPHERAL_DEFS.find((d) => d.type === type)!;
}
