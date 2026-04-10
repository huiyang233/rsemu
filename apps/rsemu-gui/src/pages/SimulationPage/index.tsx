import React from "react";
import { useAppStore } from "../../store/appStore";
import ControlBar from "./ControlBar";
import { typeToWidget } from "../../components/peripherals/registry";

export default function SimulationPage() {
  const canvasItems = useAppStore((s) => s.canvasItems);
  const configuredItems = canvasItems.filter((i) => i.config);

  return (
    <div className="flex flex-col h-full bg-[#1e1e2e]">
      <ControlBar />

      <main className="flex-1 overflow-auto p-5 space-y-6">
        {configuredItems.length > 0 ? (
          <div className="flex flex-wrap gap-4">
            {configuredItems.map((item) => {
              const Widget = typeToWidget.get(item.type);
              if (!Widget) return null;
              return <Widget key={item.instanceId} config={item.config!} />;
            })}
          </div>
        ) : (
          <div className="flex items-center justify-center h-40 text-[#45475a] text-sm">
            No components configured. Go back to Setup.
          </div>
        )}
      </main>
    </div>
  );
}
