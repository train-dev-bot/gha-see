/** Omarchy-inspired theme catalog for gha-see. */
import { useEffect, useState } from "react";

export type ThemeMode = "dark" | "light";

export interface ThemeInfo {
  id: string;
  label: string;
  mode: ThemeMode;
}

export const THEMES: ThemeInfo[] = [
  { id: "catppuccin", label: "Catppuccin", mode: "dark" },
  { id: "catppuccin-latte", label: "Catppuccin Latte", mode: "light" },
  { id: "ethereal", label: "Ethereal", mode: "dark" },
  { id: "everforest", label: "Everforest", mode: "dark" },
  { id: "flexoki-light", label: "Flexoki Light", mode: "light" },
  { id: "gruvbox", label: "Gruvbox", mode: "dark" },
  { id: "hackerman", label: "Hackerman", mode: "dark" },
  { id: "kanagawa", label: "Kanagawa", mode: "dark" },
  { id: "last-horizon", label: "Last Horizon", mode: "dark" },
  { id: "lumon", label: "Lumon", mode: "dark" },
  { id: "lupine", label: "Lupine", mode: "light" },
  { id: "matte-black", label: "Matte Black", mode: "dark" },
  { id: "miasma", label: "Miasma", mode: "dark" },
  { id: "nord", label: "Nord", mode: "dark" },
  { id: "osaka-jade", label: "Osaka Jade", mode: "dark" },
  { id: "retro-82", label: "Retro 82", mode: "dark" },
  { id: "ristretto", label: "Ristretto", mode: "dark" },
  { id: "rose-pine", label: "Rose Pine", mode: "light" },
  { id: "solitude", label: "Solitude", mode: "dark" },
  { id: "tokyo-night", label: "Tokyo Night", mode: "dark" },
  { id: "vantablack", label: "Vantablack", mode: "dark" },
  { id: "white", label: "White", mode: "light" },
];

export const DEFAULT_THEME_ID = "tokyo-night";
const STORAGE_KEY = "gha-see:theme";
export const THEME_STORAGE_KEY = STORAGE_KEY;

export function themeById(id: string): ThemeInfo {
  return THEMES.find((t) => t.id === id) ?? THEMES.find((t) => t.id === DEFAULT_THEME_ID)!;
}

export function loadStoredThemeId(): string {
  try {
    const stored = localStorage.getItem(STORAGE_KEY);
    if (stored && THEMES.some((t) => t.id === stored)) return stored;
  } catch {
    /* ignore */
  }
  return DEFAULT_THEME_ID;
}

export function applyTheme(id: string): void {
  const theme = themeById(id);
  document.documentElement.dataset.theme = theme.id;
  document.documentElement.dataset.themeMode = theme.mode;
  try {
    localStorage.setItem(STORAGE_KEY, theme.id);
  } catch {
    /* ignore */
  }
}

/** Resolve a CSS custom property to a concrete color (for canvas/SVG markers). */
export function readThemeColor(
  varName: string,
  fallback: string,
): string {
  if (typeof document === "undefined") return fallback;
  const value = getComputedStyle(document.documentElement)
    .getPropertyValue(varName)
    .trim();
  return value || fallback;
}

/** Subscribe to `data-theme` changes and return a resolved CSS variable color. */
export function useThemeColor(varName: string, fallback: string): string {
  const [color, setColor] = useState(() => readThemeColor(varName, fallback));
  useEffect(() => {
    const sync = () => setColor(readThemeColor(varName, fallback));
    sync();
    const obs = new MutationObserver(sync);
    obs.observe(document.documentElement, {
      attributes: true,
      attributeFilter: ["data-theme", "data-theme-mode"],
    });
    return () => obs.disconnect();
  }, [varName, fallback]);
  return color;
}
