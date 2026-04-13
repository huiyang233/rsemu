import type { PeripheralModuleDef } from "../registry";
import { Gamepad2 } from "lucide-react";
import JoystickPinConfig from "./PinConfig";
import JoystickWidget from "./Widget";

export const joystickDef: PeripheralModuleDef = {
  type: "joystick",
  label: "Joystick",
  color: "bg-blue-900",
  textColor: "text-blue-200",
  icon: Gamepad2,
  category: "basic-io",
  PinConfig: JoystickPinConfig,
  Widget: JoystickWidget,
};
