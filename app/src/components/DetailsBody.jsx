import { useLanguage } from "../i18n";
import "./DetailsBody.css";

const APR_ROWS = [
  ["1 - 10", "24.2%", "24.2%", "24.2%"],
  ["11 - 20", "29.0%", "29.0%", "29.0%"],
  ["21 - 30", "33.2%", "33.2%", "33.2%"],
  ["31 - 40", "37.3%", "37.3%", "37.3%"],
  ["41 - 50", "41.5%", "41.5%", "41.5%"],
  ["51+", "42.1%", "42.1%", "42.1%"],
];

export default function DetailsBody() {
  const { t } = useLanguage();

  return (
    <div className="details">
      <span className="details__glass" aria-hidden="true" />

      <div className="details__section">
        <p className="details__heading">{t.dynamicApr}</p>
        <div className="details__apr-table">
          {t.aprHeaders.map((header, colIndex) => (
            <div className="details__apr-col" key={header}>
              <span className="details__apr-col-header">{header}</span>
              {APR_ROWS.map((row, rowIndex) => (
                <span className="details__apr-col-value" key={rowIndex}>
                  {row[colIndex]}
                </span>
              ))}
            </div>
          ))}
        </div>
      </div>

      <div className="details__section">
        <p className="details__heading">{t.howItWorks}</p>
        <ol className="details__list">
          {t.howItWorksSteps.map((step) => (
            <li key={step}>{step}</li>
          ))}
        </ol>
      </div>

      <div className="details__section">
        <p className="details__heading">{t.contracts}</p>
        <p className="details__contract">
          {t.contractNft} <span className="details__contract-value">nearlegion.nfts.tg</span>
        </p>
        <p className="details__contract">
          {t.contractStaking} <span className="details__contract-value">legion-staking.near</span>
        </p>
      </div>

      <span className="details__highlight" aria-hidden="true" />
    </div>
  );
}
