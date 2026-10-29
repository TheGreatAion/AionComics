// Dark (warm charcoal) or light (Paper). app.html applies the saved choice before the first paint,
// so the window never flashes the other one.
export type Theme = "dark" | "light";

const KEY = "aion.theme";

function saved(): Theme {
  try {
    return localStorage.getItem(KEY) === "light" ? "light" : "dark";
  } catch {
    return "dark";
  }
}

export const theme = $state({ current: saved() });

export function setTheme(t: Theme) {
  theme.current = t;
  if (t === "light") document.documentElement.dataset.theme = "light";
  else delete document.documentElement.dataset.theme;
  try {
    localStorage.setItem(KEY, t);
  } catch {}
}
