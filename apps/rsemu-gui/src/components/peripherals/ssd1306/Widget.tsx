import React, { useRef, useEffect, useCallback } from "react";
import { onDisplayFrame } from "../../../lib/tauri";
import type { Ssd1306I2cConfig } from "../../../types/peripheral";

interface Props {
  config: Ssd1306I2cConfig;
}

function normalizeSize(width: number, height: number): [number, number] {
  const w = [64, 72, 96, 128].includes(width) ? width : 128;
  const h = [32, 40, 48, 64].includes(height) ? height : 64;
  return [w, h];
}

export default function Ssd1306Widget({ config }: Props) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const [cfgW, cfgH] = normalizeSize(config.width, config.height);

  const drawFrame = useCallback(
    (width: number, height: number, data: string) => {
      if (width !== cfgW || height !== cfgH) return;
      const canvas = canvasRef.current;
      if (!canvas) return;

      // Decode base64 → bytes (RGBA, 4 bytes per pixel)
      const binary = atob(data);
      const bytes = new Uint8Array(binary.length);
      for (let i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i);

      const rgba = new Uint8ClampedArray(bytes.buffer, bytes.byteOffset, bytes.byteLength);

      canvas.width = width;
      canvas.height = height;
      const ctx = canvas.getContext("2d");
      if (!ctx) return;
      ctx.putImageData(new ImageData(rgba, width, height), 0, 0);
    },
    [cfgH, cfgW]
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
        className="relative bg-black p-1"
        style={{ width: cfgW * 2, height: cfgH * 2 }}
      >
        <canvas
          ref={canvasRef}
          width={cfgW}
          height={cfgH}
          className="block"
          style={{ imageRendering: "pixelated", width: "100%", height: "100%" }}
        />
      </div>
      <p className="text-xs text-[#6c7086] text-center py-1">
        {cfgW}×{cfgH} @ 0x{config.address.toString(16)}
      </p>
    </>
  );
}
