import React from "react";
import { useAppStore } from "../../store/appStore";
import { startSimulation } from "../../lib/tauri";
import FirmwarePicker from "./FirmwarePicker";
import SimCanvas from "../../components/canvas/SimCanvas";
import Button from "../../components/ui/Button";
import Select from "../../components/ui/Select";
import type { PeripheralConfig } from "../../types/peripheral";
import { useProjectActions } from "../../hooks/useProjectActions";

export default function SetupPage() {
  const boards = useAppStore((s) => s.boards);
  const dirty = useAppStore((s) => s.dirty);
  const projectName = useAppStore((s) => s.projectName);
  const projectPath = useAppStore((s) => s.projectPath);
  const selectedBoard = useAppStore((s) => s.selectedBoard);
  const setSelectedBoard = useAppStore((s) => s.setSelectedBoard);
  const clearCanvasItems = useAppStore((s) => s.clearCanvasItems);
  const firmwarePath = useAppStore((s) => s.firmwarePath);
  const canvasItems = useAppStore((s) => s.canvasItems);
  const setPage = useAppStore((s) => s.setPage);
  const setRunning = useAppStore((s) => s.setRunning);
  const setSimError = useAppStore((s) => s.setSimError);
  const { openProject, saveProject, saveProjectAs, createNewProject } = useProjectActions();

  const board = boards.find((b) => b.id === selectedBoard);

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
        <div className="flex items-center gap-3 min-w-0">
          <span className="text-lg font-bold text-indigo-400">rsemu</span>
          <span className="text-[#45475a] text-sm">GUI</span>
          <span className="text-[#6c7086]">|</span>
          <div className="min-w-0">
            <p className="text-sm text-[#cdd6f4] truncate">
              {projectName || "untitled"}
              {dirty && <span className="text-amber-400 ml-1">*</span>}
            </p>
            <p className="text-xs text-[#6c7086] truncate" title={projectPath ?? "unsaved project"}>
              {projectPath ?? "unsaved project"}
            </p>
          </div>
        </div>
        <div className="flex items-center gap-2">
          <Button size="sm" variant="ghost" onClick={() => setPage("welcome")}>
            Home
          </Button>
          <Button
            size="sm"
            variant="ghost"
            onClick={async () => {
              const nextName = window.prompt("New project name:", "untitled");
              if (nextName === null) return;
              await createNewProject(selectedBoard, nextName);
            }}
          >
            New
          </Button>
          <Button size="sm" variant="ghost" onClick={() => void openProject()}>
            Open
          </Button>
          <Button size="sm" variant="ghost" onClick={() => void saveProject()}>
            Save
          </Button>
          <Button size="sm" variant="ghost" onClick={() => void saveProjectAs()}>
            Save As
          </Button>
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
        <section className="rounded-lg border border-[#3a3a5e] bg-[#181825] p-3">
          <div className="flex items-end gap-3">
            <Select
              label="Target Board"
              value={selectedBoard}
              onChange={(e) => {
                const next = e.target.value;
                if (next === selectedBoard) return;
                if (canvasItems.length > 0) {
                  const ok = window.confirm(
                    "Changing board will clear current component configuration. Continue?"
                  );
                  if (!ok) return;
                  clearCanvasItems(true);
                }
                setSelectedBoard(next, true);
              }}
            >
              {boards.map((b) => (
                <option key={b.id} value={b.id}>{b.name}</option>
              ))}
            </Select>
            {board && (
              <p className="text-xs text-[#6c7086] pb-1">{board.description}</p>
            )}
          </div>
        </section>
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
