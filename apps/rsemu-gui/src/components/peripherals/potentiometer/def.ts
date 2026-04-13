import type { PeripheralModuleDef } from "../registry";
import { SlidersHorizontal } from "lucide-react";
import PotentiometerPinConfig from "./PinConfig";
import PotentiometerWidget from "./Widget";

export const potentiometerDef: PeripheralModuleDef = {
  type: "potentiometer",
  label: "Potentiometer",
  color: "bg-purple-900",
  textColor: "text-purple-200",
  icon: SlidersHorizontal,
  category: "basic-io",
  PinConfig: PotentiometerPinConfig,
  Widget: PotentiometerWidget,
};
