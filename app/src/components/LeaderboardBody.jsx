import { useEffect, useState } from "react";
import { useLanguage } from "../i18n";
import { useWallet } from "../WalletContext";
import { getLeaderboard, yoctoToNear } from "../near";
import "./LeaderboardBody.css";

// Приватность: показываем только первые 3 символа + "..." + последний символ.
function shortenWalletName(name) {
  if (!name || name.length <= 5) return name;
  return `${name.slice(0, 3)}...${name.slice(-1)}`;
}

const MEDALS = ["gold", "silver", "bronze"];

export default function LeaderboardBody({ youRowRef, flashYou = false }) {
  const { t } = useLanguage();
  const { accountId, refreshKey } = useWallet();
  const [rows, setRows] = useState([]);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    // Берём с запасом (100), чтобы большинство реальных стейкеров попадало в
    // список и подсветка "моего ранга" могла скроллить к своей строке.
    getLeaderboard(100)
      .then((entries) => {
        if (!cancelled) setRows(entries);
      })
      .finally(() => !cancelled && setLoading(false));
    return () => {
      cancelled = true;
    };
  }, [refreshKey]);

  if (loading) return <div className="leaderboard"><p className="leaderboard__title">{t.topStakers}</p></div>;

  return (
    <div className="leaderboard">
      <span className="leaderboard__glass" aria-hidden="true" />
      <p className="leaderboard__title">{t.topStakers}</p>
      <div className="leaderboard__list">
        {rows.length === 0 && (
          <div style={{ padding: "20px", textAlign: "center", color: "var(--muted, #aaa)" }}>
            {t.noStakers ?? "Пока нет активных стейкеров"}
          </div>
        )}
        {rows.map((entry, i) => {
          const position = i + 1;
          const isYou = accountId && entry.owner === accountId;
          const medal = position <= 3 ? MEDALS[position - 1] : null;
          return (
            <div
              key={entry.owner}
              ref={isYou ? youRowRef : undefined}
              className={`leaderboard__row${medal ? ` leaderboard__row--${medal}` : ""}${
                isYou && flashYou ? " leaderboard__row--flash" : ""
              }`}
            >
              {isYou && <span className="leaderboard__row-glow" aria-hidden="true" />}
              <div className="leaderboard__left">
                <span className="leaderboard__position">{position}</span>
                <div className="leaderboard__name">
                  <span className="leaderboard__username" title={entry.owner}>
                    {shortenWalletName(entry.owner)}
                    {isYou && <span className="leaderboard__you">{t.you}</span>}
                  </span>
                  <span className="leaderboard__nft-count">{entry.nft_count} NFT</span>
                </div>
              </div>
              <span className="leaderboard__reward">{yoctoToNear(entry.total_reward)} Near</span>
            </div>
          );
        })}
      </div>
      <span className="leaderboard__highlight" aria-hidden="true" />
    </div>
  );
}
