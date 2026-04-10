import { create } from "zustand";
import type { BoardInfo } from "../types/board";
import type { CanvasItem, PeripheralConfig } from "../types/peripheral";

export type AppPage = "welcome" | "setup" | "simulation";

interface AppState {
  // ── Navigation ──────────────────────────────────────────────────────────
  page: AppPage;
  setPage: (page: AppPage) => void;

  // ── Setup ────────────────────────────────────────────────────────────────
  boards: BoardInfo[];
  setBoards: (boards: BoardInfo[]) => void;

  projectPath: string | null;
  projectName: string;
  projectCreatedAt: string | null;
  recentProjects: string[];
  dirty: boolean;
  setProjectMeta: (projectPath: string | null, projectName: string, createdAt?: string | null) => void;
  setRecentProjects: (paths: string[]) => void;
  setDirty: (dirty: boolean) => void;

  selectedBoard: string;
  setSelectedBoard: (id: string, markDirty?: boolean) => void;

  firmwarePath: string;
  setFirmwarePath: (path: string, markDirty?: boolean) => void;

  canvasItems: CanvasItem[];
  setCanvasItems: (items: CanvasItem[], markDirty?: boolean) => void;
  clearCanvasItems: (markDirty?: boolean) => void;
  addCanvasItem: (item: CanvasItem, markDirty?: boolean) => void;
  removeCanvasItem: (instanceId: string, markDirty?: boolean) => void;
  updateCanvasItem: (instanceId: string, updates: Partial<CanvasItem>, markDirty?: boolean) => void;
  updateCanvasItemConfig: (instanceId: string, config: PeripheralConfig, markDirty?: boolean) => void;
  moveCanvasItem: (instanceId: string, position: { x: number; y: number }, markDirty?: boolean) => void;
  moveSimItem: (instanceId: string, simPosition: { x: number; y: number }) => void;

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
  appendUartBytes: (peripheral: string, bytes: number[]) => void;
  clearUartOutput: (peripheral: string) => void;
}

export const useAppStore = create<AppState>((set) => ({
  // ── Navigation ──────────────────────────────────────────────────────────
  page: "welcome",
  setPage: (page) => set({ page }),

  // ── Setup ────────────────────────────────────────────────────────────────
  boards: [],
  setBoards: (boards) => set({ boards }),

  projectPath: null,
  projectName: "",
  projectCreatedAt: null,
  recentProjects: [],
  dirty: false,
  setProjectMeta: (projectPath, projectName, createdAt = null) =>
    set({ projectPath, projectName, projectCreatedAt: createdAt }),
  setRecentProjects: (recentProjects) => set({ recentProjects }),
  setDirty: (dirty) => set({ dirty }),

  selectedBoard: "stm32f103",
  setSelectedBoard: (id, markDirty = true) =>
    set((s) => ({ selectedBoard: id, dirty: s.dirty || markDirty })),

  firmwarePath: "",
  setFirmwarePath: (firmwarePath, markDirty = true) =>
    set((s) => ({ firmwarePath, dirty: s.dirty || markDirty })),

  canvasItems: [],
  setCanvasItems: (canvasItems, markDirty = true) =>
    set((s) => ({ canvasItems, dirty: s.dirty || markDirty })),
  clearCanvasItems: (markDirty = true) =>
    set((s) => ({ canvasItems: [], dirty: s.dirty || markDirty })),
  addCanvasItem: (item, markDirty = true) =>
    set((s) => ({ canvasItems: [...s.canvasItems, item], dirty: s.dirty || markDirty })),
  removeCanvasItem: (instanceId, markDirty = true) =>
    set((s) => ({
      canvasItems: s.canvasItems.filter((i) => i.instanceId !== instanceId),
      dirty: s.dirty || markDirty,
    })),
  updateCanvasItem: (instanceId, updates, markDirty = true) =>
    set((s) => ({
      canvasItems: s.canvasItems.map((i) =>
        i.instanceId === instanceId ? { ...i, ...updates } : i
      ),
      dirty: s.dirty || markDirty,
    })),
  updateCanvasItemConfig: (instanceId, config, markDirty = true) =>
    set((s) => ({
      canvasItems: s.canvasItems.map((i) =>
        i.instanceId === instanceId ? { ...i, config } : i
      ),
      dirty: s.dirty || markDirty,
    })),
  moveCanvasItem: (instanceId, position, markDirty = true) =>
    set((s) => ({
      canvasItems: s.canvasItems.map((i) =>
        i.instanceId === instanceId ? { ...i, position } : i
      ),
      dirty: s.dirty || markDirty,
    })),
  moveSimItem: (instanceId, simPosition) =>
    set((s) => ({
      canvasItems: s.canvasItems.map((i) =>
        i.instanceId === instanceId ? { ...i, simPosition } : i
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
  appendUartBytes: (peripheral, bytes) =>
    set((s) => {
      const MAX_UART_LEN = 65536;
      const prev = s.uartOutput[peripheral] ?? "";
      const chunk = String.fromCharCode(...bytes);
      const next = (prev + chunk).slice(-MAX_UART_LEN);
      return { uartOutput: { ...s.uartOutput, [peripheral]: next } };
    }),
  clearUartOutput: (peripheral) =>
    set((s) => ({ uartOutput: { ...s.uartOutput, [peripheral]: "" } })),
}));
