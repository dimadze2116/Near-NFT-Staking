import { useEffect, useState } from "react";
import StakeCard from "./StakeCard";
import { useLanguage } from "../i18n";
import { useWallet, AccountChangedError } from "../WalletContext";
import { getStakesForAccount, getNftMetadata, unstakeNft, yoctoToNear } from "../near";
import "./MyStakesBody.css";

function fmtBadge(seconds) {
  const s = Math.max(0, Math.floor(seconds));
  const d = Math.floor(s / 86400);
  const h = Math.floor((s % 86400) / 3600);
  const m = Math.floor((s % 3600) / 60);
  return `${d}d ${h}h ${m}m`;
}

export default function MyStakesBody() {
  const { t } = useLanguage();
  const { accountId, ensureSigner, refreshKey, refresh } = useWallet();
  const [stakes, setStakes] = useState([]);
  const [images, setImages] = useState({});
  const [loading, setLoading] = useState(true);
  const [busyId, setBusyId] = useState(null);

  useEffect(() => {
    if (!accountId) {
      setStakes([]);
      setLoading(false);
      return;
    }
    let cancelled = false;
    setLoading(true);
    getStakesForAccount(accountId)
      .then((rows) => {
        const active = rows.filter((s) => !s.claimed);
        if (cancelled) return;
        setStakes(active);
        // Картинки — отдельно и не блокируя рендер списка (nft_token по каждому token_id)
        Promise.all(active.map((s) => getNftMetadata(s.token_id).then((img) => [s.token_id, img]))).then((pairs) => {
          if (!cancelled) setImages(Object.fromEntries(pairs));
        });
      })
      .finally(() => !cancelled && setLoading(false));
    return () => {
      cancelled = true;
    };
  }, [accountId, refreshKey]);

  const handleUnstake = async (tokenId) => {
    setBusyId(tokenId);
    try {
      const signer = await ensureSigner();
      await unstakeNft(signer, tokenId);
      refresh();
    } catch (e) {
      if (e instanceof AccountChangedError) {
        refresh();
        return;
      }
      console.error("[mystakes] unstake failed", e);
      alert(e.message || String(e));
    } finally {
      setBusyId(null);
    }
  };

  if (!accountId) return <div className="scroll-body__heading">{t.connectWalletFirst ?? "Подключите кошелёк"}</div>;
  if (loading) return <div className="my-stakes-body__empty">…</div>;
  if (stakes.length === 0) return <div className="my-stakes-body__empty">{t.noStakes ?? "У вас пока нет активных стейков"}</div>;

  const now = Date.now() / 1000;

  return (
    <div className="my-stakes-body">
      {stakes.map((s) => {
        const total = s.unlocks_at - s.staked_at;
        const elapsed = now - s.staked_at;
        const progress = Math.min(100, Math.max(0, Math.floor((elapsed / total) * 100)));
        const withdrawInDays = Math.max(0, Math.ceil((s.unlocks_at - now) / 86400));
        return (
          <StakeCard
            key={s.stake_id}
            nftLabel={`NFT #${s.token_id}`}
            daysOverBadge={fmtBadge(elapsed)}
            periodDaysKey={`days${s.period_days}`}
            earnedNear={yoctoToNear(s.reward_near)}
            progress={progress}
            withdrawInDays={withdrawInDays}
            image={images[s.token_id]}
            onUnstake={() => handleUnstake(s.token_id)}
            unstaking={busyId === s.token_id}
          />
        );
      })}
    </div>
  );
}
