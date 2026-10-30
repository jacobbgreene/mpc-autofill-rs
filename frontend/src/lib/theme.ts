export const themes = [
  { value: "light", label: "Light (Default)", icon: "sun" },
  { value: "dark", label: "Dark", icon: "moon" },
  { value: "blue", label: "Blue", icon: "sun" },
  { value: "green", label: "Green", icon: "leaf" },
  { value: "purple", label: "Purple", icon: "sun" },
  { value: "pink", label: "Pink", icon: "sun" },
  { value: "orange", label: "Orange", icon: "sun" },
  { value: "teal", label: "Teal", icon: "leaf" },
  { value: "sand", label: "Sand", icon: "sun" },
  { value: "midnight", label: "Midnight", icon: "moon" },
] as const;
export type Theme = (typeof themes)[number]["value"];
const storageKey = "mpc-autofill-theme";

export function readTheme(): Theme {
  try {
    const value = localStorage.getItem(storageKey);
    if (value === "zero") return "green";
    const selected = themes.find((theme) => theme.value === value);
    if (selected) return selected.value;
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
