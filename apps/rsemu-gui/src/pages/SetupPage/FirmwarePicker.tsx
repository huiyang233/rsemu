import React from "react";
import { useAppStore } from "../../store/appStore";
import { openFirmwareDialog } from "../../lib/tauri";
import Button from "../../components/ui/Button";

export default function FirmwarePicker() {
  const path = useAppStore((s) => s.firmwarePath);
  const setPath = useAppStore((s) => s.setFirmwarePath);

  const handleBrowse = async () => {
    const p = await openFirmwareDialog();
    if (p) setPath(p);
  };

  return (
    <div className="space-y-2">
      <p className="text-xs text-[#6c7086] uppercase tracking-wider">Firmware</p>
      <div className="flex items-center gap-2">
        <div
          className={`
            flex-1 px-3 py-2 rounded border text-sm truncate
            ${path
              ? "border-green-700 bg-green-950/30 text-green-300"
              : "border-[#3a3a5e] bg-[#2a2a3e] text-[#45475a]"
            }
          `}
          title={path}
        >
          {path || "No firmware selected…"}
        </div>
        <Button variant="secondary" onClick={handleBrowse}>
          Browse .bin
        </Button>
      </div>
    </div>
  );
}
