import { useEffect, useState } from "react";
import { useLanguage } from "../i18n";
import { useWallet, AccountChangedError } from "../WalletContext";
import { getMarketListings, getNftInfo, cancelListing, yoctoToNear } from "../near";
import nftImage from "../assets/nft-legion-1011.png";
import NearIcon from "./NearIcon";
import "./MarketBody.css";

export default function MarketBody({ cart, onToggleCart, showToast }) {
  const { t } = useLanguage();
  const { accountId, ensureSigner, refreshKey, refresh } = useWallet();
  const [listings, setListings] = useState([]);
  const [loading, setLoading] = useState(true);
  const [cancellingId, setCancellingId] = useState(null);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      setLoading(true);
      try {
        const raw = await getMarketListings(0, 50);
        const enriched = await Promise.all(
          raw.map(async (item) => {
            const info = await getNftInfo(item.token_id);
            return { ...item, name: info.name, image: info.image };
          })
        );
        if (!cancelled) setListings(enriched);
      } catch (e) {
        console.error("[market] load listings failed", e);
      } finally {
        if (!cancelled) setLoading(false);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [refreshKey]);

  async function handleCancel(tokenId) {
    if (cancellingId) return;
    setCancellingId(tokenId);
    try {
      const signer = await ensureSigner();
      await cancelListing(signer, tokenId);
      showToast({ variant: "success", title: t.toastCancelSuccessTitle, text: t.toastCancelSuccessText });
      refresh();
    } catch (e) {
      if (e instanceof AccountChangedError) {
        showToast({ variant: "error", title: t.toastAccountChangedTitle, text: t.toastAccountChangedText });
        refresh();
        return;
      }
      console.error("[market] cancel failed", e);
      showToast({ variant: "error", title: t.toastCancelFailTitle, text: e.message || t.toastCancelFailText });
    } finally {
      setCancellingId(null);
    }
  }

  if (loading) return null;

  if (listings.length === 0) {
    return <div className="scroll-body__heading">{t.marketEmpty}</div>;
  }

  return (
    <div className="market-body">
      <p className="scroll-body__heading">{t.marketTitle}</p>
      <div className="market-body__grid">
        {listings.map((item) => {
          const isMine = item.seller === accountId;
          const inCart = cart.has(item.token_id);
          return (
            <div className="market-card" key={item.token_id}>
              <span className="market-card__glass" aria-hidden="true" />
              <div className="market-card__image-wrap">
                <img
                  src={item.image || nftImage}
                  alt={item.name}
                  className="market-card__image"
                  onError={(e) => {
                    e.currentTarget.src = nftImage;
                  }}
                />
              </div>
              <div className="market-card__info">
                <div className="market-card__name">{item.name}</div>
                <div className="market-card__price">
                  {yoctoToNear(item.price)} <NearIcon className="market-card__near-icon" />
                </div>
              </div>
              {isMine ? (
                <button
                  type="button"
                  className="market-card__btn market-card__btn--cancel"
                  disabled={cancellingId === item.token_id}
                  onClick={() => handleCancel(item.token_id)}
                >
                  {cancellingId === item.token_id ? t.sellSubmitting : t.marketCancelListing}
                </button>
              ) : (
                <button
                  type="button"
                  className={`market-card__btn${inCart ? " market-card__btn--in-cart" : ""}`}
                  onClick={() => onToggleCart(item)}
                >
                  {inCart ? t.marketRemoveFromCart : t.marketAddToCart}
                </button>
              )}
            </div>
          );
        })}
      </div>
    </div>
  );
}
