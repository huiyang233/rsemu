import React, { useEffect, useState } from "react";
import type { PinConfigProps } from "../registry";
import { makePinOptions, parsePin } from "../registry";
import type { JoystickConfig } from "../../../types/peripheral";
import Button from "../../ui/Button";
import PinSelect from "../PinSelect";
import LabelInput from "../LabelInput";

export default function JoystickPinConfig({
  item,
  board,
  onSave,
  onClose,
}: PinConfigProps) {
  const pinOptions = makePinOptions(board);
  const defaultPin = pinOptions[0]?.value ?? "A:0";
  const defaultPinY = pinOptions[1]?.value ?? "A:1";

  const [joyId, setJoyId] = useState(`JOY_${item.instanceId.slice(0, 4)}`);
  const [pinX, setPinX] = useState(defaultPin);
  const [pinY, setPinY] = useState(defaultPinY);
  const [adcX, setAdcX] = useState("ADC1");
  const [adcY, setAdcY] = useState("ADC1");

  useEffect(() => {
    if (!item.config) return;
    const c = item.config as JoystickConfig;
    setJoyId(c.id);
    setPinX(`${c.pin_x.port}:${c.pin_x.pin}`);
    setPinY(`${c.pin_y.port}:${c.pin_y.pin}`);
    setAdcX(c.adc_x);
    setAdcY(c.adc_y);
  }, [item.config, item.instanceId]);

  const handleSave = () => {
    const config: JoystickConfig = {
      type: "joystick",
      id: joyId,
      pin_x: parsePin(pinX),
      pin_y: parsePin(pinY),
      adc_x: adcX,
      adc_y: adcY,
    };
    onSave(item.instanceId, config);
    onClose();
  };

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60">
      <div className="bg-[#1e1e2e] border border-[#3a3a5e] rounded-lg w-96 shadow-2xl">
        <div className="flex items-center justify-between px-4 py-3 rounded-t-lg bg-blue-900">
          <h2 className="font-semibold text-blue-200">🕹️ Configure Joystick</h2>
          <button onClick={onClose} className="text-[#6c7086] hover:text-white text-sm">✕</button>
        </div>
        <div className="p-4 space-y-3">
          <LabelInput label="ID" value={joyId} onChange={setJoyId} />
          <div className="space-y-1">
            <p className="text-xs text-[#6c7086] font-medium">X Axis</p>
            <PinSelect label="GPIO Pin" options={pinOptions} value={pinX} onChange={setPinX} />
            <LabelInput label="ADC Peripheral" value={adcX} onChange={setAdcX} />
          </div>
          <div className="space-y-1">
            <p className="text-xs text-[#6c7086] font-medium">Y Axis</p>
            <PinSelect label="GPIO Pin" options={pinOptions} value={pinY} onChange={setPinY} />
            <LabelInput label="ADC Peripheral" value={adcY} onChange={setAdcY} />
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
