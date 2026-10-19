import { useEffect, useState } from "react";
import { useWallet } from "../WalletContext";
import { getClaimedStakesCount, getUniqueClaimersCount, getClaimedStakes, yoctoToNear } from "../near";
import "./AdminBody.css";

export default function AdminBody() {
  const { refreshKey } = useWallet();
  const [count, setCount] = useState(null);
  const [unique, setUnique] = useState(null);
  const [claims, setClaims] = useState([]);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    Promise.all([getClaimedStakesCount(), getUniqueClaimersCount(), getClaimedStakes(0, 30)])
      .then(([c, u, rows]) => {
        if (cancelled) return;
        setCount(c);
        setUnique(u);
        setClaims([...rows].sort((a, b) => b.unlocks_at - a.unlocks_at));
      })
      .finally(() => !cancelled && setLoading(false));
    return () => {
      cancelled = true;
    };
  }, [refreshKey]);

  return (
    <div className="admin">
      <span className="admin__glass" aria-hidden="true" />

      <div className="admin__section">
        <p className="admin__heading">🔖 Заклеймлено</p>
        <div className="admin__stats-row">
          <div className="admin__stat">
            <span className="admin__stat-value">{count ?? "—"}</span>
            <span className="admin__stat-label">Всего клеймов</span>
          </div>
          <div className="admin__stat">
            <span className="admin__stat-value">{unique ?? "—"}</span>
            <span className="admin__stat-label">Уник. аккаунтов</span>
          </div>
        </div>
      </div>

      <div className="admin__section">
        <p className="admin__heading">История клеймов</p>
        {loading && <div className="admin__empty">…</div>}
        {!loading && claims.length === 0 && <div className="admin__empty">Пока никто не забирал награды</div>}
        {!loading &&
          claims.map((c) => (
            <div className="admin__row" key={c.stake_id}>
              <div className="admin__row-left">
                <span className="admin__row-owner" title={c.owner}>
                  {c.owner.length > 22 ? c.owner.slice(0, 20) + "…" : c.owner}
                </span>
                <span className="admin__row-meta">NFT #{c.token_id} · {c.period_days}д</span>
              </div>
              <span className="admin__row-reward">{yoctoToNear(c.reward_near)} NEAR</span>
            </div>
          ))}
      </div>

      <span className="admin__highlight" aria-hidden="true" />
    </div>
  );
}
