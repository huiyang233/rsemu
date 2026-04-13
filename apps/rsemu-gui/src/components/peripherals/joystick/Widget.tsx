import React, { useCallback, useEffect, useRef, useState } from "react";
import { injectAdc, pinToAdcChannel } from "../../../lib/tauri";
import type { JoystickConfig } from "../../../types/peripheral";

interface Props {
  config: JoystickConfig;
  interactive?: boolean;
}

const BASE_R = 48;   // radius of the base circle (px)
const HANDLE_R = 16; // radius of the draggable handle (px)
const MAX_TRAVEL = BASE_R - HANDLE_R; // max displacement from center

export default function JoystickWidget({ config, interactive = true }: Props) {
  // Normalized position: -1..1 for both axes
  const [pos, setPos] = useState({ x: 0, y: 0 });
  const [dragging, setDragging] = useState(false);
  const baseRef = useRef<HTMLDivElement>(null);

  const getClampedPos = useCallback((clientX: number, clientY: number) => {
    if (!baseRef.current) return { x: 0, y: 0 };
    const rect = baseRef.current.getBoundingClientRect();
    const cx = rect.left + rect.width / 2;
    const cy = rect.top + rect.height / 2;
    let nx = (clientX - cx) / MAX_TRAVEL;
    let ny = (clientY - cy) / MAX_TRAVEL;
    const dist = Math.sqrt(nx * nx + ny * ny);
    if (dist > 1) { nx /= dist; ny /= dist; }
    return { x: nx, y: ny };
  }, []);

  const injectPos = useCallback(
    async (nx: number, ny: number) => {
      const xVal = Math.round(((nx + 1) / 2) * 4095);
      const yVal = Math.round(((ny + 1) / 2) * 4095);
      await injectAdc(config.adc_x, pinToAdcChannel(config.pin_x), xVal);
      await injectAdc(config.adc_y, pinToAdcChannel(config.pin_y), yVal);
    },
    [config],
  );

  const handlePointerMove = useCallback(
    (e: PointerEvent) => {
      const p = getClampedPos(e.clientX, e.clientY);
      setPos(p);
      injectPos(p.x, p.y);
    },
    [getClampedPos, injectPos],
  );

  const handlePointerUp = useCallback(() => {
    setDragging(false);
    setPos({ x: 0, y: 0 });
    injectPos(0, 0); // center = 2048, 2048
  }, [injectPos]);

  useEffect(() => {
    if (!dragging) return;
    window.addEventListener("pointermove", handlePointerMove);
    window.addEventListener("pointerup", handlePointerUp);
    return () => {
      window.removeEventListener("pointermove", handlePointerMove);
      window.removeEventListener("pointerup", handlePointerUp);
    };
  }, [dragging, handlePointerMove, handlePointerUp]);

  const handleX = pos.x * MAX_TRAVEL;
  const handleY = pos.y * MAX_TRAVEL;
  const xVal = Math.round(((pos.x + 1) / 2) * 4095);
  const yVal = Math.round(((pos.y + 1) / 2) * 4095);

  return (
    <div className="flex flex-col items-center gap-2 p-3">
      <p className="text-xs font-medium text-[#cdd6f4]">{config.id}</p>

      {/* Base */}
      <div
        ref={baseRef}
        className={`relative rounded-full bg-[#313244] border-2 select-none ${
          interactive
            ? "cursor-grab border-[#45475a] hover:border-blue-600"
            : "cursor-not-allowed opacity-50 border-[#45475a]"
        } ${dragging ? "cursor-grabbing border-blue-400" : ""}`}
        style={{ width: BASE_R * 2, height: BASE_R * 2 }}
        onPointerDown={(e) => {
          if (!interactive) return;
          e.preventDefault();
          setDragging(true);
          const p = getClampedPos(e.clientX, e.clientY);
          setPos(p);
          injectPos(p.x, p.y);
        }}
      >
        {/* Cross-hair lines */}
        <div className="absolute inset-0 flex items-center justify-center pointer-events-none">
          <div className="w-full h-px bg-[#45475a]" />
        </div>
        <div className="absolute inset-0 flex items-center justify-center pointer-events-none">
          <div className="h-full w-px bg-[#45475a]" />
        </div>

        {/* Handle */}
        <div
          className={`absolute rounded-full border-2 pointer-events-none ${
            dragging
              ? "bg-blue-400 border-blue-200 shadow-[0_0_10px_rgba(96,165,250,0.5)]"
              : "bg-blue-500 border-blue-300"
          }`}
          style={{
            width: HANDLE_R * 2,
            height: HANDLE_R * 2,
            left: BASE_R + handleX - HANDLE_R,
            top: BASE_R + handleY - HANDLE_R,
            transition: dragging ? "none" : "left 0.15s ease-out, top 0.15s ease-out",
          }}
        />
      </div>

      <div className="text-xs text-[#6c7086] text-center space-y-0.5">
        <p>X: <span className="text-[#a6adc8]">{xVal}</span> &nbsp; Y: <span className="text-[#a6adc8]">{yVal}</span></p>
        <p>P{config.pin_x.port}{config.pin_x.pin}/{config.adc_x} · P{config.pin_y.port}{config.pin_y.pin}/{config.adc_y}</p>
      </div>
    </div>
  );
}
