import { useEffect, useRef, useState } from "react";
import { applyTheme, loadStoredThemeId, THEMES, themeById } from "../themes";

export default function ThemeSwitcher() {
  const [open, setOpen] = useState(false);
  const [themeId, setThemeId] = useState(loadStoredThemeId);
  const rootRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    applyTheme(themeId);
  }, [themeId]);

  useEffect(() => {
    if (!open) return;
    const onDoc = (e: MouseEvent) => {
      if (!rootRef.current?.contains(e.target as Node)) setOpen(false);
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") setOpen(false);
    };
    document.addEventListener("mousedown", onDoc);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("mousedown", onDoc);
      document.removeEventListener("keydown", onKey);
    };
  }, [open]);

  const current = themeById(themeId);

  return (
    <div className="theme-switcher" ref={rootRef}>
      <button
        type="button"
        className="theme-switcher__button"
        aria-haspopup="listbox"
        aria-expanded={open}
        title="Change theme"
        onClick={() => setOpen((v) => !v)}
      >
        <span className="theme-switcher__icon" aria-hidden="true">
          ◐
        </span>
        <span className="theme-switcher__label">{current.label}</span>
      </button>
      {open && (
        <ul className="theme-switcher__menu" role="listbox" aria-label="Themes">
          {THEMES.map((theme) => (
            <li key={theme.id}>
              <button
                type="button"
                role="option"
                aria-selected={theme.id === themeId}
                className={`theme-switcher__option${
                  theme.id === themeId ? " theme-switcher__option--active" : ""
                }`}
                onClick={() => {
                  setThemeId(theme.id);
                  setOpen(false);
                }}
              >
                <span>{theme.label}</span>
                <span className="theme-switcher__mode">{theme.mode}</span>
              </button>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
