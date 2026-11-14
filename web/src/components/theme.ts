export type Theme = 'system' | 'light' | 'dark';

const storageKey = 'kiso-theme';

/** The theme the person chose, or `system`, per Kiso's ThemeSelector. */
export function savedTheme(): Theme {
  try {
    const value = localStorage.getItem(storageKey);
    return value === 'light' || value === 'dark' ? value : 'system';
  } catch {
    return 'system';
  }
}

/**
 * Applies and keeps a theme. `data-theme` is always light or dark: `system`
 * takes the OS mode, and public/theme.js follows it when it changes.
 */
export function applyTheme(theme: Theme) {
  document.documentElement.dataset.theme =
    theme === 'system'
      ? window.matchMedia('(prefers-color-scheme: dark)').matches
        ? 'dark'
        : 'light'
      : theme;
  try {
    localStorage.setItem(storageKey, theme);
  } catch {
    // Storage may be off; the choice then lasts until the page reloads.
  }
}
