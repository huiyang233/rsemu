import React from "react";
import { useDraggable } from "@dnd-kit/core";
import { CSS } from "@dnd-kit/utilities";
import { PERIPHERAL_DEFS, PeripheralTypeDef } from "../../lib/peripheralDefs";
import type { PeripheralType } from "../../types/peripheral";

function PaletteItem({ def }: { def: PeripheralTypeDef }) {
  const { attributes, listeners, setNodeRef, transform, isDragging } = useDraggable({
    id: `palette:${def.type}`,
    data: { type: def.type as PeripheralType },
  });

  const style = {
    transform: CSS.Translate.toString(transform),
    opacity: isDragging ? 0.5 : 1,
  };

  return (
    <div
      ref={setNodeRef}
      style={style}
      {...listeners}
      {...attributes}
      className={`
        flex items-center gap-2 px-3 py-2 rounded cursor-grab active:cursor-grabbing
        border border-[#3a3a5e] select-none
        ${def.color} ${def.textColor}
        hover:brightness-110 transition-all
      `}
    >
      <span className="text-lg">{def.icon}</span>
      <span className="text-sm font-medium">{def.label}</span>
    </div>
  );
}

export default function PeripheralPalette() {
  return (
    <div className="flex flex-col gap-2 w-44 shrink-0">
      <p className="text-xs text-[#6c7086] uppercase tracking-wider mb-1 px-1">
        Components
      </p>
      {PERIPHERAL_DEFS.map((def) => (
        <PaletteItem key={def.type} def={def} />
      ))}
      <p className="text-xs text-[#6c7086] mt-2 px-1">
        Drag onto canvas →
      </p>
    </div>
  );
}
