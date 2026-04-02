import React, { useRef, useEffect, useState, useCallback } from "react";
import { useAppStore } from "../../store/appStore";
import { sendUart } from "../../lib/tauri";
import type { UartConfig } from "../../types/peripheral";
import Button from "../ui/Button";

interface Props {
  config: UartConfig;
}

export default function UartTerminal({ config }: Props) {
  const output = useAppStore((s) => s.uartOutput[config.usart] ?? "");
  const clearOutput = useAppStore((s) => s.clearUartOutput);
  const [input, setInput] = useState("");
  const bottomRef = useRef<HTMLDivElement>(null);

  // Auto-scroll to bottom on new output
  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [output]);

  const handleSend = useCallback(async () => {
    if (!input) return;
    const bytes = Array.from(input + "\r\n").map((c) => c.charCodeAt(0));
    await sendUart(config.usart, bytes);
    setInput("");
  }, [input, config.usart]);

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "Enter") handleSend();
  };

  return (
    <div className="flex flex-col border border-[#3a3a5e] rounded-lg overflow-hidden bg-[#181825] w-full">
      {/* Title bar */}
      <div className="flex items-center justify-between px-3 py-1.5 bg-[#2a2a3e] border-b border-[#3a3a5e]">
        <span className="text-xs font-medium text-purple-300">⌨ {config.usart}</span>
        <Button
          variant="ghost"
          size="sm"
          onClick={() => clearOutput(config.usart)}
          className="text-xs text-[#6c7086]"
        >
          Clear
        </Button>
      </div>

      {/* Output area */}
      <pre
        className="flex-1 min-h-[120px] max-h-[240px] overflow-y-auto px-3 py-2 text-xs font-mono text-green-400 whitespace-pre-wrap break-all"
      >
        {output || <span className="text-[#45475a]">Waiting for output…</span>}
        <div ref={bottomRef} />
      </pre>

      {/* Input row */}
      <div className="flex items-center gap-1 px-2 py-1.5 border-t border-[#3a3a5e] bg-[#1e1e2e]">
        <input
          type="text"
          value={input}
          onChange={(e) => setInput(e.target.value)}
          onKeyDown={handleKeyDown}
          placeholder="Type and press Enter to send…"
          className="flex-1 bg-transparent text-xs text-[#cdd6f4] placeholder-[#45475a] focus:outline-none"
        />
        <Button variant="primary" size="sm" onClick={handleSend}>Send</Button>
      </div>
    </div>
  );
}
