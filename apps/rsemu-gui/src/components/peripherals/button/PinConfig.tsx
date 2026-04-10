import React, { useState, useEffect } from "react";
import type { PinConfigProps } from "../registry";
import { makePinOptions, parsePin } from "../registry";
import type { ButtonConfig } from "../../../types/peripheral";
import Button from "../../ui/Button";
import PinSelect from "../PinSelect";
import LabelInput from "../LabelInput";

export default function ButtonPinConfig({ item, board, onSave, onClose }: PinConfigProps) {
  const pinOptions = makePinOptions(board);
  const defaultPin = pinOptions[0]?.value ?? "A:0";

  const [btnId, setBtnId] = useState(`BTN_${item.instanceId.slice(0, 4)}`);
  const [pin, setPin] = useState(defaultPin);

  useEffect(() => {
    if (!item.config) return;
    const c = item.config as ButtonConfig;
    setBtnId(c.id);
    setPin(`${c.pin.port}:${c.pin.pin}`);
  }, [item.config, item.instanceId]);

  const handleSave = () => {
    const config: ButtonConfig = {
      type: "button",
      id: btnId,
      pin: parsePin(pin),
    };
    onSave(item.instanceId, config);
    onClose();
  };

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60">
      <div className="bg-[#1e1e2e] border border-[#3a3a5e] rounded-lg w-96 shadow-2xl">
        <div className="flex items-center justify-between px-4 py-3 rounded-t-lg bg-green-900">
          <h2 className="font-semibold text-green-200">🔘 Configure Button</h2>
          <button onClick={onClose} className="text-[#6c7086] hover:text-white text-sm">✕</button>
        </div>
        <div className="p-4 space-y-3">
          <LabelInput label="Button ID" value={btnId} onChange={setBtnId} />
          <PinSelect label="GPIO Pin" options={pinOptions} value={pin} onChange={setPin} />
        </div>
        <div className="flex justify-end gap-2 px-4 pb-4">
          <Button variant="ghost" onClick={onClose}>Cancel</Button>
          <Button variant="primary" onClick={handleSave}>Save</Button>
        </div>
      </div>
    </div>
  );
}
