// Light/dark: follows the OS until the user picks one, then remembers it.

type Theme = 'light' | 'dark';

function system(): Theme {
  return matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light';
}

function saved(): Theme | null {
  try {
    const t = localStorage.getItem('theme');
    return t === 'light' || t === 'dark' ? t : null;
  } catch {
    return null;
  }
}

class ThemeStore {
  current = $state<Theme>(saved() ?? system());

  toggle() {
    this.current = this.current === 'dark' ? 'light' : 'dark';
    document.documentElement.dataset.theme = this.current;
    try {
      localStorage.setItem('theme', this.current);
    } catch {
      // Private mode: the choice lasts for this visit.
    }
  }
}

export const theme = new ThemeStore();
