import { useState } from "react";
import { useLanguage } from "../i18n";
import nftImage from "../assets/nft-legion-1011.png";
import NearIcon from "./NearIcon";
import "./SellModal.css";

// Оставляем только цифры и одну точку — как в макете дизайнера.
const sanitize = (raw) => {
  const [head, ...rest] = raw.replace(/[^\d.]/g, "").split(".");
  return rest.length ? `${head}.${rest.join("")}` : head;
};

/**
 * Плашка ввода цены перед выставлением NFT на продажу. Живёт в
 * .stake-screen__bottom над навбаром — не портал и не затемняющая подложка.
 * onConfirm(priceString) — вызывается с введённым значением ("1.5" и т.п.),
 * сама конвертация в yoctoNEAR и вызов контракта — на стороне родителя.
 */
export default function SellModal({ nft, submitting, exiting = false, onConfirm, onClose }) {
  const { t } = useLanguage();
  const [price, setPrice] = useState("");

  if (!nft) return null;

  const trimmed = price.trim();
  const valid = /^\d+(\.\d+)?$/.test(trimmed) && Number(trimmed) > 0;

  function submit() {
    if (!valid || submitting) return;
    onConfirm(trimmed);
  }

  return (
    <div className={`sell-modal${exiting ? " sell-modal--exiting" : ""}`}>
      <span className="sell-modal__glass" aria-hidden="true" />
      <div className="sell-modal__content">
        <div className="sell-modal__head">
          <img
            src={nft.image || nftImage}
            alt={nft.name}
            className="sell-modal__thumb"
            onError={(e) => {
              e.currentTarget.src = nftImage;
            }}
          />
          <span className="sell-modal__heading">
            <span className="sell-modal__title">{t.sellNftTitle}</span>
            <span className="sell-modal__subtitle">{nft.name}</span>
          </span>
        </div>

        <label className="sell-modal__field">
          <span className="sell-modal__label">{t.sellPriceLabel}</span>
          <span className="sell-modal__input-wrap">
            <input
              type="text"
              inputMode="decimal"
              className="sell-modal__input"
              placeholder="0.00"
              autoFocus
              value={price}
              disabled={submitting}
              onChange={(e) => setPrice(sanitize(e.target.value))}
              onKeyDown={(e) => {
                if (e.key === "Enter") {
                  e.preventDefault();
                  submit();
                }
              }}
            />
            <NearIcon className="sell-modal__near" />
          </span>
        </label>

        <div className="sell-modal__actions">
          <button
            type="button"
            className="sell-modal__cancel"
            onClick={onClose}
            disabled={submitting}
          >
            {t.sellCancel}
          </button>
          <button
            type="button"
            className="sell-modal__submit"
            disabled={!valid || submitting}
            onClick={submit}
          >
            {submitting ? t.sellSubmitting : t.sellConfirm}
          </button>
        </div>
      </div>
      <span className="sell-modal__highlight" aria-hidden="true" />
    </div>
  );
}
