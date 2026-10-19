import "./DropIndicator.css";

// Скользящая "капля": стекло + линза, преломляющая контент под ней.
// children — увеличенная копия ряда кнопок для линзы (та же геометрия, без интерактива)
export default function DropIndicator({
  indicator,
  phase,
  direction,
  inset = 0,
  gap = 3,
  variant = "nav",
  clipPath,
  children,
}) {
  if (!indicator) return null;

  return (
    <div
      className={`drop-indicator drop-indicator--${variant}`}
      style={{ inset, clipPath: clipPath || undefined }}
      aria-hidden="true"
    >
      <span
        className={`drop-indicator__pill drop-indicator__pill--${phase} drop-indicator__pill--${direction}`}
        style={{ transform: `translateX(${indicator.left}px)`, width: `${indicator.width}px` }}
      >
        <span className="drop-indicator__shell">
          <span className={`drop-indicator__drop drop-indicator__drop--${variant}`} />
          {children && (
            <span
              className="drop-indicator__lens"
              style={{
                width: `${indicator.rowWidth}px`,
                gap: `${gap}px`,
                transform: `translateX(${-indicator.left}px)`,
              }}
            >
              {children}
            </span>
          )}
        </span>
      </span>
    </div>
  );
}
