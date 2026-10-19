import { useState, useRef, useEffect } from "react";
import { useLanguage } from "../i18n";
import { useWallet } from "../WalletContext";
import "./WalletButton.css";

// Сколько ждём второй тап, прежде чем вернуть кнопку в обычное состояние.
const CONFIRM_TIMEOUT_MS = 3000;

function IconArrowOut({ color }) {
  return (
    <svg viewBox="0 0 11.2071 11" width="10" height="10" fill="none" xmlns="http://www.w3.org/2000/svg">
      <path
        d="M5.5 0.5H1C0.723858 0.5 0.5 0.723858 0.5 1V10C0.5 10.2761 0.723858 10.5 1 10.5H5.5M3.625 5.5H10.5M8 8L10.5 5.5L8 3"
        stroke={color}
        strokeLinecap="round"
      />
    </svg>
  );
}

export default function WalletButton() {
  const { t } = useLanguage();
  const { accountId, connecting, connect, disconnect } = useWallet();
  // Отключение — в два тапа: случайное касание не должно рвать сессию.
  const [confirming, setConfirming] = useState(false);
  const confirmTimerRef = useRef(null);

  const clearConfirmTimer = () => {
    if (confirmTimerRef.current) {
      clearTimeout(confirmTimerRef.current);
      confirmTimerRef.current = null;
    }
  };

  // Снимаем таймер при размонтировании и если кошелёк отключился иначе.
  useEffect(() => clearConfirmTimer, []);
  useEffect(() => {
    if (!accountId) {
      clearConfirmTimer();
      setConfirming(false);
    }
  }, [accountId]);

  const handleClick = () => {
    if (!accountId) {
      connect().catch(() => {});
      return;
    }
    if (!confirming) {
      setConfirming(true);
      confirmTimerRef.current = setTimeout(() => {
        confirmTimerRef.current = null;
        setConfirming(false);
      }, CONFIRM_TIMEOUT_MS);
      return;
    }
    clearConfirmTimer();
    setConfirming(false);
    disconnect();
  };

  if (!accountId) {
    return (
      <button type="button" className="wallet-button wallet-button--connect" onClick={handleClick} disabled={connecting}>
        <span>{connecting ? "..." : t.connectLine1}</span>
        <span>{connecting ? "" : t.connectLine2}</span>
      </button>
    );
  }

  const shortAddress = (() => {
    const dotIndex = accountId.lastIndexOf(".");
    const domain = dotIndex !== -1 ? accountId.slice(dotIndex) : "";
    const base = dotIndex !== -1 ? accountId.slice(0, dotIndex) : accountId;
    if (base.length <= 8) return accountId;
    return `${base.slice(0, 5)}...${base.slice(-3)}${domain}`;
  })();

  return (
    <button
      type="button"
      className={`wallet-button ${confirming ? "wallet-button--confirm" : "wallet-button--connected"}`}
      onClick={handleClick}
      aria-label={t.walletDisconnect}
    >
      <span className="wallet-button__info">
        <span className="wallet-button__address">{confirming ? t.walletDisconnectQuestion : shortAddress}</span>
        <span className="wallet-button__status">{confirming ? t.walletDisconnectHint : t.walletConnected}</span>
      </span>
      <IconArrowOut color={confirming ? "#e6dcc0" : "#a26464"} />
    </button>
  );
}
