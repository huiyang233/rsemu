import React, { useState, useEffect } from "react";
import type { PinConfigProps } from "../registry";
import { makePinOptions, parsePin } from "../registry";
import type { St7789SpiConfig } from "../../../types/peripheral";
import Select from "../../ui/Select";
import Button from "../../ui/Button";
import PinSelect from "../PinSelect";

const DEFAULT_WIDTH = 240;
const DEFAULT_HEIGHT = 320;

export default function St7789SpiPinConfig({ item, board, onSave, onClose }: PinConfigProps) {
  const pinOptions = makePinOptions(board);
  const defaultPin = pinOptions[0]?.value ?? "A:0";

  const [pins, setPins] = useState<Record<string, string>>({});
  const [spiBase, setSpiBase] = useState<string>(
    board.spi_peripherals[0]?.base.toString(16).padStart(8, "0") ?? ""
  );
  const [displayW, setDisplayW] = useState(DEFAULT_WIDTH);
  const [displayH, setDisplayH] = useState(DEFAULT_HEIGHT);

  useEffect(() => {
    setSpiBase(board.spi_peripherals[0]?.base.toString(16).padStart(8, "0") ?? "");
    if (!item.config) return;
    const c = item.config as St7789SpiConfig;
    setSpiBase(c.spi_base.toString(16).padStart(8, "0"));
    setDisplayW(c.width);
    setDisplayH(c.height);
    setPins({
      cs: `${c.cs.port}:${c.cs.pin}`,
      dc: `${c.dc.port}:${c.dc.pin}`,
      res: c.res ? `${c.res.port}:${c.res.pin}` : defaultPin,
    });
  }, [board.spi_peripherals, defaultPin, item.config]);

  const getPin = (key: string) => pins[key] ?? defaultPin;
  const setPin = (key: string, val: string) => setPins((p) => ({ ...p, [key]: val }));

  const handleSave = () => {
    const config: St7789SpiConfig = {
      type: "st7789_spi",
      width: displayW,
      height: displayH,
      spi_base: parseInt(spiBase, 16),
      cs: parsePin(getPin("cs")),
      dc: parsePin(getPin("dc")),
      res: pins["res"] ? parsePin(pins["res"]) : undefined,
    };
    onSave(item.instanceId, config);
    onClose();
  };

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60">
      <div className="bg-[#1e1e2e] border border-[#3a3a5e] rounded-lg w-96 shadow-2xl">
        <div className="flex items-center justify-between px-4 py-3 rounded-t-lg bg-blue-900">
          <h2 className="font-semibold text-blue-200">🖥 Configure ST7789 (SPI)</h2>
          <button onClick={onClose} className="text-[#6c7086] hover:text-white text-sm">✕</button>
        </div>
        <div className="p-4 space-y-3">
          <Select label="SPI Peripheral" value={spiBase} onChange={(e) => setSpiBase(e.target.value)}>
            {board.spi_peripherals.map((s) => (
              <option key={s.name} value={s.base.toString(16).padStart(8, "0")}>
                {s.name} (0x{s.base.toString(16).toUpperCase()})
              </option>
            ))}
          </Select>
          <div className="flex gap-2">
            <Select label="Width (px)" value={displayW} onChange={(e) => setDisplayW(+e.target.value)}>
              {[128, 160, 240, 320].map((v) => <option key={v}>{v}</option>)}
            </Select>
            <Select label="Height (px)" value={displayH} onChange={(e) => setDisplayH(+e.target.value)}>
              {[128, 160, 240, 320].map((v) => <option key={v}>{v}</option>)}
            </Select>
          </div>
          <PinSelect label="CS Pin" options={pinOptions} value={getPin("cs")} onChange={(v) => setPin("cs", v)} />
          <PinSelect label="DC Pin" options={pinOptions} value={getPin("dc")} onChange={(v) => setPin("dc", v)} />
          <PinSelect label="RST Pin (optional)" options={pinOptions} value={getPin("res")} onChange={(v) => setPin("res", v)} />
        </div>
        <div className="flex justify-end gap-2 px-4 pb-4">
          <Button variant="ghost" onClick={onClose}>Cancel</Button>
          <Button variant="primary" onClick={handleSave}>Save</Button>
        </div>
      </div>
    </div>
  );
}
