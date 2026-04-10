import React from "react";
import { useAppStore } from "../../../store/appStore";
import type { LedConfig } from "../../../types/peripheral";

interface Props {
  config: LedConfig;
}

export default function LedWidget({ config }: Props) {
  const on = useAppStore((s) => s.ledStates[config.id ?? ""] ?? false);

  return (
    <div className="flex flex-col items-center gap-2 p-3">
      {/* LED indicator */}
      <div
        className={`
          w-10 h-10 rounded-full border-2 transition-all duration-100
          ${on
            ? "bg-yellow-400 border-yellow-300 shadow-[0_0_12px_4px_rgba(250,204,21,0.6)]"
            : "bg-[#313244] border-[#45475a]"
          }
        `}
      />
      {/* Label */}
      <div className="text-center">
        <p className="text-xs font-medium text-[#cdd6f4]">{config.id}</p>
        <p className="text-xs text-[#6c7086]">
          P{config.pin.port}{config.pin.pin}
        </p>
        <p className={`text-xs font-medium mt-0.5 ${on ? "text-yellow-400" : "text-[#45475a]"}`}>
          {on ? "ON" : "OFF"}
        </p>
      </div>
    </div>
  );
}
