import React, { useEffect, useState } from "react";
import type { PinConfigProps } from "../registry";
import { makePinOptions, parsePin } from "../registry";
import type { PotentiometerConfig } from "../../../types/peripheral";
import Button from "../../ui/Button";
import PinSelect from "../PinSelect";
import LabelInput from "../LabelInput";

export default function PotentiometerPinConfig({
  item,
  board,
  onSave,
  onClose,
}: PinConfigProps) {
  const pinOptions = makePinOptions(board);
  const defaultPin = pinOptions[0]?.value ?? "A:0";

  const [potId, setPotId] = useState(`POT_${item.instanceId.slice(0, 4)}`);
  const [pin, setPin] = useState(defaultPin);
  const [adc, setAdc] = useState("ADC1");
  const [orientation, setOrientation] = useState<"horizontal" | "vertical">("horizontal");

  useEffect(() => {
    if (!item.config) return;
    const c = item.config as PotentiometerConfig;
    setPotId(c.id);
    setPin(`${c.pin.port}:${c.pin.pin}`);
    setAdc(c.adc);
    setOrientation(c.orientation);
  }, [item.config, item.instanceId]);

  const handleSave = () => {
    const config: PotentiometerConfig = {
      type: "potentiometer",
      id: potId,
      pin: parsePin(pin),
      adc,
      orientation,
    };
    onSave(item.instanceId, config);
    onClose();
  };

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60">
      <div className="bg-[#1e1e2e] border border-[#3a3a5e] rounded-lg w-96 shadow-2xl">
        <div className="flex items-center justify-between px-4 py-3 rounded-t-lg bg-purple-900">
          <h2 className="font-semibold text-purple-200">🎚️ Configure Potentiometer</h2>
          <button onClick={onClose} className="text-[#6c7086] hover:text-white text-sm">✕</button>
        </div>
        <div className="p-4 space-y-3">
          <LabelInput label="ID" value={potId} onChange={setPotId} />
          <PinSelect label="GPIO Pin (ADC input)" options={pinOptions} value={pin} onChange={setPin} />
          <LabelInput label="ADC Peripheral" value={adc} onChange={setAdc} />
          <div>
            <label className="block text-xs text-[#6c7086] mb-1">Orientation</label>
            <div className="flex gap-2">
              {(["horizontal", "vertical"] as const).map((o) => (
                <button
                  key={o}
                  onClick={() => setOrientation(o)}
                  className={`flex-1 py-1.5 text-xs rounded border transition-colors ${
                    orientation === o
                      ? "bg-purple-700 border-purple-400 text-purple-100"
                      : "bg-[#313244] border-[#45475a] text-[#6c7086] hover:border-purple-600"
                  }`}
                >
                  {o === "horizontal" ? "↔ Horizontal" : "↕ Vertical"}
                </button>
              ))}
            </div>
          </div>
        </div>
        <div className="flex justify-end gap-2 px-4 pb-4">
          <Button variant="ghost" onClick={onClose}>Cancel</Button>
          <Button variant="primary" onClick={handleSave}>Save</Button>
        </div>
      </div>
    </div>
  );
}
