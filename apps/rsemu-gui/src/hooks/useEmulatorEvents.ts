import { useEffect } from "react";
import {
  simStatusBus,
  ledChangedBus,
  uartOutputBus,
  simStepsBus,
  displayFrameBus,
} from "../lib/tauri";
import { useAppStore } from "../store/appStore";
import type { DisplayFramePayload } from "../types/simulation";

/**
 * Subscribe to emulator events via the module-level EventBus.
 * Safe under React StrictMode — no duplicate Tauri listeners.
 */
export function useEmulatorEvents(
  onFrame?: (payload: DisplayFramePayload) => void
) {
  const setRunning    = useAppStore((s) => s.setRunning);
  const setSimError   = useAppStore((s) => s.setSimError);
  const setSteps      = useAppStore((s) => s.setSteps);
  const setLedState   = useAppStore((s) => s.setLedState);
  const appendUartBytes = useAppStore((s) => s.appendUartBytes);

  useEffect(() => {
    const unsubs = [
      simStatusBus.subscribe((p) => {
        setRunning(p.running);
        setSteps(p.steps);
        if (p.error) setSimError(p.error);
      }),
      ledChangedBus.subscribe((p) => setLedState(p.id, p.on)),
      uartOutputBus.subscribe((p) => appendUartBytes(p.peripheral, p.bytes)),
      simStepsBus.subscribe((steps) => setSteps(steps)),
      ...(onFrame ? [displayFrameBus.subscribe(onFrame)] : []),
    ];

    return () => unsubs.forEach((fn) => fn());
  }, []); // eslint-disable-line react-hooks/exhaustive-deps
}
