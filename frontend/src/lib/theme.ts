export type Theme = "light" | "dark" | "zero";
const storageKey = "mpc-autofill-theme";

export function readTheme(): Theme {
  try {
    const value = localStorage.getItem(storageKey);
    if (value === "light" || value === "dark" || value === "zero") return value;
  } catch {
    /* Appearance still works when storage is unavailable. */
  }
  // Unset, legacy System, and invalid preferences use the explicit default.
  return "light";
}

export function applyTheme(theme: Theme) {
  document.documentElement.dataset.theme = theme;
}

export function saveTheme(theme: Theme) {
  try {
    localStorage.setItem(storageKey, theme);
  } catch {
    /* Keep the choice for this session. */
  }
  applyTheme(theme);
}
