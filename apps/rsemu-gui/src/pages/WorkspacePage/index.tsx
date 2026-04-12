import React from "react";
import { useAppStore } from "../../store/appStore";
import { startSimulation, stopSimulation } from "../../lib/tauri";
import SimCanvas from "../../components/canvas/SimCanvas";
import Button from "../../components/ui/Button";
import Select from "../../components/ui/Select";
import Badge from "../../components/ui/Badge";
import { useProjectActions } from "../../hooks/useProjectActions";
import type { PeripheralConfig } from "../../types/peripheral";
import { Play, Square, Home, FilePlus, FolderOpen, Save, SaveAll, Cpu, FileCode } from "lucide-react";
import { open } from "@tauri-apps/plugin-dialog";

export default function WorkspacePage() {
  const boards = useAppStore((s) => s.boards);
  const dirty = useAppStore((s) => s.dirty);
  const projectName = useAppStore((s) => s.projectName);
  const projectPath = useAppStore((s) => s.projectPath);
  const selectedBoard = useAppStore((s) => s.selectedBoard);
  const setSelectedBoard = useAppStore((s) => s.setSelectedBoard);
  const clearCanvasItems = useAppStore((s) => s.clearCanvasItems);
  const firmwarePath = useAppStore((s) => s.firmwarePath);
  const setFirmwarePath = useAppStore((s) => s.setFirmwarePath);
  const canvasItems = useAppStore((s) => s.canvasItems);
  const running = useAppStore((s) => s.running);
  const simError = useAppStore((s) => s.simError);
  const steps = useAppStore((s) => s.steps);
  const setRunning = useAppStore((s) => s.setRunning);
  const setSimError = useAppStore((s) => s.setSimError);
  const resetSimState = useAppStore((s) => s.resetSimState);
  const setPage = useAppStore((s) => s.setPage);
  const { openProject, saveProject, saveProjectAs, createNewProject } = useProjectActions();

  const board = boards.find((b) => b.id === selectedBoard);

  const allConfigured = canvasItems.every((i) => i.config !== null);
  const canRun = !!firmwarePath && canvasItems.length > 0 && allConfigured;

  const handleToggleRun = async () => {
    if (running) {
      await stopSimulation();
      resetSimState();
    } else {
      const peripherals = canvasItems
        .map((i) => i.config)
        .filter(Boolean) as PeripheralConfig[];

      // Clear all widget states and bump simGen to force widget remount
      resetSimState();

      try {
        await startSimulation({
          board: selectedBoard,
          firmware_path: firmwarePath,
          peripherals,
        });
        setRunning(true);
      } catch (e) {
        setSimError(String(e));
      }
    }
  };

  const handlePickFirmware = async () => {
    const path = await open({
      multiple: false,
      filters: [{ name: "Firmware", extensions: ["bin", "hex", "elf"] }],
    });
    if (path) {
      setFirmwarePath(path);
    }
  };

  return (
    <div className="flex flex-col h-full bg-[#1e1e2e]">
      {/* Top bar */}
      <header className="px-5 py-2 border-b border-[#3a3a5e] bg-[#181825]">
        <div className="flex items-center gap-3">
          {/* Left: project info + file actions */}
          <div className="flex items-center gap-2 min-w-0">
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

          <div className="flex items-center gap-1">
            <Button size="sm" variant="ghost" onClick={() => setPage("welcome")} title="Home">
              <Home className="w-4 h-4" />
            </Button>
            <Button
              size="sm"
              variant="ghost"
              onClick={async () => {
                const nextName = window.prompt("New project name:", "untitled");
                if (nextName === null) return;
                await createNewProject(selectedBoard, nextName);
              }}
              title="New"
            >
              <FilePlus className="w-4 h-4" />
            </Button>
            <Button size="sm" variant="ghost" onClick={() => void openProject()} title="Open">
              <FolderOpen className="w-4 h-4" />
            </Button>
            <Button size="sm" variant="ghost" onClick={() => void saveProject()} title="Save">
              <Save className="w-4 h-4" />
            </Button>
            <Button size="sm" variant="ghost" onClick={() => void saveProjectAs()} title="Save As">
              <SaveAll className="w-4 h-4" />
            </Button>
          </div>

          {/* Spacer */}
          <div className="flex-1" />

          {/* Right: status + Play/Stop */}
          {running && (
            <Badge color="green">Running</Badge>
          )}
          {simError && (
            <span className="text-xs text-red-400 truncate max-w-xs" title={simError}>
              {simError}
            </span>
          )}
          {running && steps > 0 && (
            <span className="text-xs text-[#6c7086] font-mono">
              {steps.toLocaleString()} steps
            </span>
          )}
          {!running && !canRun && (
            <span className="text-xs text-[#6c7086]">
              {!firmwarePath
                ? "Select firmware"
                : canvasItems.length === 0
                  ? "Add a component"
                  : "Configure all components"}
            </span>
          )}
          <Button
            variant={running ? "danger" : "primary"}
            disabled={!running && !canRun}
            onClick={handleToggleRun}
            className="gap-1.5"
            title={running ? "Stop" : "Run Simulation"}
          >
            {running ? <Square className="w-3.5 h-3.5" /> : <Play className="w-3.5 h-3.5" />}
            {running ? "Stop" : "Run"}
          </Button>
        </div>

        {/* Second row: Board + Firmware, always visible */}
        <div className="flex items-center gap-4 mt-2">
          <div className="flex items-center gap-2">
            <Cpu className="w-3.5 h-3.5 text-[#6c7086]" />
            <Select
              label="Board"
              value={selectedBoard}
              disabled={running}
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
              className="py-0.5 text-xs"
            >
              {boards.map((b) => (
                <option key={b.id} value={b.id}>{b.name}</option>
              ))}
            </Select>
            {board && (
              <span className="text-xs text-[#6c7086]">{board.description}</span>
            )}
          </div>

          <div className="flex items-center gap-2">
            <FileCode className="w-3.5 h-3.5 text-[#6c7086]" />
            <span className="text-xs text-[#6c7086]">Firmware</span>
            {running ? (
              <span className="text-xs text-[#cdd6f4] font-mono truncate max-w-xs" title={firmwarePath}>
                {firmwarePath || "—"}
              </span>
            ) : (
              <>
                {firmwarePath ? (
                  <span
                    className="text-xs text-green-400 font-mono truncate max-w-xs cursor-pointer hover:underline"
                    title={firmwarePath}
                    onClick={handlePickFirmware}
                  >
                    {firmwarePath.split(/[/\\]/).pop()}
                  </span>
                ) : (
                  <span className="text-xs text-[#45475a]">No firmware selected</span>
                )}
                <Button size="sm" variant="ghost" onClick={handlePickFirmware}>
                  Browse
                </Button>
              </>
            )}
          </div>
        </div>
      </header>

      {/* Canvas */}
      <main className="flex-1 overflow-hidden">
        <SimCanvas mode={running ? "run" : "config"} />
      </main>
    </div>
  );
}
