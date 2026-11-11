import { createContext, useCallback, useContext, useMemo, useState } from "react";

const STORAGE_KEY = "near-nft-theme";
const ThemeContext = createContext(null);

function currentTheme() {
  if (typeof document === "undefined") return "dark";
  return document.documentElement.dataset.theme === "light" ? "light" : "dark";
}

export function ThemeProvider({ children }) {
  const [theme, setThemeState] = useState(currentTheme);

  const setTheme = useCallback((nextTheme) => {
    const value = nextTheme === "light" ? "light" : "dark";
    document.documentElement.dataset.theme = value;
    document.documentElement.style.colorScheme = value;
    try { localStorage.setItem(STORAGE_KEY, value); } catch { /* Storage may be unavailable. */ }
    setThemeState(value);
  }, []);

  const toggleTheme = useCallback(() => {
    setTheme(theme === "dark" ? "light" : "dark");
  }, [setTheme, theme]);

  const value = useMemo(() => ({ theme, setTheme, toggleTheme }), [setTheme, theme, toggleTheme]);
  return <ThemeContext.Provider value={value}>{children}</ThemeContext.Provider>;
}

// eslint-disable-next-line react-refresh/only-export-components
export function useTheme() {
  const context = useContext(ThemeContext);
  if (!context) throw new Error("useTheme must be used within ThemeProvider");
  return context;
}
