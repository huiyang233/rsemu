import type { PeripheralModuleDef, PinConfigProps } from "../registry";
import { TerminalSquare } from "lucide-react";
import UartPinConfig from "./PinConfig";
import UartWidget from "./Widget";

export const uartDef: PeripheralModuleDef = {
  type: "uart",
  label: "UART Terminal",
  color: "bg-purple-900",
  textColor: "text-purple-200",
  icon: TerminalSquare,
  category: "communication",
  PinConfig: UartPinConfig,
  Widget: UartWidget,
};
