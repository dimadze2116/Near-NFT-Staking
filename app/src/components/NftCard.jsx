import nftImage from "../assets/nft-legion-1011.png";
import "./NftCard.css";

function SellIcon() {
  return (
    <svg viewBox="0 0 24 24" width="15" height="15" fill="none" aria-hidden="true">
      <path
        d="M20 12.5 12.5 20a2 2 0 0 1-2.83 0l-6.34-6.34a2 2 0 0 1 0-2.83L11 3.17A2 2 0 0 1 12.41 2.5H19a1.5 1.5 0 0 1 1.5 1.5v6.59a2 2 0 0 1-.5 1.41Z"
        stroke="currentColor"
        strokeWidth="1.6"
        strokeLinejoin="round"
      />
      <circle cx="15.5" cy="7.5" r="1.4" fill="currentColor" />
    </svg>
  );
}

export default function NftCard({ name = "Near Legion #1011", tokenId = "#1011", image, selected = false, onClick, onSell }) {
  return (
    <div className={`nft-card${selected ? " nft-card--active" : ""}`}>
      {selected && <span className="nft-card__glow" aria-hidden="true" />}
      <button type="button" className="nft-card__inner" onClick={onClick} aria-pressed={selected}>
        <span className="nft-card__glass" aria-hidden="true" />
        <span className="nft-card__image-wrap">
          <img
            src={image || nftImage}
            alt={name}
            className="nft-card__image"
            onError={(e) => {
              e.currentTarget.src = nftImage;
            }}
          />
        </span>
        <span className="nft-card__info">
          <span className="nft-card__name">{name}</span>
          <span className="nft-card__id">{tokenId}</span>
        </span>
        <span className="nft-card__highlight" aria-hidden="true" />
      </button>
      {onSell && (
        <button
          type="button"
          className="nft-card__sell-btn"
          aria-label="Sell"
          onClick={(e) => {
            e.stopPropagation();
            onSell();
          }}
        >
          <SellIcon />
        </button>
      )}
    </div>
  );
}
