import { useLanguage } from "../i18n";
import "./MyRankBar.css";

export default function MyRankBar({
  position,
  name,
  nftCount,
  reward,
  exiting = false,
  onClick,
}) {
  const { t } = useLanguage();

  return (
    <div className={`my-rank${exiting ? " my-rank--exiting" : ""}`}>
      <span className="my-rank__glass" aria-hidden="true" />
      <button type="button" className="my-rank__row" onClick={onClick}>
        <div className="my-rank__left">
          <span className="my-rank__position">{position}</span>
          <div className="my-rank__name">
            <span className="my-rank__username">
              {name}
              <span className="my-rank__you">{t.you}</span>
            </span>
            <span className="my-rank__nft-count">{nftCount}</span>
          </div>
        </div>
        <span className="my-rank__reward">{reward}</span>
      </button>
      <span className="my-rank__highlight" aria-hidden="true" />
    </div>
  );
}
