export type Theme = 'system' | 'light' | 'dark';

const storageKey = 'kiso-theme';

export function savedTheme(): Theme {
  try {
    const value = localStorage.getItem(storageKey);
    return value === 'light' || value === 'dark' ? value : 'system';
  } catch {
    return 'system';
  }
}

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
    return;
  }
}
