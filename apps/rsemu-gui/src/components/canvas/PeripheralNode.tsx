import React, { useCallback, useRef } from "react";
import type { CanvasItem } from "../../types/peripheral";
import { getPeripheralDef } from "../../lib/peripheralDefs";
import Button from "../ui/Button";

interface Props {
  item: CanvasItem;
  onConfigure: (item: CanvasItem) => void;
  onRemove: (instanceId: string) => void;
  onMove: (instanceId: string, pos: { x: number; y: number }) => void;
}

export default function PeripheralNode({ item, onConfigure, onRemove, onMove }: Props) {
  const def = getPeripheralDef(item.type);
  const startPos = useRef<{ mouseX: number; mouseY: number; itemX: number; itemY: number } | null>(null);

  const handleMouseDown = useCallback(
    (e: React.MouseEvent) => {
      if ((e.target as HTMLElement).closest("button")) return;
      e.preventDefault();
      startPos.current = {
        mouseX: e.clientX,
        mouseY: e.clientY,
        itemX: item.position.x,
        itemY: item.position.y,
      };

      const onMouseMove = (me: MouseEvent) => {
        if (!startPos.current) return;
        const dx = me.clientX - startPos.current.mouseX;
        const dy = me.clientY - startPos.current.mouseY;
        onMove(item.instanceId, {
          x: Math.max(0, startPos.current.itemX + dx),
          y: Math.max(0, startPos.current.itemY + dy),
        });
      };

      const onMouseUp = () => {
        startPos.current = null;
        window.removeEventListener("mousemove", onMouseMove);
        window.removeEventListener("mouseup", onMouseUp);
      };

      window.addEventListener("mousemove", onMouseMove);
      window.addEventListener("mouseup", onMouseUp);
    },
    [item, onMove]
  );

  const configured = item.config !== null;

  return (
    <div
      style={{ left: item.position.x, top: item.position.y }}
      className={`
        absolute w-36 rounded border select-none cursor-move
        ${def.color} border-[#3a3a5e]
        shadow-lg hover:shadow-xl transition-shadow
      `}
      onMouseDown={handleMouseDown}
    >
      {/* Header */}
      <div className={`flex items-center justify-between px-2 py-1.5 ${def.textColor}`}>
        <span className="text-sm font-semibold flex items-center gap-1">
          <span>{def.icon}</span>
          <span>{def.label}</span>
        </span>
        <button
          onClick={() => onRemove(item.instanceId)}
          className="text-[#6c7086] hover:text-red-400 transition-colors text-xs leading-none ml-1"
          title="Remove"
        >
          ✕
        </button>
      </div>

      {/* Body */}
      <div className="px-2 pb-2 space-y-1">
        {configured ? (
          <p className="text-xs text-green-400">✓ Configured</p>
        ) : (
          <p className="text-xs text-yellow-400">⚠ Not configured</p>
        )}
        <Button
          variant="ghost"
          size="sm"
          className="w-full justify-center border border-[#3a3a5e] mt-1"
          onClick={() => onConfigure(item)}
        >
          Configure pins
        </Button>
      </div>
    </div>
  );
}
