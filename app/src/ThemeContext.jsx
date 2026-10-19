import { createContext, useContext, useState, useEffect } from "react";

export const PALETTES = [
  { id: "violet", label: "Violet", swatch: "#8565fa", swatchLight: "#b9a8f7" },
  { id: "emerald", label: "Emerald", swatch: "#2ebe82", swatchLight: "#86e0b8" },
  { id: "cyan", label: "Cyan", swatch: "#38b2d6", swatchLight: "#8fd4ea" },
  { id: "amber", label: "Amber", swatch: "#e9a036", swatchLight: "#f2ce8a" },
  { id: "onyx", label: "Onyx", swatch: "#3b3d47", swatchLight: "#c8ccd6" },
  { id: "crimson", label: "Crimson", swatch: "#e8424a", swatchLight: "#f2989e" },
];
export const MODES = ["dark", "light"];

const DEFAULT_PALETTE = "violet";
const DEFAULT_MODE = "dark";
const LS_PALETTE_KEY = "nft-staking:palette";
const LS_MODE_KEY = "nft-staking:mode";

function readStored(key, allowed, fallback) {
  try {
    const v = localStorage.getItem(key);
    if (v && allowed.includes(v)) return v;
  } catch {
    /* localStorage недоступен (приватный режим и т.п.) — используем дефолт */
  }
  return fallback;
}

const ThemeContext = createContext({
  palette: DEFAULT_PALETTE,
  mode: DEFAULT_MODE,
  setPalette: () => {},
  setMode: () => {},
});

export function ThemeProvider({ children }) {
  const [palette, setPalette] = useState(() =>
    readStored(LS_PALETTE_KEY, PALETTES.map((p) => p.id), DEFAULT_PALETTE)
  );
  const [mode, setMode] = useState(() => readStored(LS_MODE_KEY, MODES, DEFAULT_MODE));

  useEffect(() => {
    document.documentElement.dataset.palette = palette;
    try {
      localStorage.setItem(LS_PALETTE_KEY, palette);
    } catch {
      /* ignore */
    }
  }, [palette]);

  useEffect(() => {
    document.documentElement.dataset.mode = mode;
    try {
      localStorage.setItem(LS_MODE_KEY, mode);
    } catch {
      /* ignore */
    }
  }, [mode]);

  return (
    <ThemeContext.Provider value={{ palette, mode, setPalette, setMode }}>
      {children}
    </ThemeContext.Provider>
  );
}

export function useTheme() {
  return useContext(ThemeContext);
}
