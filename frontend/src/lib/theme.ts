export type Theme = "system" | "light" | "dark";
const storageKey = "mpc-autofill-theme";

export function readTheme(): Theme {
  try {
    const value = localStorage.getItem(storageKey);
    if (value === "light" || value === "dark") return value;
  } catch {
    /* Appearance still works when storage is unavailable. */
  }
  return "system";
}

export function applyTheme(theme: Theme) {
  const dark =
    theme === "dark" ||
    (theme === "system" && matchMedia("(prefers-color-scheme: dark)").matches);
  document.documentElement.dataset.theme = dark ? "dark" : "light";
}

export function saveTheme(theme: Theme) {
  try {
    localStorage.setItem(storageKey, theme);
  } catch {
    /* Keep the choice for this session. */
  }
  applyTheme(theme);
}
