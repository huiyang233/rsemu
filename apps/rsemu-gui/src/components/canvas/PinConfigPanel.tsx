import React, { useState, useEffect } from "react";
import type { CanvasItem, PeripheralConfig, PinMapping } from "../../types/peripheral";
import type { BoardInfo } from "../../types/board";
import { getPeripheralDef } from "../../lib/peripheralDefs";
import Button from "../ui/Button";
import Select from "../ui/Select";

interface Props {
  item: CanvasItem;
  board: BoardInfo;
  onSave: (instanceId: string, config: PeripheralConfig) => void;
  onClose: () => void;
}

function makePinOptions(board: BoardInfo): { value: string; label: string }[] {
  return board.gpio_ports.flatMap((port) =>
    Array.from({ length: 16 }, (_, pin) => ({
      value: `${port}:${pin}`,
      label: `P${port}${pin}`,
    }))
  );
}

function parsePin(value: string): PinMapping {
  const [port, pin] = value.split(":");
  return { port, pin: parseInt(pin, 10) };
}

export default function PinConfigPanel({ item, board, onSave, onClose }: Props) {
  const def = getPeripheralDef(item.type);
  const pinOptions = makePinOptions(board);
  const defaultPin = pinOptions[0]?.value ?? "A:0";

  // ── Local state ──────────────────────────────────────────────────────────
  const [pins, setPins] = useState<Record<string, string>>({});
  const [spiBase, setSpiBase] = useState<string>(
    board.spi_peripherals[0]?.base.toString(16).padStart(8, "0") ?? ""
  );
  const [displayW, setDisplayW] = useState(240);
  const [displayH, setDisplayH] = useState(320);
  const [activeLow, setActiveLow] = useState(true);
  const [ledId, setLedId] = useState(`LED_${item.instanceId.slice(0, 4)}`);
  const [btnId, setBtnId] = useState(`BTN_${item.instanceId.slice(0, 4)}`);
  const [usart, setUsart] = useState(board.usart_peripherals[0]?.name ?? "USART1");

  // Pre-fill from existing config
  useEffect(() => {
    if (!item.config) return;
    const c = item.config;
    if (c.type === "st7789") {
      setSpiBase(c.spi_base.toString(16).padStart(8, "0"));
      setDisplayW(c.width);
      setDisplayH(c.height);
      setPins({
        cs: `${c.cs.port}:${c.cs.pin}`,
        dc: `${c.dc.port}:${c.dc.pin}`,
        res: c.res ? `${c.res.port}:${c.res.pin}` : defaultPin,
      });
    } else if (c.type === "led") {
      setLedId(c.id);
      setActiveLow(c.active_low);
      setPins({ pin: `${c.pin.port}:${c.pin.pin}` });
    } else if (c.type === "button") {
      setBtnId(c.id);
      setPins({ pin: `${c.pin.port}:${c.pin.pin}` });
    } else if (c.type === "uart") {
      setUsart(c.usart);
    }
  }, [item.config]);

  const getPin = (key: string) => pins[key] ?? defaultPin;
  const setPin = (key: string, val: string) => setPins((p) => ({ ...p, [key]: val }));

  const handleSave = () => {
    let config: PeripheralConfig;
    if (item.type === "st7789") {
      config = {
        type: "st7789",
        width: displayW,
        height: displayH,
        spi_base: parseInt(spiBase, 16),
        cs: parsePin(getPin("cs")),
        dc: parsePin(getPin("dc")),
        res: pins["res"] ? parsePin(pins["res"]) : undefined,
      };
    } else if (item.type === "led") {
      config = {
        type: "led",
        id: ledId,
        pin: parsePin(getPin("pin")),
        active_low: activeLow,
      };
    } else if (item.type === "button") {
      config = {
        type: "button",
        id: btnId,
        pin: parsePin(getPin("pin")),
      };
    } else {
      config = { type: "uart", usart };
    }
    onSave(item.instanceId, config);
    onClose();
  };

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60">
      <div className="bg-[#1e1e2e] border border-[#3a3a5e] rounded-lg w-96 shadow-2xl">
        {/* Header */}
        <div className={`flex items-center justify-between px-4 py-3 rounded-t-lg ${def.color}`}>
          <h2 className={`font-semibold ${def.textColor}`}>
            {def.icon} Configure {def.label}
          </h2>
          <button onClick={onClose} className="text-[#6c7086] hover:text-white text-sm">✕</button>
        </div>

        <div className="p-4 space-y-3">
          {/* ST7789 */}
          {item.type === "st7789" && (
            <>
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
            </>
          )}

          {/* LED */}
          {item.type === "led" && (
            <>
              <LabelInput label="LED ID" value={ledId} onChange={setLedId} />
              <PinSelect label="GPIO Pin" options={pinOptions} value={getPin("pin")} onChange={(v) => setPin("pin", v)} />
              <label className="flex items-center gap-2 text-sm text-[#cdd6f4] cursor-pointer">
                <input
                  type="checkbox"
                  checked={activeLow}
                  onChange={(e) => setActiveLow(e.target.checked)}
                  className="accent-indigo-500"
                />
                Active-low (LED on when pin is LOW)
              </label>
            </>
          )}

          {/* Button */}
          {item.type === "button" && (
            <>
              <LabelInput label="Button ID" value={btnId} onChange={setBtnId} />
              <PinSelect label="GPIO Pin" options={pinOptions} value={getPin("pin")} onChange={(v) => setPin("pin", v)} />
            </>
          )}

          {/* UART */}
          {item.type === "uart" && (
            <Select label="USART Peripheral" value={usart} onChange={(e) => setUsart(e.target.value)}>
              {board.usart_peripherals.map((u) => (
                <option key={u.name} value={u.name}>{u.name}</option>
              ))}
            </Select>
          )}
        </div>

        <div className="flex justify-end gap-2 px-4 pb-4">
          <Button variant="ghost" onClick={onClose}>Cancel</Button>
          <Button variant="primary" onClick={handleSave}>Save</Button>
        </div>
      </div>
    </div>
  );
}

// ── Helper sub-components ────────────────────────────────────────────────────

function PinSelect({
  label,
  options,
  value,
  onChange,
}: {
  label: string;
  options: { value: string; label: string }[];
  value: string;
  onChange: (v: string) => void;
}) {
  return (
    <Select label={label} value={value} onChange={(e) => onChange(e.target.value)}>
      {options.map((o) => (
        <option key={o.value} value={o.value}>{o.label}</option>
      ))}
    </Select>
  );
}

function LabelInput({
  label,
  value,
  onChange,
}: {
  label: string;
  value: string;
  onChange: (v: string) => void;
}) {
  return (
    <label className="flex flex-col gap-1 text-xs text-[#a6adc8]">
      {label}
      <input
        type="text"
        value={value}
        onChange={(e) => onChange(e.target.value)}
        className="bg-[#2a2a3e] border border-[#3a3a5e] rounded px-2 py-1.5 text-sm text-[#cdd6f4] focus:outline-none focus:border-indigo-500"
      />
    </label>
  );
}
