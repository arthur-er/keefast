// Theme handling: System, Dark or Light (the app setting), applied as `data-theme` on <html>,
// which tokens.css switches on. System follows the OS: in Tauri the window's theme (which tracks
// GTK's and Windows' setting), in a browser `prefers-color-scheme`.

import { useEffect, useState } from "react";

import type { Theme } from "@/bindings";
import { inTauri } from "@/api";

export type AppliedTheme = "dark" | "light";

export function resolveTheme(setting: Theme, osDark: boolean): AppliedTheme {
  if (setting === "system") return osDark ? "dark" : "light";
  return setting;
}

export function applyTheme(theme: AppliedTheme, root: HTMLElement = document.documentElement) {
  root.dataset.theme = theme;
  root.style.colorScheme = theme;
}

/** Whether the OS is set to dark, kept current. */
export function useOsDark(): boolean {
  const query = typeof window === "undefined" ? null : window.matchMedia("(prefers-color-scheme: dark)");
  const [dark, setDark] = useState(query?.matches ?? true);
  useEffect(() => {
    if (!query) return;
    const onChange = () => setDark(query.matches);
    query.addEventListener("change", onChange);
    if (!inTauri) return () => query.removeEventListener("change", onChange);
    // WebKitGTK doesn't always follow GTK's dark setting in prefers-color-scheme; the window does.
    let stop: (() => void) | undefined;
    let cancelled = false;
    void import("@tauri-apps/api/window").then(async ({ getCurrentWindow }) => {
      const window = getCurrentWindow();
      const theme = await window.theme();
      if (cancelled) return;
      if (theme) setDark(theme === "dark");
      stop = await window.onThemeChanged(({ payload }) => setDark(payload === "dark"));
      if (cancelled) stop();
    });
    return () => {
      cancelled = true;
      query.removeEventListener("change", onChange);
      stop?.();
    };
  }, [query]);
  return dark;
}

/** Applies the theme setting to the page while mounted. */
export function useApplyTheme(setting: Theme | undefined) {
  const osDark = useOsDark();
  const theme = resolveTheme(setting ?? "system", osDark);
  useEffect(() => applyTheme(theme), [theme]);
  return theme;
}
