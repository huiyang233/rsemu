import React from "react";
import { useDraggable } from "@dnd-kit/core";
import { CSS } from "@dnd-kit/utilities";
import { paletteEntries, categoryMeta, categoryOrder } from "../peripherals/registry";
import type { PeripheralType } from "../../types/peripheral";
import type { PeripheralCategory } from "../peripherals/registry";
import type { LucideIcon } from "lucide-react";

interface PaletteEntry {
  type: PeripheralType;
  label: string;
  color: string;
  textColor: string;
  icon: LucideIcon;
  category: PeripheralCategory;
}

function PaletteItem({ entry }: { entry: PaletteEntry }) {
  const { attributes, listeners, setNodeRef, transform, isDragging } = useDraggable({
    id: `palette:${entry.type}`,
    data: { type: entry.type as PeripheralType },
  });

  const style = {
    transform: CSS.Translate.toString(transform),
    opacity: isDragging ? 0.5 : 1,
  };

  const Icon = entry.icon;

  return (
    <div
      ref={setNodeRef}
      style={style}
      {...listeners}
      {...attributes}
      className={`
        flex items-center gap-2 px-3 py-2 rounded cursor-grab active:cursor-grabbing
        border border-[#3a3a5e] select-none
        ${entry.color} ${entry.textColor}
        hover:brightness-110 transition-all
      `}
    >
      <Icon className="w-4 h-4" />
      <span className="text-sm font-medium">{entry.label}</span>
    </div>
  );
}

export default function PeripheralPalette() {
  return (
    <div className="flex flex-col gap-3 w-48 shrink-0">
      <p className="text-xs text-[#6c7086] uppercase tracking-wider px-1">
        Components
      </p>
      {categoryOrder.map((cat) => {
        const meta = categoryMeta[cat];
        const entries = paletteEntries.filter((e) => e.category === cat);
        if (entries.length === 0) return null;
        const CatIcon = meta.icon;
        return (
          <div key={cat} className="space-y-1.5">
            <p className="text-xs text-[#585b70] uppercase tracking-wider px-1 flex items-center gap-1.5">
              <CatIcon className="w-3 h-3" />
              {meta.label}
            </p>
            {entries.map((entry) => (
              <PaletteItem key={entry.type} entry={entry as PaletteEntry} />
            ))}
          </div>
        );
      })}
      <p className="text-xs text-[#6c7086] mt-2 px-1">
        Drag onto canvas
      </p>
    </div>
  );
}
