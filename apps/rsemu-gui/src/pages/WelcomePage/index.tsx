import React, { useMemo, useState } from "react";
import { useAppStore } from "../../store/appStore";
import { useProjectActions } from "../../hooks/useProjectActions";
import Button from "../../components/ui/Button";
import Select from "../../components/ui/Select";

export default function WelcomePage() {
  const boards = useAppStore((s) => s.boards);
  const [board, setBoard] = useState("stm32f103");
  const [name, setName] = useState("untitled");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const {
    recentProjects,
    openProject,
    loadProjectByPath,
    createNewProject,
    removeFromRecent,
  } = useProjectActions();

  const boardOptions = useMemo(
    () => boards.map((b) => ({ id: b.id, label: b.name })),
    [boards]
  );

  const handleOpen = async () => {
    setBusy(true);
    setError(null);
    try {
      await openProject();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const handleNew = async () => {
    if (!board) return;
    setBusy(true);
    setError(null);
    try {
      await createNewProject(board, name);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="h-full bg-[#1e1e2e] text-[#cdd6f4] flex items-center justify-center p-6">
      <div className="w-full max-w-3xl rounded-xl border border-[#3a3a5e] bg-[#181825] p-6 space-y-6">
        <header>
          <h1 className="text-2xl font-bold text-indigo-400">rsemu GUI</h1>
          <p className="text-sm text-[#a6adc8] mt-1">
            Open an existing project or create a new one to start debugging.
          </p>
        </header>

        <section className="rounded-lg border border-[#3a3a5e] p-4 space-y-3">
          <p className="text-xs uppercase tracking-wider text-[#6c7086]">New Project</p>
          <div className="grid grid-cols-1 md:grid-cols-2 gap-3">
            <Select
              label="Board"
              value={board}
              onChange={(e) => setBoard(e.target.value)}
            >
              {boardOptions.map((b) => (
                <option key={b.id} value={b.id}>{b.label}</option>
              ))}
            </Select>
            <label className="flex flex-col gap-1 text-xs text-[#a6adc8]">
              Name
              <input
                value={name}
                onChange={(e) => setName(e.target.value)}
                className="bg-[#2a2a3e] border border-[#3a3a5e] rounded px-2 py-1.5 text-sm text-[#cdd6f4] focus:outline-none focus:border-indigo-500"
              />
            </label>
          </div>
          <div className="flex gap-2">
            <Button variant="primary" onClick={handleNew} disabled={busy || !board}>
              Create
            </Button>
            <Button variant="secondary" onClick={handleOpen} disabled={busy}>
              Open Project...
            </Button>
          </div>
        </section>

        <section className="rounded-lg border border-[#3a3a5e] p-4 space-y-3">
          <p className="text-xs uppercase tracking-wider text-[#6c7086]">Recent Projects</p>
          {recentProjects.length === 0 && (
            <p className="text-sm text-[#6c7086]">No recent projects.</p>
          )}
          <div className="space-y-2">
            {recentProjects.map((path) => (
              <div key={path} className="flex items-center gap-2">
                <button
                  className="flex-1 text-left text-sm px-3 py-2 rounded border border-[#3a3a5e] bg-[#2a2a3e] hover:border-indigo-500 truncate"
                  onClick={async () => {
                    setBusy(true);
                    setError(null);
                    try {
                      await loadProjectByPath(path);
                    } catch (e) {
                      setError(String(e));
                    } finally {
                      setBusy(false);
                    }
                  }}
                  title={path}
                >
                  {path}
                </button>
                <Button
                  size="sm"
                  variant="ghost"
                  onClick={() => void removeFromRecent(path)}
                  title="Remove from recent"
                >
                  Remove
                </Button>
              </div>
            ))}
          </div>
        </section>

        {error && (
          <div className="text-sm text-red-400 rounded border border-red-900/50 bg-red-950/20 px-3 py-2">
            {error}
          </div>
        )}
      </div>
    </div>
  );
}
