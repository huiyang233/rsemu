import React, { useState, useEffect } from "react";
import type { PinConfigProps } from "../registry";
import type { St7789FsmcConfig } from "../../../types/peripheral";
import Select from "../../ui/Select";
import Button from "../../ui/Button";

const DEFAULT_WIDTH = 240;
const DEFAULT_HEIGHT = 320;

export default function St7789FsmcPinConfig({ item, board, onSave, onClose }: PinConfigProps) {
  const [fsmcBase, setFsmcBase] = useState<string>(
    board.fsmc_peripherals[0]?.base.toString(16).padStart(8, "0") ?? "60000000"
  );
  const [displayW, setDisplayW] = useState(DEFAULT_WIDTH);
  const [displayH, setDisplayH] = useState(DEFAULT_HEIGHT);

  useEffect(() => {
    setFsmcBase(board.fsmc_peripherals[0]?.base.toString(16).padStart(8, "0") ?? "60000000");
    if (!item.config) return;
    const c = item.config as St7789FsmcConfig;
    setFsmcBase(c.fsmc_base.toString(16).padStart(8, "0"));
    setDisplayW(c.width);
    setDisplayH(c.height);
  }, [board.fsmc_peripherals, item.config]);

  const handleSave = () => {
    const config: St7789FsmcConfig = {
      type: "st7789_fsmc",
      width: displayW,
      height: displayH,
      fsmc_base: parseInt(fsmcBase, 16) || 0x60000000,
    };
    onSave(item.instanceId, config);
    onClose();
  };

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60">
      <div className="bg-[#1e1e2e] border border-[#3a3a5e] rounded-lg w-96 shadow-2xl">
        <div className="flex items-center justify-between px-4 py-3 rounded-t-lg bg-blue-800">
          <h2 className="font-semibold text-blue-200">🖥 Configure ST7789 (FSMC)</h2>
          <button onClick={onClose} className="text-[#6c7086] hover:text-white text-sm">✕</button>
        </div>
        <div className="p-4 space-y-3">
          <Select label="FSMC Bank Base" value={fsmcBase} onChange={(e) => setFsmcBase(e.target.value)}>
            {board.fsmc_peripherals.length > 0 ? (
              // Show FSMC banks: Bank1=0x60000000, Bank2=0x70000000, etc.
              <>
                <option value="60000000">Bank 1 (0x60000000)</option>
                <option value="64000000">Bank 1 Sub 2 (0x64000000)</option>
                <option value="68000000">Bank 1 Sub 3 (0x68000000)</option>
                <option value="6C000000">Bank 1 Sub 4 (0x6C000000)</option>
                <option value="70000000">Bank 2 (0x70000000)</option>
                <option value="80000000">Bank 3 (0x80000000)</option>
                <option value="90000000">Bank 4 (0x90000000)</option>
              </>
            ) : (
              <option value="60000000">Bank 1 (0x60000000)</option>
            )}
          </Select>
          <div className="flex gap-2">
            <Select label="Width (px)" value={displayW} onChange={(e) => setDisplayW(+e.target.value)}>
              {[128, 160, 240, 320].map((v) => <option key={v}>{v}</option>)}
            </Select>
            <Select label="Height (px)" value={displayH} onChange={(e) => setDisplayH(+e.target.value)}>
              {[128, 160, 240, 320].map((v) => <option key={v}>{v}</option>)}
            </Select>
          </div>
          <p className="text-xs text-[#6c7086]">
            FSMC mode uses address bit 0 for DC (data/command). No pin configuration needed.
          </p>
        </div>
        <div className="flex justify-end gap-2 px-4 pb-4">
          <Button variant="ghost" onClick={onClose}>Cancel</Button>
          <Button variant="primary" onClick={handleSave}>Save</Button>
        </div>
      </div>
    </div>
  );
}
