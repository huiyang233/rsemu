import React, { useCallback, useRef, useState } from "react";
import { DndContext, DragEndEvent, useDroppable } from "@dnd-kit/core";
import { useAppStore } from "../../store/appStore";
import type { CanvasItem, PeripheralConfig, PeripheralType } from "../../types/peripheral";
import { getPeripheralDef, typeToWidget } from "../peripherals/registry";
import PeripheralPalette from "./PeripheralPalette";
import PinConfigPanel from "./PinConfigPanel";
import { Settings, Trash2, AlertTriangle } from "lucide-react";

let instanceCounter = 0;
function newInstanceId() {
  return `inst_${Date.now()}_${instanceCounter++}`;
}

interface SimCanvasProps {
  mode: "config" | "run";
}

function DropZone({ isConfig, children }: { isConfig: boolean; children: React.ReactNode }) {
  const { setNodeRef, isOver } = useDroppable({ id: "canvas-drop" });
  return (
    <div
      ref={setNodeRef}
      className={`
        relative flex-1 rounded-lg min-h-[400px] overflow-auto
        transition-colors
        ${isConfig
          ? `border-2 border-dashed ${isOver ? "border-indigo-500 bg-indigo-950/20" : "border-[#3a3a5e] bg-[#181825]"}`
          : "bg-[#181825]"
        }
      `}
    >
      {children}
      {isConfig && (
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
      )}
    </div>
  );
}

function CanvasItemFrame({
  item,
  isConfig,
  onConfigure,
  onRemove,
  onMove,
}: {
  item: CanvasItem;
  isConfig: boolean;
  onConfigure: (item: CanvasItem) => void;
  onRemove: (id: string) => void;
  onMove: (id: string, pos: { x: number; y: number }) => void;
}) {
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

  if (!def) {
    return (
      <div
        style={{ left: item.position.x, top: item.position.y }}
        className="absolute w-36 rounded border border-red-500/50 bg-red-900/30 select-none cursor-move p-2"
        onMouseDown={handleMouseDown}
      >
        <p className="text-xs text-red-400">Unknown: {item.type}</p>
        {isConfig && (
          <button onClick={() => onRemove(item.instanceId)} className="text-xs text-[#6c7086] hover:text-red-400 mt-1">
            Remove
          </button>
        )}
      </div>
    );
  }

  const configured = item.config !== null;
  const Icon = def.icon;
  const Widget = typeToWidget.get(item.type);

  return (
    <div
      style={{ left: item.position.x, top: item.position.y }}
      className="absolute rounded-lg border border-[#3a3a5e] bg-[#2a2a3e] shadow-lg overflow-hidden select-none"
    >
      {/* Title bar - drag handle */}
      <div
        className="flex items-center gap-2 px-3 py-1.5 bg-[#313244] border-b border-[#3a3a5e] cursor-grab active:cursor-grabbing"
        onMouseDown={handleMouseDown}
      >
        <Icon className="w-3.5 h-3.5 text-[#a6adc8]" />
        <span className="text-xs font-medium text-[#cdd6f4] flex-1">{def.label}</span>

        {/* Config-mode controls */}
        {isConfig && (
          <>
            {!configured && (
              <button
                className="text-amber-400 hover:text-amber-300 transition-colors"
                title="Not configured"
              >
                <AlertTriangle className="w-3.5 h-3.5" />
              </button>
            )}
            <button
              onClick={() => onConfigure(item)}
              className="text-[#6c7086] hover:text-[#cdd6f4] transition-colors"
              title="Configure"
            >
              <Settings className="w-3.5 h-3.5" />
            </button>
            <button
              onClick={() => onRemove(item.instanceId)}
              className="text-[#6c7086] hover:text-red-400 transition-colors"
              title="Remove"
            >
              <Trash2 className="w-3.5 h-3.5" />
            </button>
          </>
        )}

        <span className="text-[#585b70] text-xs cursor-grab">⠿</span>
      </div>

      {/* Widget content — always show if configured */}
      {configured && Widget ? (
        <Widget config={item.config!} interactive={!isConfig} />
      ) : !configured ? (
        <div className="px-3 py-4">
          <p className="text-xs text-[#585b70] text-center">
            Click <Settings className="w-3 h-3 inline" /> to configure
          </p>
        </div>
      ) : null}
    </div>
  );
}

export default function SimCanvas({ mode }: SimCanvasProps) {
  const boards = useAppStore((s) => s.boards);
  const selectedBoard = useAppStore((s) => s.selectedBoard);
  const board = boards.find((b) => b.id === selectedBoard);

  const canvasItems = useAppStore((s) => s.canvasItems);
  const addCanvasItem = useAppStore((s) => s.addCanvasItem);
  const removeCanvasItem = useAppStore((s) => s.removeCanvasItem);
  const updateCanvasItemConfig = useAppStore((s) => s.updateCanvasItemConfig);
  const moveCanvasItem = useAppStore((s) => s.moveCanvasItem);
  const simGen = useAppStore((s) => s.simGen);

  const [configTarget, setConfigTarget] = useState<CanvasItem | null>(null);
  const canvasRef = useRef<HTMLDivElement>(null);

  const isConfig = mode === "config";

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

  const handleMoveItem = useCallback(
    (id: string, pos: { x: number; y: number }) => {
      moveCanvasItem(id, pos, false);
    },
    [moveCanvasItem]
  );

  return (
    <DndContext onDragEnd={isConfig ? handleDragEnd : undefined}>
      <div className="flex gap-4 h-full" ref={canvasRef}>
        {isConfig && <PeripheralPalette />}

        <DropZone isConfig={isConfig}>
          {canvasItems.length === 0 && isConfig && (
            <p className="absolute inset-0 flex items-center justify-center text-[#45475a] text-sm pointer-events-none">
              Drag components here to build your board
            </p>
          )}
          {canvasItems.map((item) => (
            <CanvasItemFrame
              key={`${simGen}:${item.instanceId}`}
              item={item}
              isConfig={isConfig}
              onConfigure={setConfigTarget}
              onRemove={removeCanvasItem}
              onMove={isConfig ? moveCanvasItem : handleMoveItem}
            />
          ))}
        </DropZone>
      </div>

      {isConfig && configTarget && board && (
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
