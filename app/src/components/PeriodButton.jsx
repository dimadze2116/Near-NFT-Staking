import "./PeriodButton.css";

export default function PeriodButton({ days, apr, selected = false, onClick, ref }) {
  return (
    <button
      type="button"
      ref={ref}
      className={`period-button${selected ? " period-button--selected" : ""}`}
      onClick={onClick}
      aria-pressed={selected}
    >
      <span className="period-button__days">{days}</span>
      {apr && <span className="period-button__apr">{apr}</span>}
    </button>
  );
}
