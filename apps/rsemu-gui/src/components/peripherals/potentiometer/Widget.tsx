import React, { useCallback, useRef, useState } from "react";
import { injectAdc, pinToAdcChannel } from "../../../lib/tauri";
import type { PotentiometerConfig } from "../../../types/peripheral";

interface Props {
  config: PotentiometerConfig;
  interactive?: boolean;
}

export default function PotentiometerWidget({ config, interactive = true }: Props) {
  const [value, setValue] = useState(2048);
  const [dragging, setDragging] = useState(false);
  const trackRef = useRef<HTMLDivElement>(null);
  const isHorizontal = config.orientation !== "vertical";

  const computeValue = useCallback(
    (clientX: number, clientY: number): number => {
      if (!trackRef.current) return value;
      const rect = trackRef.current.getBoundingClientRect();
      const ratio = isHorizontal
        ? (clientX - rect.left) / rect.width
        : (clientY - rect.top) / rect.height;
      return Math.round(Math.max(0, Math.min(1, ratio)) * 4095);
    },
    [isHorizontal, value],
  );

  const handleInject = useCallback(
    async (raw: number) => {
      const channel = pinToAdcChannel(config.pin);
      await injectAdc(config.adc, channel, raw);
    },
    [config],
  );

  const percent = (value / 4095) * 100;

  return (
    <div className="flex flex-col items-center gap-2 p-3">
      <div className="text-center">
        <p className="text-xs font-medium text-[#cdd6f4]">{config.id}</p>
        <p className="text-xs text-[#6c7086]">
          P{config.pin.port}{config.pin.pin} | {config.adc}
        </p>
      </div>

      {/* Track */}
      <div
        ref={trackRef}
        className={`relative rounded-full bg-[#45475a] select-none ${
          interactive ? "cursor-pointer" : "cursor-not-allowed opacity-50"
        } ${isHorizontal ? "w-32 h-3" : "w-3 h-32"}`}
        onPointerDown={(e) => {
          if (!interactive) return;
          e.currentTarget.setPointerCapture(e.pointerId);
          setDragging(true);
          const raw = computeValue(e.clientX, e.clientY);
          setValue(raw);
          handleInject(raw);
        }}
        onPointerMove={(e) => {
          if (!dragging || !interactive) return;
          const raw = computeValue(e.clientX, e.clientY);
          setValue(raw);
          handleInject(raw);
        }}
        onPointerUp={() => setDragging(false)}
        onPointerCancel={() => setDragging(false)}
      >
        {/* Fill */}
        <div
          className="absolute rounded-full bg-purple-600"
          style={
            isHorizontal
              ? { left: 0, top: 0, bottom: 0, width: `${percent}%` }
              : { top: 0, left: 0, right: 0, height: `${percent}%` }
          }
        />
        {/* Thumb */}
        <div
          className="absolute w-5 h-5 rounded-full bg-[#cdd6f4] border-2 border-purple-400 shadow -translate-x-1/2 -translate-y-1/2"
          style={
            isHorizontal
              ? { left: `${percent}%`, top: "50%" }
              : { top: `${percent}%`, left: "50%" }
          }
        />
      </div>

      <p className="text-xs text-[#a6adc8]">
        {value} <span className="text-[#6c7086]">/ 4095</span>
      </p>
    </div>
  );
}
