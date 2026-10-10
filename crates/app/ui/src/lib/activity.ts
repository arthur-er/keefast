// Tells the backend the user is active (keys, clicks, scrolling), which postpones the idle lock,
// at most every few seconds.

import { useEffect } from "react";

import { api } from "@/api";

const EVERY_MS = 5000;

export function useActivityReporting() {
  useEffect(() => {
    let last = 0;
    const report = () => {
      const now = Date.now();
      if (now - last < EVERY_MS) return;
      last = now;
      void api.noteActivity().catch(() => {});
    };
    const events = ["keydown", "pointerdown", "wheel"] as const;
    for (const name of events) window.addEventListener(name, report, { capture: true, passive: true });
    return () => {
      for (const name of events) window.removeEventListener(name, report, { capture: true });
    };
  }, []);
}
