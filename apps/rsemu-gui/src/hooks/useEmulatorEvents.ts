import { useEffect } from "react";
import {
  onSimStatus,
  onLedChanged,
  onDisplayFrame,
  onUartOutput,
  onSimSteps,
} from "../lib/tauri";
import { useAppStore } from "../store/appStore";

/**
 * Subscribe to all Tauri emulator events and update the global store.
 * Mount this once at the App level.
 */
export function useEmulatorEvents(
  onFrame?: (payload: { width: number; height: number; data: string }) => void
) {
  const setRunning = useAppStore((s) => s.setRunning);
  const setSimError = useAppStore((s) => s.setSimError);
  const setSteps = useAppStore((s) => s.setSteps);
  const setLedState = useAppStore((s) => s.setLedState);
  const appendUartByte = useAppStore((s) => s.appendUartByte);

  useEffect(() => {
    const unlisteners: Array<() => void> = [];

    onSimStatus((p) => {
      setRunning(p.running);
      setSteps(p.steps);
      if (p.error) setSimError(p.error);
    }).then((fn) => unlisteners.push(fn));

    onLedChanged((p) => setLedState(p.id, p.on))
      .then((fn) => unlisteners.push(fn));

    onUartOutput((p) => appendUartByte(p.peripheral, p.byte))
      .then((fn) => unlisteners.push(fn));

    onSimSteps((steps) => setSteps(steps))
      .then((fn) => unlisteners.push(fn));

    if (onFrame) {
      onDisplayFrame((p) => onFrame(p))
        .then((fn) => unlisteners.push(fn));
    }

    return () => unlisteners.forEach((fn) => fn());
  }, []); // eslint-disable-line react-hooks/exhaustive-deps
}
