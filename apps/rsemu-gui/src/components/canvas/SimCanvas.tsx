import React, { useCallback, useRef, useState } from "react";
import { DndContext, DragEndEvent, useDroppable } from "@dnd-kit/core";
import { useAppStore } from "../../store/appStore";
import type { CanvasItem, PeripheralConfig, PeripheralType } from "../../types/peripheral";
import PeripheralNode from "./PeripheralNode";
import PeripheralPalette from "./PeripheralPalette";
import PinConfigPanel from "./PinConfigPanel";

let instanceCounter = 0;
function newInstanceId() {
  return `inst_${Date.now()}_${instanceCounter++}`;
}

function DropZone({ children }: { children: React.ReactNode }) {
  const { setNodeRef, isOver } = useDroppable({ id: "canvas-drop" });
  return (
    <div
      ref={setNodeRef}
      className={`
        relative flex-1 rounded-lg border-2 border-dashed min-h-[400px] overflow-hidden
        transition-colors
        ${isOver ? "border-indigo-500 bg-indigo-950/20" : "border-[#3a3a5e] bg-[#181825]"}
      `}
    >
      {children}
      {/* Grid overlay */}
      <svg
        className="absolute inset-0 pointer-events-none opacity-10"
        style={{ width: "100%", height: "100%" }}
      >
        <defs>
          <pattern id="grid" width="24" height="24" patternUnits="userSpaceOnUse">
            <path d="M 24 0 L 0 0 0 24" fill="none" stroke="#6c7086" strokeWidth="0.5" />
          </pattern>
        </defs>
        <rect width="100%" height="100%" fill="url(#grid)" />
      </svg>
    </div>
  );
}

export default function SimCanvas() {
  const boards = useAppStore((s) => s.boards);
  const selectedBoard = useAppStore((s) => s.selectedBoard);
  const board = boards.find((b) => b.id === selectedBoard);

  const canvasItems = useAppStore((s) => s.canvasItems);
  const addCanvasItem = useAppStore((s) => s.addCanvasItem);
  const removeCanvasItem = useAppStore((s) => s.removeCanvasItem);
  const updateCanvasItemConfig = useAppStore((s) => s.updateCanvasItemConfig);
  const moveCanvasItem = useAppStore((s) => s.moveCanvasItem);

  const [configTarget, setConfigTarget] = useState<CanvasItem | null>(null);
  const canvasRef = useRef<HTMLDivElement>(null);

  const handleDragEnd = useCallback(
    (event: DragEndEvent) => {
      const { active, over } = event;
      if (!over || over.id !== "canvas-drop") return;
      if (!active.id.toString().startsWith("palette:")) return;

      const rect = canvasRef.current?.getBoundingClientRect();
      const type = (active.data.current?.type as PeripheralType) ?? "led";
      const drop = event.delta;

      addCanvasItem({
        instanceId: newInstanceId(),
        type,
        position: {
          x: Math.max(8, (rect ? 80 : 80) + drop.x),
          y: Math.max(8, 80 + drop.y),
        },
        simPosition: null,
        config: null,
      });
    },
    [addCanvasItem]
  );

  const handleSaveConfig = useCallback(
    (instanceId: string, config: PeripheralConfig) => {
      updateCanvasItemConfig(instanceId, config);
    },
    [updateCanvasItemConfig]
  );

  return (
    <DndContext onDragEnd={handleDragEnd}>
      <div className="flex gap-4 h-full" ref={canvasRef}>
        <PeripheralPalette />

        <DropZone>
          {canvasItems.length === 0 && (
            <p className="absolute inset-0 flex items-center justify-center text-[#45475a] text-sm pointer-events-none">
              Drag components here to build your board
            </p>
          )}
          {canvasItems.map((item) => (
            <PeripheralNode
              key={item.instanceId}
              item={item}
              onConfigure={setConfigTarget}
              onRemove={removeCanvasItem}
              onMove={moveCanvasItem}
            />
          ))}
        </DropZone>
      </div>

      {configTarget && board && (
        <PinConfigPanel
          item={configTarget}
          board={board}
          onSave={handleSaveConfig}
          onClose={() => setConfigTarget(null)}
        />
      )}
    </DndContext>
  );
}
