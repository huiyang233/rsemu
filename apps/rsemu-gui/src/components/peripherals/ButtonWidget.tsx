import React, { useCallback, useState } from "react";
import { injectGpio } from "../../lib/tauri";
import type { ButtonConfig } from "../../types/peripheral";

interface Props {
  config: ButtonConfig;
}

export default function ButtonWidget({ config }: Props) {
  const [pressed, setPressed] = useState(false);

  const handlePress = useCallback(async () => {
    setPressed(true);
    await injectGpio(config.pin.port, config.pin.pin, false); // active-low button
  }, [config]);

  const handleRelease = useCallback(async () => {
    setPressed(false);
    await injectGpio(config.pin.port, config.pin.pin, true);
  }, [config]);

  return (
    <div className="flex flex-col items-center gap-2 p-3 bg-[#2a2a3e] rounded-lg border border-[#3a3a5e] w-28">
      <button
        onMouseDown={handlePress}
        onMouseUp={handleRelease}
        onMouseLeave={() => { if (pressed) handleRelease(); }}
        onTouchStart={(e) => { e.preventDefault(); handlePress(); }}
        onTouchEnd={(e) => { e.preventDefault(); handleRelease(); }}
        className={`
          w-12 h-12 rounded-full border-2 font-bold text-xs transition-all duration-75
          select-none cursor-pointer
          ${pressed
            ? "bg-green-500 border-green-300 shadow-[0_0_10px_rgba(74,222,128,0.5)] scale-95"
            : "bg-[#313244] border-[#45475a] hover:border-green-600 hover:bg-[#3a3a5e]"
          }
        `}
      >
        {pressed ? "●" : "○"}
      </button>
      <div className="text-center">
        <p className="text-xs font-medium text-[#cdd6f4]">{config.id}</p>
        <p className="text-xs text-[#6c7086]">
          P{config.pin.port}{config.pin.pin}
        </p>
        <p className={`text-xs mt-0.5 ${pressed ? "text-green-400" : "text-[#45475a]"}`}>
          {pressed ? "PRESSED" : "IDLE"}
        </p>
      </div>
    </div>
  );
}
