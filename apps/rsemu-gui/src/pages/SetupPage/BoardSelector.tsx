import React from "react";
import { useAppStore } from "../../store/appStore";
import type { BoardInfo } from "../../types/board";
import Badge from "../../components/ui/Badge";

interface Props {
  boards: BoardInfo[];
}

export default function BoardSelector({ boards }: Props) {
  const selected = useAppStore((s) => s.selectedBoard);
  const setSelected = useAppStore((s) => s.setSelectedBoard);

  return (
    <div className="space-y-2">
      <p className="text-xs text-[#6c7086] uppercase tracking-wider">Select MCU Board</p>
      <div className="flex gap-3">
        {boards.map((b) => (
          <button
            key={b.id}
            onClick={() => setSelected(b.id)}
            className={`
              flex-1 text-left px-4 py-3 rounded-lg border transition-all
              ${selected === b.id
                ? "border-indigo-500 bg-indigo-950/40 text-[#cdd6f4]"
                : "border-[#3a3a5e] bg-[#2a2a3e] text-[#a6adc8] hover:border-[#6c7086]"
              }
            `}
          >
            <p className="font-semibold text-sm">{b.name}</p>
            <p className="text-xs text-[#6c7086] mt-0.5">{b.description}</p>
            <div className="flex gap-1 mt-2">
              <Badge color="indigo">{b.flash_kb} KB Flash</Badge>
              <Badge color="gray">{b.ram_kb} KB RAM</Badge>
            </div>
          </button>
        ))}
      </div>
    </div>
  );
}
