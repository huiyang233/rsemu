import React, { useRef, useEffect, useCallback, useState } from "react";
import { onDisplayFrame } from "../../../lib/tauri";
import type { St7789SpiConfig, St7789FsmcConfig } from "../../../types/peripheral";

type St7789DisplayConfig = St7789SpiConfig | St7789FsmcConfig;

interface Props {
  config: St7789DisplayConfig;
}

export default function DisplayWidget({ config }: Props) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const [hasFrame, setHasFrame] = useState(false);

  const drawFrame = useCallback(
    (width: number, height: number, data: string) => {
      if (width !== config.width || height !== config.height) return;
      const canvas = canvasRef.current;
      if (!canvas) return;

      const binary = atob(data);
      const bytes = new Uint8Array(binary.length);
      for (let i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i);

      const rgba = new Uint8ClampedArray(width * height * 4);
      for (let i = 0; i < width * height; i++) {
        const b = bytes[i * 4 + 0];
        const g = bytes[i * 4 + 1];
        const r = bytes[i * 4 + 2];
        const a = bytes[i * 4 + 3];
        rgba[i * 4 + 0] = r;
        rgba[i * 4 + 1] = g;
        rgba[i * 4 + 2] = b;
        rgba[i * 4 + 3] = a === 0 ? 255 : a;
      }

      canvas.width = width;
      canvas.height = height;
      const ctx = canvas.getContext("2d")!;
      ctx.putImageData(new ImageData(rgba, width, height), 0, 0);
      setHasFrame(true);
    },
    [config.height, config.width]
  );

  useEffect(() => {
    let unlisten: (() => void) | null = null;
    onDisplayFrame((p) => drawFrame(p.width, p.height, p.data)).then((fn) => {
      unlisten = fn;
    });
    return () => unlisten?.();
  }, [drawFrame]);

  return (
    <>
      <div
        className="relative bg-black"
        style={{ width: config.width, height: config.height }}
      >
        <canvas
          ref={canvasRef}
          width={config.width}
          height={config.height}
          className="block"
          style={{ imageRendering: "pixelated", width: "100%", height: "100%" }}
        />
        {!hasFrame && (
          <div className="absolute inset-0 flex items-center justify-center pointer-events-none">
            <p className="text-[#45475a] text-xs">No frame yet</p>
          </div>
        )}
      </div>
      <p className="text-xs text-[#6c7086] text-center py-1">
        {config.width}×{config.height}
      </p>
    </>
  );
}
