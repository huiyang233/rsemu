import React, { useState, useEffect } from "react";
import type { PinConfigProps } from "../registry";
import { makePinOptions, parsePin } from "../registry";
import type { LedConfig } from "../../../types/peripheral";
import Button from "../../ui/Button";
import PinSelect from "../PinSelect";
import LabelInput from "../LabelInput";

export default function LedPinConfig({ item, board, onSave, onClose }: PinConfigProps) {
  const pinOptions = makePinOptions(board);
  const defaultPin = pinOptions[0]?.value ?? "A:0";

  const [ledId, setLedId] = useState(`LED_${item.instanceId.slice(0, 4)}`);
  const [pin, setPin] = useState(defaultPin);
  const [activeLow, setActiveLow] = useState(true);

  useEffect(() => {
    if (!item.config) return;
    const c = item.config as LedConfig;
    setLedId(c.id ?? `LED_${item.instanceId.slice(0, 4)}`);
    setActiveLow(c.active_low);
    setPin(`${c.pin.port}:${c.pin.pin}`);
  }, [item.config, item.instanceId]);

  const handleSave = () => {
    const config: LedConfig = {
      type: "led",
      id: ledId,
      pin: parsePin(pin),
      active_low: activeLow,
    };
    onSave(item.instanceId, config);
    onClose();
  };

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60">
      <div className="bg-[#1e1e2e] border border-[#3a3a5e] rounded-lg w-96 shadow-2xl">
        <div className="flex items-center justify-between px-4 py-3 rounded-t-lg bg-yellow-900">
          <h2 className="font-semibold text-yellow-200">💡 Configure LED</h2>
          <button onClick={onClose} className="text-[#6c7086] hover:text-white text-sm">✕</button>
        </div>
        <div className="p-4 space-y-3">
          <LabelInput label="LED ID" value={ledId} onChange={setLedId} />
          <PinSelect label="GPIO Pin" options={pinOptions} value={pin} onChange={setPin} />
          <label className="flex items-center gap-2 text-sm text-[#cdd6f4] cursor-pointer">
            <input
              type="checkbox"
              checked={activeLow}
              onChange={(e) => setActiveLow(e.target.checked)}
              className="accent-indigo-500"
            />
            Active-low (LED on when pin is LOW)
          </label>
        </div>
        <div className="flex justify-end gap-2 px-4 pb-4">
          <Button variant="ghost" onClick={onClose}>Cancel</Button>
          <Button variant="primary" onClick={handleSave}>Save</Button>
        </div>
      </div>
    </div>
  );
}
