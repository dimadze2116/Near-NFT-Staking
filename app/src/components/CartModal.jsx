import { useLanguage } from "../i18n";
import { yoctoToNear } from "../near";
import nftImage from "../assets/nft-legion-1011.png";
import NearIcon from "./NearIcon";
import "./CartModal.css";

// Крестик — та же геометрия, что у ErrorIcon в Toast.
function CloseIcon() {
  return (
    <svg viewBox="0 0 24 24" fill="none" aria-hidden="true">
      <path
        d="M7 7l10 10M17 7L7 17"
        stroke="currentColor"
        strokeWidth="2.6"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  );
}

/**
 * Плашка корзины. Живёт в .stake-screen__bottom над навбаром — не портал
 * и не затемняющая подложка: фон виден сквозь стекло, как в макете.
 * cart — Map<token_id, listing> (listing содержит name/image/price в yoctoNEAR).
 */
export default function CartModal({ cart, submitting, exiting = false, onRemove, onBuyAll, onClose }) {
  const { t } = useLanguage();

  const items = Array.from(cart.values());
  const totalYocto = items.reduce((sum, item) => sum + BigInt(item.price), 0n);

  return (
    <div className={`cart-modal${exiting ? " cart-modal--exiting" : ""}`}>
      <span className="cart-modal__glass" aria-hidden="true" />
      <div className="cart-modal__content">
        <div className="cart-modal__head">
          <span className="cart-modal__title">{t.marketCartTitle}</span>
          <button
            type="button"
            className="cart-modal__close"
            onClick={onClose}
            disabled={submitting}
            aria-label={t.marketCartTitle}
          >
            <CloseIcon />
          </button>
        </div>

        {items.length === 0 ? (
          <div className="cart-modal__empty">{t.marketCartEmpty}</div>
        ) : (
          <>
            <div className="cart-modal__items">
              {items.map((item) => (
                <div className="cart-modal__item" key={item.token_id}>
                  <img
                    src={item.image || nftImage}
                    alt={item.name}
                    className="cart-modal__thumb"
                    onError={(e) => {
                      e.currentTarget.src = nftImage;
                    }}
                  />
                  <span className="cart-modal__item-info">
                    <span className="cart-modal__item-name">{item.name}</span>
                    <span className="cart-modal__item-price">
                      {yoctoToNear(item.price)}
                      <NearIcon className="cart-modal__near" />
                    </span>
                  </span>
                  <button
                    type="button"
                    className="cart-modal__remove"
                    onClick={() => onRemove(item.token_id)}
                    disabled={submitting}
                    aria-label="Remove"
                  >
                    <CloseIcon />
                  </button>
                </div>
              ))}
            </div>

            <div className="cart-modal__total">
              <span className="cart-modal__total-label">{t.marketCartTotal}</span>
              <span className="cart-modal__total-value">
                {yoctoToNear(totalYocto.toString())}
                <NearIcon className="cart-modal__near" />
              </span>
            </div>

            <button
              type="button"
              className="cart-modal__buy"
              disabled={submitting}
              onClick={() => onBuyAll(items, totalYocto.toString())}
            >
              {submitting ? t.marketBuying : t.marketBuyAll}
            </button>
          </>
        )}
      </div>
      <span className="cart-modal__highlight" aria-hidden="true" />
    </div>
  );
}
