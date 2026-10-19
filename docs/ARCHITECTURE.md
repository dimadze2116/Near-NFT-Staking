# Architecture

Three parts: two Rust contracts on NEAR, a TypeScript backend, and a React Mini App. The backend never signs contract transactions — users sign their own, and administrative operations are signed manually.

```
Telegram ──► Bot (Telegraf)  ──► SQLite        telegram_id ↔ near_account
   │              │
   │              └─► REST API ◄── Mini App (React)
   │                                   │
   └──► Mini App ──────────────────────┘
                                       │ user signs
                                       ▼
                        NEAR mainnet: staking + marketplace contracts
                                       │
                                       ▼
                             nearlegion.nfts.tg (NEP-171)
```

## Onboarding

A user proves two identities and the system links them:

**Telegram identity** comes from `initData`, which the Mini App passes to the backend. Its HMAC signature is verified with the bot token — which is why that token is treated as a credential, not a configuration value.

**NEAR identity** comes from a NEP-413 signed message. The backend issues a nonce with a limited lifetime, the wallet signs it, and the signature is verified server-side.

Only after NFT ownership is confirmed on-chain is the pairing written to the database. The channel invite follows.

A cron job re-checks ownership twice daily and removes members who sold their NFT and hold no active stakes. RPC failures are distinguished from genuine non-ownership, so a flaky node cannot cause a wrongful removal.

## Staking

Staking happens through `nft_transfer_call` — the NFT moves to the contract and carries the chosen period in `msg`. The contract:

1. Verifies the caller is the NFT collection
2. Validates the period (7, 14 or 30 days)
3. Computes the reward from the account's tier position
4. **Reserves that reward from the pool** and refuses the stake if the pool cannot cover it

That fourth step is the core solvency property: a reward is deducted at creation, not promised at maturity. If the pool is short, `nft_on_transfer` returns `true` and the NFT goes straight back to the sender rather than sitting in a contract that cannot pay for it.

APR is tiered by position — an account's first NFTs earn a lower rate than its tenth. The reward is fixed in state at stake time, so changing the formula later does not retroactively alter existing positions.

`unstake` checks the period has elapsed, marks the record claimed **before** scheduling any promise, returns the NFT, then pays the reward. Both transfers run under callbacks; a failed NFT return rolls the stake back, and a failed reward payment returns the funds to the pool and logs for manual review rather than rolling back a stake whose NFT has already been delivered.

## Marketplace

Listings are escrow: the NFT moves to the contract, price travels in `msg` as yoctoNEAR. The contract holds it until bought or cancelled by the seller.

`buy_multiple` takes up to four token IDs and a deposit covering the total:

1. Rejects duplicates and over-long carts before touching state
2. Removes all listings up front, so a concurrent buy or cancel cannot race the in-flight transfers
3. Schedules one transfer per item, each with its own callback
4. Refunds any excess deposit

Each callback either records the fee and pays the seller, or — if the transfer failed — refunds the buyer in full and restores the listing. The promise chain always terminates with `.then()`, because near-sdk refuses to return a promise assembled with `.and()`.

The fee is 0.5%, accumulated in `fees_collected`. Note that withdrawing fees decreases that counter, so it cannot be used to reconstruct historical volume after a withdrawal.

## Backend

Five modules, deliberately small:

| File | Responsibility |
| --- | --- |
| `bot.ts` | Telegram commands, admin menu, channel management |
| `server.ts` | Webhook, REST API for the Mini App, `initData` verification |
| `near.ts` | NEAR RPC reads only — no signing, no key material |
| `db.ts` | SQLite via `sql.js`, persisted to disk |
| `cron.ts` | Twice-daily ownership checks and maturity notifications |

`near.ts` is read-only by construction. The key-handling code that used to live there was removed rather than guarded, so there is nothing to misconfigure.

The database holds the Telegram↔NEAR pairing, nonces, and a record of which maturity notifications were already sent. It must live on persistent storage: with an ephemeral filesystem it silently resets on each deploy, and every user has to re-link their wallet.

## Mini App

React with Vite, served as a static page the bot opens. Wallet connection goes through `@hot-labs/near-connect`; all transactions are signed by the user's own wallet.

Two implementation details worth knowing:

**CSS minification is disabled** (`cssMinify: false` in `vite.config.js`). The minifier strips `backdrop-filter` in a way that silently removes the glass effect throughout the interface.

**iOS viewport handling** uses a `--tg-vh` custom property rather than `100svh`, applied only on iOS. Telegram's embedded webview reports viewport height differently across platforms, and a uniform fix broke Android.

The cart is capped at four items client-side, matching the contract limit, so a user is told before signing rather than receiving a gas error afterwards.
