import type { CanvasItem } from "../types/peripheral";
import type { RsemuProjectFile } from "../types/project";

function nowIso() {
  return new Date().toISOString();
}

function dirname(path: string): string {
  const normalized = path.replace(/\\/g, "/");
  const idx = normalized.lastIndexOf("/");
  return idx >= 0 ? normalized.slice(0, idx) : "";
}

function joinPath(base: string, rel: string): string {
  if (!base) return rel;
  return `${base.replace(/\/+$/, "")}/${rel.replace(/^\/+/, "")}`;
}

function normalizeSegments(path: string): string[] {
  const raw = path.replace(/\\/g, "/").split("/");
  const out: string[] = [];
  for (const seg of raw) {
    if (!seg || seg === ".") continue;
    if (seg === "..") out.pop();
    else out.push(seg);
  }
  return out;
}

export function toRelativePath(baseDir: string, targetAbsPath: string): string | null {
  const baseNorm = normalizeSegments(baseDir);
  const targetNorm = normalizeSegments(targetAbsPath);
  if (baseNorm.length === 0 || targetNorm.length === 0) return null;

  let i = 0;
  while (i < baseNorm.length && i < targetNorm.length && baseNorm[i] === targetNorm[i]) {
    i += 1;
  }
  if (i === 0) return null;

  const up = Array.from({ length: baseNorm.length - i }, () => "..");
  const down = targetNorm.slice(i);
  const rel = [...up, ...down].join("/");
  return rel || ".";
}

export function resolveFirmwarePath(projectFilePath: string, firmwarePath: string, kind: "relative" | "absolute"): string {
  if (kind === "absolute") return firmwarePath;
  return joinPath(dirname(projectFilePath), firmwarePath);
}

export function makeFirmwarePathForSave(projectFilePath: string, firmwareAbsPath: string): {
  path: string;
  path_kind: "relative" | "absolute";
} {
  const base = dirname(projectFilePath);
  const rel = toRelativePath(base, firmwareAbsPath);
  if (!rel || rel.startsWith("../..")) {
    return { path: firmwareAbsPath, path_kind: "absolute" };
  }
  return { path: rel, path_kind: "relative" };
}

export function createProjectFile(args: {
  name: string;
  board: string;
  firmwarePath: string;
  canvasItems: CanvasItem[];
  projectFilePath: string;
  createdAt?: string;
}): RsemuProjectFile {
  const ts = nowIso();
  const firmware = makeFirmwarePathForSave(args.projectFilePath, args.firmwarePath);
  return {
    schema_version: 1,
    project: {
      name: args.name,
      created_at: args.createdAt ?? ts,
      updated_at: ts,
    },
    target: {
      board: args.board,
    },
    firmware,
    canvas: {
      items: args.canvasItems,
    },
    ui: {
      last_page: "workspace",
    },
  };
}

export function parseProjectFile(raw: string): RsemuProjectFile {
  const parsed = JSON.parse(raw) as Partial<RsemuProjectFile>;
  if (parsed.schema_version !== 1) {
    throw new Error("Unsupported project schema version");
  }
  if (!parsed.project?.name) {
    throw new Error("Invalid project: missing project.name");
  }
  if (!parsed.target?.board) {
    throw new Error("Invalid project: missing target.board");
  }
  if (!parsed.canvas?.items || !Array.isArray(parsed.canvas.items)) {
    throw new Error("Invalid project: missing canvas.items");
  }
  if (!parsed.firmware?.path || !parsed.firmware?.path_kind) {
    throw new Error("Invalid project: missing firmware path");
  }
  return parsed as RsemuProjectFile;
}
