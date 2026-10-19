import { useState, useRef } from "react";
import { createPortal, flushSync } from "react-dom";
import { useTheme, PALETTES } from "../ThemeContext";
import { useLanguage } from "../i18n";
import "./PaletteMenu.css";

const LONG_PRESS_MS = 400;

function MoonIcon() {
  return (
    <svg viewBox="0 0 24 24" fill="none" aria-hidden="true">
      <path
        d="M20.5 13.4A8 8 0 1 1 10.6 3.5a6.3 6.3 0 0 0 9.9 9.9Z"
        stroke="currentColor"
        strokeWidth="1.6"
        strokeLinejoin="round"
      />
    </svg>
  );
}

function SunIcon() {
  return (
    <svg viewBox="0 0 24 24" fill="none" aria-hidden="true">
      <circle cx="12" cy="12" r="3.8" stroke="currentColor" strokeWidth="1.6" />
      <path
        d="M12 2.6v2.2M12 19.2v2.2M4.4 4.4l1.6 1.6M18 18l1.6 1.6M2.6 12h2.2M19.2 12h2.2M4.4 19.6 6 18M18 6l1.6-1.6"
        stroke="currentColor"
        strokeWidth="1.6"
        strokeLinecap="round"
      />
    </svg>
  );
}

/**
 * Кнопка темы — совмещает 2 действия:
 * - Тап (короткое нажатие / клик / клавиатура) — мгновенно переключает
 *   светлый/тёмный режим, палитра остаётся той же.
 * - Долгое нажатие (удержание ~400мс) — открывает попап с 6 цветами
 *   палитры (для текущего режима), режим остаётся тем же.
 * Иконка на кнопке показывает ТЕКУЩИЙ режим (луна — тёмный, солнце — светлый).
 */
export default function PaletteMenu() {
  const { t } = useLanguage();
  const { palette, mode, setPalette, setMode } = useTheme();
  const [open, setOpen] = useState(false);
  const [pos, setPos] = useState(null);
  const triggerRef = useRef(null);
  const pressTimerRef = useRef(null);
  const longPressFiredRef = useRef(false);

  function openPopover() {
    const rect = triggerRef.current.getBoundingClientRect();
    setPos({ top: rect.bottom + 8, left: rect.left + rect.width / 2 });
    setOpen(true);
  }

  function handlePointerDown() {
    longPressFiredRef.current = false;
    clearTimeout(pressTimerRef.current);
    pressTimerRef.current = setTimeout(() => {
      longPressFiredRef.current = true;
      openPopover();
    }, LONG_PRESS_MS);
  }

  function handlePointerUp(event) {
    clearTimeout(pressTimerRef.current);
    if (!longPressFiredRef.current) {
      apply(palette, mode === "dark" ? "light" : "dark", event);
    }
  }

  function handlePointerCancel() {
    clearTimeout(pressTimerRef.current);
  }

  // Клавиатурная активация (Enter/Space) — событие без реального указателя
  // (detail === 0), pointerDown/Up для неё не срабатывают — обрабатываем click.
  function handleClick(event) {
    if (event.detail === 0) {
      apply(palette, mode === "dark" ? "light" : "dark", event);
    }
  }

  function apply(paletteId, modeId, event) {
    const doApply = () => {
      setPalette(paletteId);
      setMode(modeId);
    };
    if (paletteId === palette && modeId === mode) return;
    const reduced = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    if (!document.startViewTransition || reduced) {
      doApply();
      return;
    }
    let x = event.clientX;
    let y = event.clientY;
    if (event.detail === 0) {
      const r = event.currentTarget.getBoundingClientRect();
      x = r.left + r.width / 2;
      y = r.top + r.height / 2;
    }
    const radius = Math.hypot(
      Math.max(x, window.innerWidth - x),
      Math.max(y, window.innerHeight - y)
    );
    document
      .startViewTransition(() => {
        flushSync(doApply);
      })
      .ready.then(() => {
        document.documentElement.animate(
          { clipPath: [`circle(0px at ${x}px ${y}px)`, `circle(${radius}px at ${x}px ${y}px)`] },
          { duration: 520, easing: "cubic-bezier(0.4, 0, 0.2, 1)", pseudoElement: "::view-transition-new(root)" }
        );
      })
      .catch(() => {});
  }

  function pickPalette(paletteId, event) {
    apply(paletteId, mode, event);
    setOpen(false);
  }

  return (
    <>
      <button
        ref={triggerRef}
        type="button"
        className="language-button palette-menu__trigger"
        aria-haspopup="dialog"
        aria-expanded={open}
        aria-label={mode === "dark" ? t.themeLight : t.themeDark}
        title={t.themeHint}
        onPointerDown={handlePointerDown}
        onPointerUp={handlePointerUp}
        onPointerLeave={handlePointerCancel}
        onPointerCancel={handlePointerCancel}
        onContextMenu={(e) => e.preventDefault()}
        onClick={handleClick}
      >
        {mode === "dark" ? <MoonIcon /> : <SunIcon />}
      </button>
      {open &&
        pos &&
        createPortal(
          <>
            <div className="palette-menu__backdrop" onClick={() => setOpen(false)} />
            <div
              className="palette-menu__popover"
              role="dialog"
              aria-label={t.appearance}
              style={{ top: pos.top, left: pos.left }}
            >
              <div className="palette-menu__row" role="radiogroup" aria-label={t.appearance}>
                {PALETTES.map((p) => {
                  const swatchColor = mode === "light" ? p.swatchLight : p.swatch;
                  const active = p.id === palette;
                  return (
                    <button
                      key={p.id}
                      type="button"
                      className={`palette-menu__swatch${active ? " palette-menu__swatch--active" : ""}`}
                      style={{ "--swatch": swatchColor }}
                      role="radio"
                      aria-checked={active}
                      aria-label={p.label}
                      title={p.label}
                      onClick={(e) => pickPalette(p.id, e)}
                    >
                      <span className="palette-menu__dot" aria-hidden="true" />
                    </button>
                  );
                })}
              </div>
            </div>
          </>,
          document.body
        )}
    </>
  );
}
