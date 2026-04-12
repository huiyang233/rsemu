import React, { useCallback, useRef } from "react";
import type { CanvasItem } from "../../types/peripheral";
import type { LucideIcon } from "lucide-react";

const GRID = 16;
const snap = (v: number) => Math.round(v / GRID) * GRID;

interface WidgetFrameProps {
  item: CanvasItem;
  label: string;
  icon: LucideIcon;
  onMove: (id: string, pos: { x: number; y: number }) => void;
  headerExtra?: React.ReactNode;
  children: React.ReactNode;
}

export default function WidgetFrame({ item, label, icon: Icon, onMove, headerExtra, children }: WidgetFrameProps) {
  const pos = item.position;
  const startPos = useRef<{ mouseX: number; mouseY: number; itemX: number; itemY: number } | null>(null);

  const handleMouseDown = useCallback(
    (e: React.MouseEvent) => {
      e.preventDefault();
      startPos.current = {
        mouseX: e.clientX,
        mouseY: e.clientY,
        itemX: pos.x,
        itemY: pos.y,
      };

      const onMouseMove = (me: MouseEvent) => {
        if (!startPos.current) return;
        const dx = me.clientX - startPos.current.mouseX;
        const dy = me.clientY - startPos.current.mouseY;
        onMove(item.instanceId, {
          x: snap(Math.max(0, startPos.current.itemX + dx)),
          y: snap(Math.max(0, startPos.current.itemY + dy)),
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
    [item.instanceId, onMove, pos.x, pos.y]
  );

  return (
    <div
      style={{ left: snap(pos.x), top: snap(pos.y) }}
      className="absolute rounded-lg border border-[#3a3a5e] bg-[#2a2a3e] shadow-lg overflow-hidden select-none"
    >
      {/* Title bar - drag handle */}
      <div
        className="flex items-center gap-2 px-3 py-1.5 bg-[#313244] border-b border-[#3a3a5e] cursor-grab active:cursor-grabbing"
        onMouseDown={handleMouseDown}
      >
        <Icon className="w-3.5 h-3.5 text-[#a6adc8]" />
        <span className="text-xs font-medium text-[#cdd6f4] flex-1">{label}</span>
        {headerExtra}
        <span className="text-[#585b70] text-xs cursor-grab">⠿</span>
      </div>

      {/* Widget content */}
      <div>{children}</div>
    </div>
  );
}
