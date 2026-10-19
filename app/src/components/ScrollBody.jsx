import NftCard from "./NftCard";
import { useLanguage } from "../i18n";
import "./ScrollBody.css";

export default function ScrollBody({ nfts, selected, onToggle, onSell }) {
  const { t } = useLanguage();

  return (
    <div className="scroll-body">
      <div className="scroll-body__nfts">
        <p className="scroll-body__heading">
          {t.selectNft}
          <span className="scroll-body__heading-arrow">→</span>
          {t.selectPeriod}
        </p>
        <div className="scroll-body__grid">
          {nfts.map((nft) => (
            <NftCard
              key={nft.id}
              name={nft.name}
              tokenId={nft.tokenId}
              image={nft.image}
              selected={selected.has(nft.id)}
              onClick={() => onToggle(nft.id)}
              onSell={onSell ? () => onSell(nft) : undefined}
            />
          ))}
        </div>
      </div>
    </div>
  );
}
