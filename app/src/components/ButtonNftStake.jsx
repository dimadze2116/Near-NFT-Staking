import { useLanguage } from "../i18n";
import NearIcon from "./NearIcon";
import "./ButtonNftStake.css";

export default function ButtonNftStake({
  onClick,
  label,
  amount,
  timer,
  compact = false,
  exiting = false,
  disabled = false,
  slideIn = false,
  withNearIcon = false,
}) {
  const { t } = useLanguage();

  return (
    <div
      className={`nft-stake-button${compact ? " nft-stake-button--compact" : ""}${
        exiting ? " nft-stake-button--exiting" : ""
      }${disabled ? " nft-stake-button--disabled" : ""}${slideIn ? " nft-stake-button--slide-in" : ""}`}
    >
      <button type="button" className="nft-stake-button__inner" onClick={onClick} disabled={disabled}>
        <span className="nft-stake-button__label">
          {amount != null ? (
            <>
              {t.stakeFor(amount)} <NearIcon className="nft-stake-button__near-icon" />
            </>
          ) : (
            <>
              {label ?? t.stakeNft}
              {withNearIcon && <NearIcon className="nft-stake-button__near-icon" />}
            </>
          )}
        </span>
        {timer && <span className="nft-stake-button__timer">{timer}</span>}
      </button>
      <span className="nft-stake-button__highlight" aria-hidden="true" />
    </div>
  );
}
