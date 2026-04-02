import React from "react";
import { useAppStore } from "../../store/appStore";
import ControlBar from "./ControlBar";
import LedWidget from "../../components/peripherals/LedWidget";
import DisplayWidget from "../../components/peripherals/DisplayWidget";
import UartTerminal from "../../components/peripherals/UartTerminal";
import ButtonWidget from "../../components/peripherals/ButtonWidget";
import type {
  LedConfig,
  St7789Config,
  UartConfig,
  ButtonConfig,
} from "../../types/peripheral";

export default function SimulationPage() {
  const canvasItems = useAppStore((s) => s.canvasItems);

  const leds = canvasItems.filter((i) => i.type === "led" && i.config) as {
    instanceId: string;
    config: LedConfig;
  }[];
  const displays = canvasItems.filter((i) => i.type === "st7789" && i.config) as {
    instanceId: string;
    config: St7789Config;
  }[];
  const uarts = canvasItems.filter((i) => i.type === "uart" && i.config) as {
    instanceId: string;
    config: UartConfig;
  }[];
  const buttons = canvasItems.filter((i) => i.type === "button" && i.config) as {
    instanceId: string;
    config: ButtonConfig;
  }[];

  return (
    <div className="flex flex-col h-full bg-[#1e1e2e]">
      <ControlBar />

      <main className="flex-1 overflow-auto p-5 space-y-6">
        {/* Displays row */}
        {displays.length > 0 && (
          <section>
            <SectionTitle>Display</SectionTitle>
            <div className="flex flex-wrap gap-4">
              {displays.map((d) => (
                <DisplayWidget key={d.instanceId} config={d.config} />
              ))}
            </div>
          </section>
        )}

        {/* LEDs + Buttons row */}
        {(leds.length > 0 || buttons.length > 0) && (
          <section>
            <SectionTitle>GPIO</SectionTitle>
            <div className="flex flex-wrap gap-3">
              {leds.map((l) => (
                <LedWidget key={l.instanceId} config={l.config} />
              ))}
              {buttons.map((b) => (
                <ButtonWidget key={b.instanceId} config={b.config} />
              ))}
            </div>
          </section>
        )}

        {/* UART terminals */}
        {uarts.length > 0 && (
          <section>
            <SectionTitle>UART</SectionTitle>
            <div className="flex flex-col gap-3">
              {uarts.map((u) => (
                <UartTerminal key={u.instanceId} config={u.config} />
              ))}
            </div>
          </section>
        )}

        {canvasItems.length === 0 && (
          <div className="flex items-center justify-center h-40 text-[#45475a] text-sm">
            No components configured. Go back to Setup.
          </div>
        )}
      </main>
    </div>
  );
}

function SectionTitle({ children }: { children: React.ReactNode }) {
  return (
    <p className="text-xs text-[#6c7086] uppercase tracking-wider mb-3">{children}</p>
  );
}
