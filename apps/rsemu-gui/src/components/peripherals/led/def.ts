import type { PeripheralModuleDef, PinConfigProps } from "../registry";
import { Lightbulb } from "lucide-react";
import LedPinConfig from "./PinConfig";
import LedWidget from "./Widget";

export const ledDef: PeripheralModuleDef = {
  type: "led",
  label: "LED",
  color: "bg-yellow-900",
  textColor: "text-yellow-200",
  icon: Lightbulb,
  category: "basic-io",
  PinConfig: LedPinConfig,
  Widget: LedWidget,
};
