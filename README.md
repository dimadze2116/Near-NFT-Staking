# Near NFT Staking

A Telegram Mini App for staking NFTs and earning **$NEAR** rewards — fully
on-chain, live on NEAR mainnet.

Connect your wallet, lock your NFTs for a chosen period, and earn tiered
rewards based on how many NFTs you stake. No bridges, no wrapping — just
your NFTs, your wallet, and real on-chain yield.

---

## ✨ Features

- **One-tap wallet connect** — sign in with any NEAR wallet, right inside
  Telegram.
- **Flexible staking periods** — lock your NFTs for 7, 14, or 30 days.
- **Tiered APR** — the more NFTs you stake, the higher your reward tier.
- **Batch staking** — stake multiple NFTs in a single wallet signature.
- **Live leaderboard** — see how you rank against other stakers, updated
  in real time from on-chain data.
- **Capped reward pool** — rewards are backed by a fixed monthly budget with
  a hard mathematical ceiling, so payouts can never exceed what's actually
  funded.
- **6 color themes** with light & dark mode for each — pick your favorite
  look and it's remembered across sessions.
- **Multi-language interface** (English / Russian).

## 🛠 How it works

1. Open the Mini App from the Telegram bot.
2. Connect your NEAR wallet.
3. Pick the NFTs you want to stake and choose a lock period.
4. Confirm the transaction — your NFTs are now earning rewards.
5. Track progress in **My Stakes**, compete on the **Leaderboard**, and
   claim your rewards once a stake matures.

## 🧱 Tech stack

- **Smart contract:** Rust, [`near-sdk`](https://github.com/near/near-sdk-rs),
  deployed on NEAR mainnet.
- **Frontend:** React + Vite, running as a Telegram Mini App.
- **Bot backend:** Node.js / TypeScript ([Telegraf](https://telegraf.js.org/)),
  handling Telegram integration and NFT/stake verification.
- **Wallet connection:** [`@hot-labs/near-connect`](https://github.com/hot-dao/near-connect)
  for wallet selection and signing.

## 📐 Reward model

Rewards are calculated per-NFT based on the staker's position tier (1st NFT
staked, 2nd, 3rd, and so on), with the same annualized rate applied
proportionally across 7/14/30-day periods. The total monthly reward budget
and the number of simultaneously active stakes are both capped, guaranteeing
the pool can never be overdrawn regardless of how the tiers are distributed.

## 🔒 Security notes

- The app never has access to private keys or seed phrases — all signing
  happens in the user's own wallet.
- Telegram authentication is verified server-side using Telegram's official
  HMAC signature scheme.
- The staking contract enforces ownership and deposit checks on-chain for
  every stake, unstake, and claim action.

## 📄 License

This project is provided as-is. See the repository for license details.
