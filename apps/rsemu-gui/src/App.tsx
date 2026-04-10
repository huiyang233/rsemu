import React, { useEffect, useState } from "react";
import { useAppStore } from "./store/appStore";
import { useEmulatorEvents } from "./hooks/useEmulatorEvents";
import SetupPage from "./pages/SetupPage";
import SimulationPage from "./pages/SimulationPage";
import WelcomePage from "./pages/WelcomePage";
import {
  clearLastProject,
  getAppPreferences,
  getBoards,
  readProjectFile,
  rememberProject,
} from "./lib/tauri";
import { parseProjectFile, resolveFirmwarePath } from "./lib/project";

export default function App() {
  const page = useAppStore((s) => s.page);
  const setPage = useAppStore((s) => s.setPage);
  const setBoards = useAppStore((s) => s.setBoards);
  const setRecentProjects = useAppStore((s) => s.setRecentProjects);
  const setProjectMeta = useAppStore((s) => s.setProjectMeta);
  const setSelectedBoard = useAppStore((s) => s.setSelectedBoard);
  const setFirmwarePath = useAppStore((s) => s.setFirmwarePath);
  const setCanvasItems = useAppStore((s) => s.setCanvasItems);
  const setDirty = useAppStore((s) => s.setDirty);
  const [booting, setBooting] = useState(true);

  // Subscribe to all backend events globally
  useEmulatorEvents();

  useEffect(() => {
    let cancelled = false;
    async function bootstrap() {
      try {
        const boards = await getBoards();
        if (cancelled) return;
        setBoards(boards);

        const prefs = await getAppPreferences();
        if (cancelled) return;
        setRecentProjects(prefs.recent_projects);

        if (prefs.last_opened_project) {
          try {
            const raw = await readProjectFile(prefs.last_opened_project);
            const project = parseProjectFile(raw);
            const boardExists = boards.some((b) => b.id === project.target.board);
            if (!boardExists) {
              throw new Error(`Board not supported: ${project.target.board}`);
            }

            setProjectMeta(
              prefs.last_opened_project,
              project.project.name,
              project.project.created_at
            );
            setSelectedBoard(project.target.board, false);
            setFirmwarePath(
              resolveFirmwarePath(
                prefs.last_opened_project,
                project.firmware.path,
                project.firmware.path_kind
              ),
              false
            );
            setCanvasItems(project.canvas.items, false);
            setDirty(false);
            setPage("setup");

            const refreshed = await rememberProject(prefs.last_opened_project);
            if (!cancelled) {
              setRecentProjects(refreshed.recent_projects);
            }
          } catch (e) {
            console.error("failed to auto-load project", e);
            await clearLastProject();
            if (!cancelled) setPage("welcome");
          }
        } else {
          setPage("welcome");
        }
      } catch (e) {
        console.error("app bootstrap failed", e);
        if (!cancelled) setPage("welcome");
      } finally {
        if (!cancelled) setBooting(false);
      }
    }

    void bootstrap();
    return () => {
      cancelled = true;
    };
  }, [
    setBoards,
    setCanvasItems,
    setDirty,
    setFirmwarePath,
    setPage,
    setProjectMeta,
    setRecentProjects,
    setSelectedBoard,
  ]);

  if (booting) {
    return (
      <div className="h-full flex items-center justify-center bg-[#1e1e2e] text-[#a6adc8] text-sm">
        Loading project context...
      </div>
    );
  }

  if (page === "welcome") return <WelcomePage />;
  return page === "setup" ? <SetupPage /> : <SimulationPage />;
}
