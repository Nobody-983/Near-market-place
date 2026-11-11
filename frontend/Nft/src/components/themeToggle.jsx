import { Moon, Sun } from "lucide-react";
import { useTheme } from "../context/themeContext";

function ThemeToggle({ inline = false, compact = false }) {
  const { theme, toggleTheme } = useTheme();
  const light = theme === "light";

  return (
    <button
      type="button"
      onClick={toggleTheme}
      className={`theme-toggle ${inline ? "theme-toggle--inline" : ""} ${compact ? "theme-toggle--compact" : ""}`}
      aria-label={`Switch to ${light ? "dark" : "light"} mode`}
      title={`Switch to ${light ? "dark" : "light"} mode`}
    >
      {light ? <Moon size={18} /> : <Sun size={18} />}
      <span>{light ? "Dark" : "Light"}</span>
    </button>
  );
}

export default ThemeToggle;
