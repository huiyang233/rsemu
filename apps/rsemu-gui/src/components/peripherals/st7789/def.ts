import type { PeripheralModuleDef, PinConfigProps } from "../registry";
import St7789SpiPinConfig from "./St7789SpiPinConfig";
import St7789FsmcPinConfig from "./St7789FsmcPinConfig";
import DisplayWidget from "./Widget";

export const st7789SpiDef: PeripheralModuleDef = {
  type: "st7789_spi",
  label: "ST7789 (SPI)",
  color: "bg-blue-900",
  textColor: "text-blue-200",
  icon: "🖥",
  PinConfig: St7789SpiPinConfig,
  Widget: DisplayWidget,
};

export const st7789FsmcDef: PeripheralModuleDef = {
  type: "st7789_fsmc",
  label: "ST7789 (FSMC)",
  color: "bg-blue-800",
  textColor: "text-blue-200",
  icon: "🖥",
  PinConfig: St7789FsmcPinConfig,
  Widget: DisplayWidget,
};
