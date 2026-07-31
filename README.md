# NEAR NFT Staking Bot

@NearNFTStaking_bot

Telegram bot and Mini App for the **NEAR Legion** NFT collection (`nearlegion.nfts.tg`): NFT-gated access to a private channel, on-chain staking, and a peer-to-peer marketplace.

## What the project does

- Verifies NFT ownership through a NEAR wallet
- Verifies Telegram users through `initData`
- Grants access to a private channel only for approved users
- Opens a Mini App for staking and trading NFTs
- Shows active stakes, reward pool, leaderboard, and reward preview
- Runs a marketplace with escrow: list, cancel, buy (up to 4 NFTs per purchase)
- Sends notifications: matured stakes (daily cron) and NFT sales
- Prevents NFT loss when the reward pool is empty

## On-chain

Both contracts are deployed to NEAR **mainnet** and are written in Rust.

| Contract | Account | Purpose |
| --- | --- | --- |
| Staking | `legion-staking.near` | Stake NFTs for 7/14/30 days, tiered APR, leaderboard |
| Marketplace | `legion-marketplace.near` | Escrow listings, 0.5% fee, multi-item cart |
| NFT collection | `nearlegion.nfts.tg` | The collection itself (third-party, NEP-171) |

## Main user flow

1. User opens the bot and connects a NEAR wallet, signing a message (NEP-413)
2. The bot checks NFT ownership on-chain
3. If the NFT is present, access to the private channel is granted
4. The user opens the Mini App with `/stake`
5. Staking: select NFTs and a period, sent via `nft_transfer_call`
6. If the reward pool is sufficient, the stake is created; if not, the NFT is returned safely
7. Marketplace: list an NFT for sale, cancel a listing, or buy up to 4 NFTs in one transaction
8. Notifications arrive when a stake matures or an NFT is sold

## Components

### Telegram bot

User commands: `/start`, `/status`, `/stake`, `/mystakes`, `/claim`, `/pool`, `/whoami`, `/lang`, `/help`

Admin menu (inline): pool, stakes, claims, users, status, restart, start/stop, health.

Two admin actions — reward pool top-up (`/deposittopup`) and withdrawal (`/withdrawpool`) — **do not execute transactions**. They return a ready-to-paste `near-cli` command with the amount already converted to yoctoNEAR. This is deliberate: the contract owner is the contract account itself, holding a single full-access key, so keeping that key in a server environment variable would mean full control over the contract, not just pool management.

### Mini App

React app built with Vite, served as `public/newapp.html`.

- NEAR wallet connection via `@hot-labs/near-connect`
- Staking flow with period selection and reward preview
- Marketplace: listings grid, sell dialog, cart, purchase
- Leaderboard and personal rank
- 6 colour palettes × light/dark theme
- iOS viewport handling (`viewport-fit=cover`, `--tg-vh`)

### Staking contract (`legion-staking.near`)

Public methods:
`nft_on_transfer`, `unstake`, `deposit_rewards`, `withdraw_rewards`, `prune_claimed_stakes`, `admin_return_nft`, `preview_reward`, `get_stake`, `get_stakes_for_account`, `get_reward_pool`, `get_apr_bps`, `get_contract_stats`, `get_leaderboard`, `get_my_rank`, `get_claimed_stakes`, `get_claimed_stakes_count`, `get_claimed_stake_ids`, `get_unique_claimers_count`, `get_active_stakes_count`, `get_matured_stakes_count`, `get_total_staked`

Notes:
- Stake ownership is derived from `previous_owner_id`, not `sender_id` — under NEP-171 these differ when NEP-178 approvals are used
- Rewards are reserved from the pool at stake creation, so payouts are always covered
- `unstake` returns the NFT and pays the reward, both under callbacks with rollback on failure
- Owner methods require exactly 1 yoctoNEAR (`deposit_rewards` excluded — it takes an arbitrary deposit)
- `admin_return_nft` sends the NFT to the owner recorded in state; the recipient is not a parameter, and the method only works 90 days after unlock

### Marketplace contract (`legion-marketplace.near`)

Public methods:
`nft_on_transfer`, `cancel_listing`, `buy_multiple`, `withdraw_fees`, `admin_return_listing`, `get_listing`, `get_listings`, `get_listings_by_seller`, `get_fees_collected`, `get_fee_bps`, `get_active_listings_count`, `get_marketplace_stats`

Notes:
- Listing price is passed in `msg` of `nft_transfer_call`, in yoctoNEAR
- Fee is 0.5% (50 bps), taken from the sale and accumulated in `fees_collected`
- Up to 4 items per purchase — a gas ceiling, not a product choice
- Duplicate `token_ids` in one purchase are rejected: they would create an undeletable phantom listing
- A failed NFT transfer refunds the buyer in full and puts the listing back on sale
- `cancel_listing` requires exactly 1 yoctoNEAR

## Security

- Telegram `initData` is validated by signature
- NEAR wallet identity is verified through a signed NEP-413 message
- Nonces have limited lifetime
- NFT verification happens before wallet persistence
- An empty reward pool never burns or loses NFTs
- The backend holds **no private keys** for the contracts: every state-changing operation is performed manually by an administrator via `near-cli`
- Contracts are built with `overflow-checks = true` and `panic = "abort"`
- Both contracts went through an internal security review; findings were fixed and redeployed

## Tech stack

Node.js · TypeScript · Telegraf · Express · near-api-js (RPC views) · near-kit (signature verification) · SQLite via `sql.js` · React + Vite · Rust `near-sdk` 5.1

## Deployment

- Bot and Mini App: [Render](https://nft-staking-bot.onrender.com), auto-deploy from `main`, persistent disk mounted at `/opt/render/project/src/data` for the SQLite file
- Design previews: Netlify draft deploys (production deploys consume credits, drafts do not)

## Key files

- `src/bot.ts` — Telegram bot logic and admin menu
- `src/server.ts` — API, webhook, `initData` verification
- `src/near.ts` — NEAR RPC helpers (read-only)
- `src/db.ts` — SQLite storage
- `src/cron.ts` — daily checks at 08:00 and 20:00
- `contract/src/lib.rs` — staking contract
- `public/newapp.html` — Mini App entry point (built React app)
- `ROADMAP.md` — full development history and open items

## Made

IronClaw - local version

AI:
1. DeepSeek V4 Flash from DeepSeek
2. GPT 5.4 Mini from OpenAI
3. Claude Sonnet 5 from Anthropic
4. Claude Opus 4.6 from Anthropic
5. Claude Opus 5.0 from Anthropic
