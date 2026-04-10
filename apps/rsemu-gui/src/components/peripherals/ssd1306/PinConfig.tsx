import React, { useState, useEffect } from "react";
import type { PinConfigProps } from "../registry";
import { makePinOptions, parsePin } from "../registry";
import type { Ssd1306I2cConfig } from "../../../types/peripheral";
import Select from "../../ui/Select";
import Button from "../../ui/Button";
import LabelInput from "../LabelInput";

const WIDTH_OPTIONS = [64, 72, 96, 128];
const HEIGHT_OPTIONS = [32, 40, 48, 64];
const DEFAULT_WIDTH = 128;
const DEFAULT_HEIGHT = 64;

export default function Ssd1306PinConfig({ item, board, onSave, onClose }: PinConfigProps) {
  const [i2c, setI2c] = useState(board.i2c_peripherals[0]?.name ?? "I2C1");
  const [displayW, setDisplayW] = useState(DEFAULT_WIDTH);
  const [displayH, setDisplayH] = useState(DEFAULT_HEIGHT);
  const [addr, setAddr] = useState("3c");

  useEffect(() => {
    setI2c(board.i2c_peripherals[0]?.name ?? "I2C1");
    if (!item.config) return;
    const c = item.config as Ssd1306I2cConfig;
    setDisplayW(WIDTH_OPTIONS.includes(c.width) ? c.width : DEFAULT_WIDTH);
    setDisplayH(HEIGHT_OPTIONS.includes(c.height) ? c.height : DEFAULT_HEIGHT);
    setI2c(c.i2c);
    setAddr(c.address.toString(16));
  }, [board.i2c_peripherals, item.config]);

  const handleSave = () => {
    const trimmed = addr.trim().toLowerCase();
    const parsed = trimmed.startsWith("0x")
      ? parseInt(trimmed.slice(2), 16)
      : parseInt(trimmed, 16);
    const address = Number.isFinite(parsed) ? Math.max(0, Math.min(0x7f, parsed)) : 0x3c;
    const config: Ssd1306I2cConfig = {
      type: "ssd1306_i2c",
      width: WIDTH_OPTIONS.includes(displayW) ? displayW : DEFAULT_WIDTH,
      height: HEIGHT_OPTIONS.includes(displayH) ? displayH : DEFAULT_HEIGHT,
      i2c,
      address,
    };
    onSave(item.instanceId, config);
    onClose();
  };

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60">
      <div className="bg-[#1e1e2e] border border-[#3a3a5e] rounded-lg w-96 shadow-2xl">
        <div className="flex items-center justify-between px-4 py-3 rounded-t-lg bg-cyan-900">
          <h2 className="font-semibold text-cyan-200">📟 Configure SSD1306 (I2C)</h2>
          <button onClick={onClose} className="text-[#6c7086] hover:text-white text-sm">✕</button>
        </div>
        <div className="p-4 space-y-3">
          <Select label="I2C Peripheral" value={i2c} onChange={(e) => setI2c(e.target.value)}>
            {board.i2c_peripherals.map((p) => (
              <option key={p.name} value={p.name}>
                {p.name} (0x{p.base.toString(16).toUpperCase()})
              </option>
            ))}
          </Select>
          <div className="flex gap-2">
            <Select label="Width (px)" value={displayW} onChange={(e) => setDisplayW(+e.target.value)}>
              {WIDTH_OPTIONS.map((v) => <option key={v}>{v}</option>)}
            </Select>
            <Select label="Height (px)" value={displayH} onChange={(e) => setDisplayH(+e.target.value)}>
              {HEIGHT_OPTIONS.map((v) => <option key={v}>{v}</option>)}
            </Select>
          </div>
          <LabelInput
            label="I2C Address (hex, 7-bit, default 0x3C)"
            value={addr}
            onChange={setAddr}
          />
        </div>
        <div className="flex justify-end gap-2 px-4 pb-4">
          <Button variant="ghost" onClick={onClose}>Cancel</Button>
          <Button variant="primary" onClick={handleSave}>Save</Button>
        </div>
      </div>
    </div>
  );
}
