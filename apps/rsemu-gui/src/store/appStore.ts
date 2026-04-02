import { create } from "zustand";
import type { BoardInfo } from "../types/board";
import type { CanvasItem, PeripheralConfig } from "../types/peripheral";

export type AppPage = "setup" | "simulation";

interface AppState {
  // ── Navigation ──────────────────────────────────────────────────────────
  page: AppPage;
  setPage: (page: AppPage) => void;

  // ── Setup ────────────────────────────────────────────────────────────────
  boards: BoardInfo[];
  setBoards: (boards: BoardInfo[]) => void;

  selectedBoard: string;
  setSelectedBoard: (id: string) => void;

  firmwarePath: string;
  setFirmwarePath: (path: string) => void;

  canvasItems: CanvasItem[];
  addCanvasItem: (item: CanvasItem) => void;
  removeCanvasItem: (instanceId: string) => void;
  updateCanvasItem: (instanceId: string, updates: Partial<CanvasItem>) => void;
  updateCanvasItemConfig: (instanceId: string, config: PeripheralConfig) => void;
  moveCanvasItem: (instanceId: string, position: { x: number; y: number }) => void;

  // ── Simulation runtime ───────────────────────────────────────────────────
  running: boolean;
  simError: string | null;
  steps: number;
  setRunning: (running: boolean) => void;
  setSimError: (error: string | null) => void;
  setSteps: (steps: number) => void;

  ledStates: Record<string, boolean>;
  setLedState: (id: string, on: boolean) => void;

  uartOutput: Record<string, string>;
  appendUartByte: (peripheral: string, byte: number) => void;
  clearUartOutput: (peripheral: string) => void;
}

export const useAppStore = create<AppState>((set) => ({
  // ── Navigation ──────────────────────────────────────────────────────────
  page: "setup",
  setPage: (page) => set({ page }),

  // ── Setup ────────────────────────────────────────────────────────────────
  boards: [],
  setBoards: (boards) => set({ boards }),

  selectedBoard: "stm32f103",
  setSelectedBoard: (id) => set({ selectedBoard: id }),

  firmwarePath: "",
  setFirmwarePath: (firmwarePath) => set({ firmwarePath }),

  canvasItems: [],
  addCanvasItem: (item) =>
    set((s) => ({ canvasItems: [...s.canvasItems, item] })),
  removeCanvasItem: (instanceId) =>
    set((s) => ({ canvasItems: s.canvasItems.filter((i) => i.instanceId !== instanceId) })),
  updateCanvasItem: (instanceId, updates) =>
    set((s) => ({
      canvasItems: s.canvasItems.map((i) =>
        i.instanceId === instanceId ? { ...i, ...updates } : i
      ),
    })),
  updateCanvasItemConfig: (instanceId, config) =>
    set((s) => ({
      canvasItems: s.canvasItems.map((i) =>
        i.instanceId === instanceId ? { ...i, config } : i
      ),
    })),
  moveCanvasItem: (instanceId, position) =>
    set((s) => ({
      canvasItems: s.canvasItems.map((i) =>
        i.instanceId === instanceId ? { ...i, position } : i
      ),
    })),

  // ── Simulation runtime ───────────────────────────────────────────────────
  running: false,
  simError: null,
  steps: 0,
  setRunning: (running) => set({ running }),
  setSimError: (simError) => set({ simError }),
  setSteps: (steps) => set({ steps }),

  ledStates: {},
  setLedState: (id, on) =>
    set((s) => ({ ledStates: { ...s.ledStates, [id]: on } })),

  uartOutput: {},
  appendUartByte: (peripheral, byte) =>
    set((s) => ({
      uartOutput: {
        ...s.uartOutput,
        [peripheral]: (s.uartOutput[peripheral] ?? "") + String.fromCharCode(byte),
      },
    })),
  clearUartOutput: (peripheral) =>
    set((s) => ({ uartOutput: { ...s.uartOutput, [peripheral]: "" } })),
}));
