import React from "react";
import { useAppStore } from "./store/appStore";
import { useEmulatorEvents } from "./hooks/useEmulatorEvents";
import SetupPage from "./pages/SetupPage";
import SimulationPage from "./pages/SimulationPage";

export default function App() {
  const page = useAppStore((s) => s.page);

  // Subscribe to all backend events globally
  useEmulatorEvents();

  return page === "setup" ? <SetupPage /> : <SimulationPage />;
}
