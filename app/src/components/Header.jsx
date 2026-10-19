import { useEffect, useState } from "react";
import WalletButton from "./WalletButton";
import LanguageButton from "./LanguageButton";
import PaletteMenu from "./PaletteMenu";
import { useLanguage } from "../i18n";
import { useWallet } from "../WalletContext";
import { getRewardPool, getStakesForAccount, yoctoToNear } from "../near";
import "./Header.css";

export default function Header() {
  const { t } = useLanguage();
  const { accountId, refreshKey } = useWallet();
  const [pool, setPool] = useState(null);
  const [activeCount, setActiveCount] = useState(null);
  const [pendingReward, setPendingReward] = useState(null);

  useEffect(() => {
    getRewardPool()
      .then((v) => setPool(yoctoToNear(v)))
      .catch(() => setPool(null));
  }, [refreshKey]);

  useEffect(() => {
    if (!accountId) {
      setActiveCount(null);
      setPendingReward(null);
      return;
    }
    getStakesForAccount(accountId)
      .then((stakes) => {
        const active = stakes.filter((s) => !s.claimed);
        setActiveCount(active.length);
        const total = active.reduce((sum, s) => sum + BigInt(s.reward_near || "0"), 0n);
        setPendingReward(yoctoToNear(total.toString()));
      })
      .catch(() => {
        setActiveCount(null);
        setPendingReward(null);
      });
  }, [accountId, refreshKey]);

  const STATS = [
    { value: activeCount ?? "—", label: t.statNftsInStake },
    { value: pendingReward ?? "—", label: t.statPendingNear },
    { value: pool ?? "—", label: t.statNearPool },
  ];

  return (
    <div className="header">
      <span className="header__glass" aria-hidden="true" />
      <div className="header__top">
        <div className="header__title">
          <span>Near</span>
          <span>NFT Staking</span>
        </div>
        <div className="header__actions">
          <PaletteMenu />
          <LanguageButton />
          <WalletButton />
        </div>
      </div>
      <div className="header__stats">
        {STATS.map((stat) => (
          <div className="header__stat" key={stat.label}>
            <span className="header__stat-value">{stat.value}</span>
            <span className="header__stat-label">{stat.label}</span>
          </div>
        ))}
      </div>
      <span className="header__highlight" aria-hidden="true" />
    </div>
  );
}
