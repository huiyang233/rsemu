import React, { useRef, useEffect, useCallback, useState } from "react";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { uartOutputBus, sendUart } from "../../../lib/tauri";
import type { UartConfig } from "../../../types/peripheral";

interface Props {
  config: UartConfig;
  onClear?: () => void;
}

export default function UartTerminal({ config, onClear }: Props) {
  const containerRef = useRef<HTMLDivElement>(null);
  const termRef = useRef<Terminal | null>(null);
  const fitAddonRef = useRef<FitAddon | null>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  const [inputText, setInputText] = useState("");
  const [hexMode, setHexMode] = useState(false);

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
      cursorBlink: false,
      scrollback: 1000,
      disableStdin: true,
    });

    const fitAddon = new FitAddon();
    term.loadAddon(fitAddon);
    term.open(containerRef.current);
    fitAddon.fit();

    termRef.current = term;
    fitAddonRef.current = fitAddon;

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
    onClear?.();
  }, [onClear]);

  const handleSend = useCallback(() => {
    if (!inputText.trim()) return;

    let bytes: number[];
    if (hexMode) {
      bytes = inputText
        .trim()
        .split(/\s+/)
        .map((h) => parseInt(h, 16))
        .filter((b) => Number.isFinite(b) && b >= 0 && b <= 255);
      if (!bytes.length) return;
    } else {
      const text = inputText + "\r\n";
      bytes = Array.from(text).map((c) => c.charCodeAt(0));
    }

    sendUart(config.usart, bytes).catch(console.error);
    setInputText("");
    inputRef.current?.focus();
  }, [inputText, hexMode, config.usart]);

  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      if (e.key === "Enter") {
        e.preventDefault();
        handleSend();
      }
    },
    [handleSend]
  );

  return (
    <>
      {/* xterm.js terminal */}
      <div ref={containerRef} className="px-1 py-1" style={{ height: 200 }} />

      {/* Input bar */}
      <div className="flex items-center gap-1.5 px-2 py-1.5 border-t border-[#3a3a5e] bg-[#181825]">
        <input
          ref={inputRef}
          type="text"
          value={inputText}
          onChange={(e) => setInputText(e.target.value)}
          onKeyDown={handleKeyDown}
          placeholder={hexMode ? "HEX: 48 65 6C 6C 6F" : "Type to send..."}
          className="flex-1 bg-[#2a2a3e] border border-[#3a3a5e] rounded px-2 py-1 text-xs text-[#cdd6f4] font-mono focus:outline-none focus:border-indigo-500"
        />
        <button
          onClick={() => setHexMode((m) => !m)}
          className={`px-2 py-1 rounded text-xs font-mono border transition-colors ${
            hexMode
              ? "bg-indigo-600 border-indigo-500 text-white"
              : "bg-[#2a2a3e] border-[#3a3a5e] text-[#6c7086] hover:text-[#cdd6f4]"
          }`}
          title="Toggle HEX mode"
        >
          HEX
        </button>
        <button
          onClick={handleClear}
          className="px-2 py-1 rounded text-xs text-[#6c7086] bg-[#2a2a3e] border border-[#3a3a5e] hover:text-[#cdd6f4] transition-colors"
        >
          Clear
        </button>
        <button
          onClick={handleSend}
          className="px-3 py-1 rounded text-xs font-medium bg-indigo-600 hover:bg-indigo-500 text-white transition-colors"
        >
          Send
        </button>
      </div>
    </>
  );
}
