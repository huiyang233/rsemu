import type { CanvasItem } from "./peripheral";

export interface ProjectMetadata {
  name: string;
  created_at: string;
  updated_at: string;
}

export interface ProjectTarget {
  board: string;
}

export interface ProjectFirmware {
  path: string;
  path_kind: "relative" | "absolute";
}

export interface ProjectCanvas {
  items: CanvasItem[];
}

export interface ProjectUiState {
  last_page?: "setup" | "simulation";
}

export interface RsemuProjectFile {
  schema_version: 1;
  project: ProjectMetadata;
  target: ProjectTarget;
  firmware: ProjectFirmware;
  canvas: ProjectCanvas;
  ui?: ProjectUiState;
}

export interface AppPreferences {
  last_opened_project: string | null;
  recent_projects: string[];
}
