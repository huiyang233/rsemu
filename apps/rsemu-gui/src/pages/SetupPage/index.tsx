import React, { useEffect } from "react";
import { useAppStore } from "../../store/appStore";
import { getBoards, startSimulation } from "../../lib/tauri";
import BoardSelector from "./BoardSelector";
import FirmwarePicker from "./FirmwarePicker";
import SimCanvas from "../../components/canvas/SimCanvas";
import Button from "../../components/ui/Button";
import type { PeripheralConfig } from "../../types/peripheral";

export default function SetupPage() {
  const boards = useAppStore((s) => s.boards);
  const setBoards = useAppStore((s) => s.setBoards);
  const selectedBoard = useAppStore((s) => s.selectedBoard);
  const firmwarePath = useAppStore((s) => s.firmwarePath);
  const canvasItems = useAppStore((s) => s.canvasItems);
  const setPage = useAppStore((s) => s.setPage);
  const setRunning = useAppStore((s) => s.setRunning);
  const setSimError = useAppStore((s) => s.setSimError);

  useEffect(() => {
    getBoards().then(setBoards).catch(console.error);
  }, [setBoards]);

  const allConfigured = canvasItems.every((i) => i.config !== null);
  const canRun = !!firmwarePath && canvasItems.length > 0 && allConfigured;

  const handleRun = async () => {
    const peripherals = canvasItems
      .map((i) => i.config)
      .filter(Boolean) as PeripheralConfig[];

    setSimError(null);
    setRunning(false);

    try {
      await startSimulation({
        board: selectedBoard,
        firmware_path: firmwarePath,
        peripherals,
      });
      setPage("simulation");
    } catch (e) {
      setSimError(String(e));
    }
  };

  return (
    <div className="flex flex-col h-full bg-[#1e1e2e]">
      {/* Top bar */}
      <header className="flex items-center justify-between px-5 py-3 border-b border-[#3a3a5e] bg-[#181825]">
        <div className="flex items-center gap-2">
          <span className="text-lg font-bold text-indigo-400">rsemu</span>
          <span className="text-[#45475a] text-sm">GUI</span>
        </div>
        <div className="flex items-center gap-2">
          {!canRun && (
            <span className="text-xs text-[#6c7086]">
              {!firmwarePath
                ? "Select a firmware file"
                : canvasItems.length === 0
                  ? "Add at least one component"
                  : "Configure all components"}
            </span>
          )}
          <Button
            variant="primary"
            disabled={!canRun}
            onClick={handleRun}
            className="gap-2"
          >
            ▶ Run Simulation
          </Button>
        </div>
      </header>

      {/* Body */}
      <main className="flex-1 flex flex-col gap-4 overflow-hidden p-5">
        {boards.length > 0 && <BoardSelector boards={boards} />}
        <FirmwarePicker />

        <div className="flex-1 flex flex-col gap-1 overflow-hidden">
          <p className="text-xs text-[#6c7086] uppercase tracking-wider">
            Board Layout
            {canvasItems.length > 0 && (
              <span className="ml-2 normal-case text-[#45475a]">
                ({canvasItems.length} component{canvasItems.length !== 1 ? "s" : ""}
                {!allConfigured ? " — some unconfigured" : ""})
              </span>
            )}
          </p>
          <div className="flex-1 overflow-hidden">
            <SimCanvas />
          </div>
        </div>
      </main>
    </div>
  );
}
