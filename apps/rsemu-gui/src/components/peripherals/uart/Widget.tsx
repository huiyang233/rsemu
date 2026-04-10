import React, { useRef, useEffect, useCallback } from "react";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { uartOutputBus, sendUart } from "../../../lib/tauri";
import type { UartConfig } from "../../../types/peripheral";
import Button from "../../ui/Button";

interface Props {
  config: UartConfig;
}

export default function UartTerminal({ config }: Props) {
  const containerRef = useRef<HTMLDivElement>(null);
  const termRef = useRef<Terminal | null>(null);
  const fitAddonRef = useRef<FitAddon | null>(null);

  // Initialize xterm once on mount
  useEffect(() => {
    if (!containerRef.current || termRef.current) return;

    const term = new Terminal({
      theme: {
        background: "#181825",
        foreground: "#cdd6f4",
        cursor: "#cdd6f4",
        black: "#45475a",
        red: "#f38ba8",
        green: "#a6e3a1",
        yellow: "#f9e2af",
        blue: "#89b4fa",
        magenta: "#cba4f7",
        cyan: "#94e2d5",
        white: "#bac2de",
        brightBlack: "#585b70",
        brightRed: "#f38ba8",
        brightGreen: "#a6e3a1",
        brightYellow: "#f9e2af",
        brightBlue: "#89b4fa",
        brightMagenta: "#cba4f7",
        brightCyan: "#94e2d5",
        brightWhite: "#a6adc8",
      },
      fontSize: 12,
      fontFamily: '"JetBrains Mono", "Fira Code", "Cascadia Code", monospace',
      cursorBlink: true,
      scrollback: 1000,
    });

    const fitAddon = new FitAddon();
    term.loadAddon(fitAddon);
    term.open(containerRef.current);
    fitAddon.fit();

    termRef.current = term;
    fitAddonRef.current = fitAddon;

    // Handle user keyboard input → send to UART
    term.onData((data) => {
      const bytes = Array.from(data).map((c) => c.charCodeAt(0));
      sendUart(config.usart, bytes).catch(console.error);
    });

    return () => {
      term.dispose();
      termRef.current = null;
    };
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  // Subscribe to UART output from the emulator
  useEffect(() => {
    const unsub = uartOutputBus.subscribe((p) => {
      if (p.peripheral.toUpperCase() === config.usart.toUpperCase() && termRef.current) {
        termRef.current.write(String.fromCharCode(p.byte));
      }
    });
    return unsub;
  }, [config.usart]);

  const handleClear = useCallback(() => {
    termRef.current?.clear();
    termRef.current?.reset();
  }, []);

  return (
    <div className="flex flex-col border border-[#3a3a5e] rounded-lg overflow-hidden bg-[#181825] w-full">
      {/* Title bar */}
      <div className="flex items-center justify-between px-3 py-1.5 bg-[#2a2a3e] border-b border-[#3a3a5e]">
        <span className="text-xs font-medium text-purple-300">⌨ {config.usart}</span>
        <Button variant="ghost" size="sm" onClick={handleClear} className="text-xs text-[#6c7086]">
          Clear
        </Button>
      </div>

      {/* xterm.js terminal */}
      <div
        ref={containerRef}
        className="px-1 py-1"
        style={{ height: 240 }}
      />
    </div>
  );
}
