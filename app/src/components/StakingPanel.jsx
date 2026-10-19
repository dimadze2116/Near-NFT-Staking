import { useLayoutEffect, useRef, useState } from "react";
import PeriodButton from "./PeriodButton";
import DropIndicator from "./DropIndicator";
import useDropIndicator from "./useDropIndicator";
import { useLanguage } from "../i18n";
import "./StakingPanel.css";

const PERIODS = [
  { id: "7", daysKey: "days7" },
  { id: "14", daysKey: "days14" },
  { id: "30", daysKey: "days30" },
];

export default function StakingPanel({ exiting = false, period, onPeriodChange }) {
  const { t } = useLanguage();
  const ex = exiting ? " staking-panel__row--exiting" : "";
  const selectedPeriod = period;
  const setSelectedPeriod = onPeriodChange;
  const buttonsRef = useRef(null);
  const buttonRefs = useRef({});
  const { indicator, phase, direction } = useDropIndicator({
    containerRef: buttonsRef,
    buttonRefs,
    activeId: selectedPeriod,
    maxStretch: 1,
  });

  // Контур из скруглённых прямоугольников по форме кнопок: капля видна только
  // внутри них — пролетает "сквозь" зазоры, не теряя радиусов на срезах
  const [clipPath, setClipPath] = useState("");

  useLayoutEffect(() => {
    const measure = () => {
      const container = buttonsRef.current;
      if (!container) return;
      const containerRect = container.getBoundingClientRect();
      const r = 13;
      const parts = PERIODS.map(({ id }) => {
        const el = buttonRefs.current[id];
        if (!el) return "";
        const b = el.getBoundingClientRect();
        const x = b.left - containerRect.left;
        const y = b.top - containerRect.top;
        const w = b.width;
        const h = b.height;
        return (
          `M${(x + r).toFixed(2)},${y.toFixed(2)} ` +
          `h${(w - 2 * r).toFixed(2)} a${r},${r} 0 0 1 ${r},${r} ` +
          `v${(h - 2 * r).toFixed(2)} a${r},${r} 0 0 1 ${-r},${r} ` +
          `h${(2 * r - w).toFixed(2)} a${r},${r} 0 0 1 ${-r},${-r} ` +
          `v${(2 * r - h).toFixed(2)} a${r},${r} 0 0 1 ${r},${-r} z`
        );
      });
      setClipPath(`path('${parts.join(" ")}')`);
    };

    measure();
    window.addEventListener("resize", measure);
    return () => window.removeEventListener("resize", measure);
  }, []);

  return (
    <>
      {/* Период */}
      <div className={`staking-panel__row staking-panel__row--periods${ex}`}>
        <div className="staking-panel__buttons" ref={buttonsRef}>
          <DropIndicator
            indicator={indicator}
            phase={phase}
            direction={direction}
            variant="panel"
            clipPath={clipPath}
          />
          {PERIODS.map((period) => (
            <PeriodButton
              key={period.id}
              ref={(el) => {
                buttonRefs.current[period.id] = el;
              }}
              days={t[period.daysKey]}
              selected={selectedPeriod === period.id}
              onClick={() => setSelectedPeriod(period.id)}
            />
          ))}
        </div>
        <span className="staking-panel__row-highlight" aria-hidden="true" />
      </div>
    </>
  );
}
