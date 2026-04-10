import { useCallback } from "react";
import { useAppStore } from "../store/appStore";
import {
  clearLastProject,
  forgetProject,
  openProjectDialog,
  readProjectFile,
  rememberProject,
  saveProjectDialog,
  writeProjectFile,
} from "../lib/tauri";
import { createProjectFile, parseProjectFile, resolveFirmwarePath } from "../lib/project";

function projectNameFromPath(path: string): string {
  const normalized = path.replace(/\\/g, "/");
  const file = normalized.split("/").pop() ?? "project";
  return file.replace(/\.rsemu\.json$/i, "").replace(/\.json$/i, "");
}

export function useProjectActions() {
  const boards = useAppStore((s) => s.boards);
  const recentProjects = useAppStore((s) => s.recentProjects);
  const projectPath = useAppStore((s) => s.projectPath);
  const projectName = useAppStore((s) => s.projectName);
  const projectCreatedAt = useAppStore((s) => s.projectCreatedAt);
  const selectedBoard = useAppStore((s) => s.selectedBoard);
  const firmwarePath = useAppStore((s) => s.firmwarePath);
  const canvasItems = useAppStore((s) => s.canvasItems);
  const dirty = useAppStore((s) => s.dirty);

  const setProjectMeta = useAppStore((s) => s.setProjectMeta);
  const setSelectedBoard = useAppStore((s) => s.setSelectedBoard);
  const setFirmwarePath = useAppStore((s) => s.setFirmwarePath);
  const setCanvasItems = useAppStore((s) => s.setCanvasItems);
  const setDirty = useAppStore((s) => s.setDirty);
  const setPage = useAppStore((s) => s.setPage);
  const setRecentProjects = useAppStore((s) => s.setRecentProjects);

  const saveProjectAs = useCallback(async (): Promise<boolean> => {
    const path = await saveProjectDialog();
    if (!path) return false;

    const payload = createProjectFile({
      name: projectName || projectNameFromPath(path),
      board: selectedBoard,
      firmwarePath,
      canvasItems,
      projectFilePath: path,
      createdAt: projectCreatedAt ?? undefined,
    });
    await writeProjectFile(path, JSON.stringify(payload, null, 2));
    const prefs = await rememberProject(path);
    setRecentProjects(prefs.recent_projects);
    setProjectMeta(path, payload.project.name, payload.project.created_at);
    setDirty(false);
    return true;
  }, [
    canvasItems,
    firmwarePath,
    projectCreatedAt,
    projectName,
    selectedBoard,
    setDirty,
    setProjectMeta,
    setRecentProjects,
  ]);

  const saveProject = useCallback(async (): Promise<boolean> => {
    if (!projectPath) {
      return saveProjectAs();
    }

    const payload = createProjectFile({
      name: projectName || projectNameFromPath(projectPath),
      board: selectedBoard,
      firmwarePath,
      canvasItems,
      projectFilePath: projectPath,
      createdAt: projectCreatedAt ?? undefined,
    });
    await writeProjectFile(projectPath, JSON.stringify(payload, null, 2));
    const prefs = await rememberProject(projectPath);
    setRecentProjects(prefs.recent_projects);
    setProjectMeta(projectPath, payload.project.name, payload.project.created_at);
    setDirty(false);
    return true;
  }, [
    canvasItems,
    firmwarePath,
    projectCreatedAt,
    projectName,
    projectPath,
    saveProjectAs,
    selectedBoard,
    setDirty,
    setProjectMeta,
    setRecentProjects,
  ]);

  const ensureSafeToProceed = useCallback(async (): Promise<boolean> => {
    if (!dirty) return true;
    const saveFirst = window.confirm(
      "You have unsaved changes. Press OK to save before continuing. Press Cancel for more options."
    );
    if (saveFirst) {
      return saveProject();
    }
    const discard = window.confirm("Discard unsaved changes and continue?");
    return discard;
  }, [dirty, saveProject]);

  const loadProjectByPath = useCallback(async (path: string, options?: { skipDirtyCheck?: boolean }) => {
    if (!options?.skipDirtyCheck) {
      const ok = await ensureSafeToProceed();
      if (!ok) return false;
    }

    const raw = await readProjectFile(path);
    const project = parseProjectFile(raw);
    const boardExists = boards.some((b) => b.id === project.target.board);
    if (!boardExists) {
      throw new Error(`Board not supported in current GUI: ${project.target.board}`);
    }
    const resolvedFirmwarePath = resolveFirmwarePath(
      path,
      project.firmware.path,
      project.firmware.path_kind
    );

    setProjectMeta(path, project.project.name, project.project.created_at);
    setSelectedBoard(project.target.board, false);
    setFirmwarePath(resolvedFirmwarePath, false);
    setCanvasItems(project.canvas.items, false);
    setDirty(false);
    setPage("setup");

    const prefs = await rememberProject(path);
    setRecentProjects(prefs.recent_projects);
    return true;
  }, [
    boards,
    ensureSafeToProceed,
    setCanvasItems,
    setDirty,
    setFirmwarePath,
    setPage,
    setProjectMeta,
    setRecentProjects,
    setSelectedBoard,
  ]);

  const openProject = useCallback(async () => {
    const path = await openProjectDialog();
    if (!path) return false;
    return loadProjectByPath(path);
  }, [loadProjectByPath]);

  const createNewProject = useCallback(async (board: string, name: string) => {
    const ok = await ensureSafeToProceed();
    if (!ok) return false;

    setProjectMeta(null, name.trim() || "untitled", null);
    setSelectedBoard(board, false);
    setFirmwarePath("", false);
    setCanvasItems([], false);
    setDirty(true);
    setPage("setup");
    return true;
  }, [ensureSafeToProceed, setCanvasItems, setDirty, setFirmwarePath, setPage, setProjectMeta, setSelectedBoard]);

  const removeFromRecent = useCallback(async (path: string) => {
    const prefs = await forgetProject(path);
    setRecentProjects(prefs.recent_projects);
  }, [setRecentProjects]);

  const clearAutoLoad = useCallback(async () => {
    const prefs = await clearLastProject();
    setRecentProjects(prefs.recent_projects);
  }, [setRecentProjects]);

  return {
    recentProjects,
    projectPath,
    projectName,
    dirty,
    openProject,
    loadProjectByPath,
    createNewProject,
    saveProject,
    saveProjectAs,
    removeFromRecent,
    clearAutoLoad,
  };
}
