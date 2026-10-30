// Dark (warm charcoal) or light (Paper), or whichever Windows is set to. app.html applies the saved
// choice before the first paint, so the window never flashes the other one.
export type Theme = "dark" | "light";
export type ThemeChoice = Theme | "system";

const KEY = "aion.theme";
const system = typeof matchMedia === "function" ? matchMedia("(prefers-color-scheme: light)") : null;

function saved(): ThemeChoice {
  try {
    const v = localStorage.getItem(KEY);
    return v === "light" || v === "system" ? v : "dark";
  } catch {
    return "dark";
  }
}

const resolve = (c: ThemeChoice): Theme => (c === "system" ? (system?.matches ? "light" : "dark") : c);

/** `choice` is what was picked; `current` is the theme on screen. */
export const theme = $state({ choice: saved(), current: resolve(saved()) });

function apply() {
  theme.current = resolve(theme.choice);
  if (theme.current === "light") document.documentElement.dataset.theme = "light";
  else delete document.documentElement.dataset.theme;
}

system?.addEventListener("change", () => theme.choice === "system" && apply());

export function setTheme(c: ThemeChoice) {
  theme.choice = c;
  apply();
  try {
    localStorage.setItem(KEY, c);
  } catch {}
}
