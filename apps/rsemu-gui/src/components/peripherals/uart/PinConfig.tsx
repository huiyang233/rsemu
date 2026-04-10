import React, { useState, useEffect } from "react";
import type { PinConfigProps } from "../registry";
import type { UartConfig } from "../../../types/peripheral";
import Select from "../../ui/Select";
import Button from "../../ui/Button";

export default function UartPinConfig({ item, board, onSave, onClose }: PinConfigProps) {
  const [usart, setUsart] = useState(board.usart_peripherals[0]?.name ?? "USART1");

  useEffect(() => {
    setUsart(board.usart_peripherals[0]?.name ?? "USART1");
    if (!item.config) return;
    const c = item.config as UartConfig;
    setUsart(c.usart);
  }, [board.usart_peripherals, item.config]);

  const handleSave = () => {
    const config: UartConfig = {
      type: "uart",
      usart,
    };
    onSave(item.instanceId, config);
    onClose();
  };

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60">
      <div className="bg-[#1e1e2e] border border-[#3a3a5e] rounded-lg w-96 shadow-2xl">
        <div className="flex items-center justify-between px-4 py-3 rounded-t-lg bg-purple-900">
          <h2 className="font-semibold text-purple-200">⌨ Configure UART Terminal</h2>
          <button onClick={onClose} className="text-[#6c7086] hover:text-white text-sm">✕</button>
        </div>
        <div className="p-4 space-y-3">
          <Select label="USART Peripheral" value={usart} onChange={(e) => setUsart(e.target.value)}>
            {board.usart_peripherals.map((u) => (
              <option key={u.name} value={u.name}>{u.name}</option>
            ))}
          </Select>
        </div>
        <div className="flex justify-end gap-2 px-4 pb-4">
          <Button variant="ghost" onClick={onClose}>Cancel</Button>
          <Button variant="primary" onClick={handleSave}>Save</Button>
        </div>
      </div>
    </div>
  );
}
