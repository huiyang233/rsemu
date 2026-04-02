import React from "react";
import { useAppStore } from "../../store/appStore";
import { stopSimulation } from "../../lib/tauri";
import Button from "../../components/ui/Button";
import Badge from "../../components/ui/Badge";

export default function ControlBar() {
  const running = useAppStore((s) => s.running);
  const steps = useAppStore((s) => s.steps);
  const simError = useAppStore((s) => s.simError);
  const setPage = useAppStore((s) => s.setPage);
  const setRunning = useAppStore((s) => s.setRunning);

  const handleStop = async () => {
    await stopSimulation();
    setRunning(false);
  };

  const handleBack = async () => {
    await stopSimulation();
    setRunning(false);
    setPage("setup");
  };

  return (
    <div className="flex items-center gap-3 px-5 py-2.5 border-b border-[#3a3a5e] bg-[#181825]">
      <span className="text-lg font-bold text-indigo-400">rsemu</span>
      <span className="text-[#45475a] text-sm mr-2">GUI</span>

      <Badge color={running ? "green" : simError ? "red" : "gray"}>
        {running ? "● Running" : simError ? "● Error" : "● Stopped"}
      </Badge>

      {steps > 0 && (
        <span className="text-xs text-[#6c7086] font-mono">
          {steps.toLocaleString()} steps
        </span>
      )}

      {simError && (
        <span className="text-xs text-red-400 truncate max-w-xs" title={simError}>
          {simError}
        </span>
      )}

      <div className="ml-auto flex gap-2">
        <Button variant="danger" size="sm" onClick={handleStop} disabled={!running}>
          ■ Stop
        </Button>
        <Button variant="secondary" size="sm" onClick={handleBack}>
          ← Back to Setup
        </Button>
      </div>
    </div>
  );
}
