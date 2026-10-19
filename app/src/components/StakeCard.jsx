import nftImage from "../assets/nft-legion-1011.png";
import ButtonNftStake from "./ButtonNftStake";
import { useLanguage } from "../i18n";
import "./StakeCard.css";

export default function StakeCard({
  nftLabel = "NFT #966",
  daysOverBadge = "3d",
  periodDaysKey = "days7",
  earnedNear = "0.0115",
  progress = 43,
  withdrawInDays = 4,
  image,
  onUnstake,
  unstaking = false,
}) {
  const { t } = useLanguage();
  return (
    <div className="stake-card">
      <img
        src={image || nftImage}
        alt=""
        className="stake-card__bg"
        onError={(e) => {
          e.currentTarget.src = nftImage;
        }}
      />
      <div className="stake-card__title">
        <span className="stake-card__badge">{nftLabel}</span>
        <span className="stake-card__badge stake-card__badge--days">{daysOverBadge}</span>
      </div>
      <div className="stake-card__stake">
        <div className="stake-card__stats">
          <div className="stake-card__stat">
            <span className="stake-card__stat-label">{t.period}</span>
            <span className="stake-card__stat-value">{t[periodDaysKey]}</span>
          </div>
          <div className="stake-card__stat">
            <span className="stake-card__stat-label">{t.earnedNear}</span>
            <span className="stake-card__stat-value">{earnedNear}</span>
          </div>
          <div className="stake-card__stat">
            <span className="stake-card__stat-label">{t.progress}</span>
            <span className="stake-card__stat-value">{progress}%</span>
          </div>
        </div>
        <div className="stake-card__progress">
          <span className="stake-card__progress-track" />
          <span className="stake-card__progress-fill" style={{ width: `${progress}%` }} />
        </div>
        {progress >= 100 ? (
          <ButtonNftStake compact label={unstaking ? "..." : t.unstakeNft} onClick={onUnstake} disabled={unstaking} />
        ) : (
          <div className="stake-card__withdraw">
            <span className="stake-card__withdraw-text">{t.withdrawIn(withdrawInDays)}</span>
          </div>
        )}
      </div>
    </div>
  );
}
