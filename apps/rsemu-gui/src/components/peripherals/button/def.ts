import type { PeripheralModuleDef, PinConfigProps } from "../registry";
import ButtonPinConfig from "./PinConfig";
import ButtonWidget from "./Widget";

export const buttonDef: PeripheralModuleDef = {
  type: "button",
  label: "Button",
  color: "bg-green-900",
  textColor: "text-green-200",
  icon: "🔘",
  PinConfig: ButtonPinConfig,
  Widget: ButtonWidget,
};
