import type { PeripheralModuleDef, PinConfigProps } from "../registry";
import Ssd1306PinConfig from "./PinConfig";
import Ssd1306Widget from "./Widget";

export const ssd1306Def: PeripheralModuleDef = {
  type: "ssd1306_i2c",
  label: "SSD1306 (I2C)",
  color: "bg-cyan-900",
  textColor: "text-cyan-200",
  icon: "📟",
  PinConfig: Ssd1306PinConfig,
  Widget: Ssd1306Widget,
};
