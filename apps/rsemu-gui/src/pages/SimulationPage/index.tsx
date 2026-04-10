import React, { useEffect, useRef } from "react";
import { useAppStore } from "../../store/appStore";
import ControlBar from "./ControlBar";
import WidgetFrame from "./WidgetFrame";
import { typeToWidget, getPeripheralDef } from "../../components/peripherals/registry";

const GRID = 16;
const COL_WIDTH = 320;
const ROW_HEIGHT = 320;

/** Auto-layout: arrange items in a grid starting at (16, 16) */
function autoLayout(count: number): { x: number; y: number }[] {
  const cols = Math.max(1, Math.floor((1200 - 32) / COL_WIDTH));
  return Array.from({ length: count }, (_, i) => ({
    x: snap(16 + (i % cols) * COL_WIDTH),
    y: snap(16 + Math.floor(i / cols) * ROW_HEIGHT),
  }));
}

const snap = (v: number) => Math.round(v / GRID) * GRID;

export default function SimulationPage() {
  const canvasItems = useAppStore((s) => s.canvasItems);
  const moveSimItem = useAppStore((s) => s.moveSimItem);
  const updateCanvasItem = useAppStore((s) => s.updateCanvasItem);
  const configuredItems = canvasItems.filter((i) => i.config);
  const initializedRef = useRef(false);

  // First render: assign simPosition to items that don't have one
  useEffect(() => {
    if (initializedRef.current) return;
    const needsLayout = configuredItems.filter((i) => !i.simPosition);
    if (needsLayout.length === 0) return;
    initializedRef.current = true;

    const positions = autoLayout(needsLayout.length);
    needsLayout.forEach((item, idx) => {
      updateCanvasItem(item.instanceId, { simPosition: positions[idx] }, false);
    });
  }, [configuredItems, updateCanvasItem]);

  return (
    <div className="flex flex-col h-full bg-[#1e1e2e]">
      <ControlBar />

      <main className="flex-1 overflow-auto relative">
        {/* Grid background */}
        <svg
          className="absolute inset-0 pointer-events-none opacity-10"
          style={{ width: "100%", height: "100%" }}
        >
          <defs>
            <pattern id="sim-grid" width="16" height="16" patternUnits="userSpaceOnUse">
              <path d="M 16 0 L 0 0 0 16" fill="none" stroke="#6c7086" strokeWidth="0.5" />
            </pattern>
          </defs>
          <rect width="100%" height="100%" fill="url(#sim-grid)" />
        </svg>

        <div className="relative" style={{ minWidth: 1200, minHeight: 800 }}>
          {configuredItems.length > 0 ? (
            configuredItems.map((item) => {
              const Widget = typeToWidget.get(item.type);
              const def = getPeripheralDef(item.type);
              if (!Widget || !def) return null;
              return (
                <WidgetFrame
                  key={item.instanceId}
                  item={item}
                  label={def.label}
                  icon={def.icon}
                  onMove={moveSimItem}
                >
                  <Widget config={item.config!} />
                </WidgetFrame>
              );
            })
          ) : (
            <div className="absolute inset-0 flex items-center justify-center text-[#45475a] text-sm pointer-events-none">
              No components configured. Go back to Setup.
            </div>
          )}
        </div>
      </main>
    </div>
  );
}
