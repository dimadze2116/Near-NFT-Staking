# Security

Both contracts hold third-party assets in escrow — staked NFTs and listed NFTs belong to users, not to the project. This document records what was done about that.

## Internal security review

A line-by-line review of both contracts covering access control, escrow integrity, callback handling, protection against double-claims and state reuse, gas budgeting, and reward arithmetic with respect to overflow.

It produced **15 findings**. Fourteen are fixed and redeployed to mainnet; the one remaining requires a state migration and is scheduled for when the staking contract clears of active positions.

### The most serious finding

In the staking contract, the owner of a position was recorded from `sender_id` — the account that initiated the transfer — rather than `previous_owner_id`, the account that actually owned the NFT.

Under NEP-171 these are the same account in the normal flow, but they diverge when NEP-178 approvals are in play. An account holding an approval on someone else's NFT could call `nft_transfer_call` against the staking contract, become the recorded owner of the stake, and on unstake walk away with both the NFT and the reward.

This was the only finding exploitable by an outsider holding no keys of any kind. The marketplace contract made the correct choice in the equivalent place — a copy-paste divergence between two crates.

```rust
// Before: owner taken from the caller
pub fn nft_on_transfer(&mut self, sender_id: AccountId, ...) {
    let stake_id = format!("{}:{}", sender_id, token_id);
    // ...
    owner: sender_id.clone(),

// After: owner taken from the NFT's previous owner
pub fn nft_on_transfer(&mut self, #[allow(unused_variables)] sender_id: AccountId,
                       previous_owner_id: AccountId, ...) {
    let owner_id = previous_owner_id;
```

### Other findings of note

**An administrative method could irreversibly lock every staked NFT.** `clear_all_stakes` wiped all stake records and zeroed the reward pool. Since NFTs physically sit on the contract and the only route back is `unstake`, which requires the record to exist, one call would have stranded every user's NFT permanently — the contract had no emergency exit. The method was removed and replaced with `prune_claimed_stakes`, which is paginated and only touches records whose NFTs have already been returned.

**Duplicate cart entries created an undeletable listing.** With the same `token_id` twice in one purchase, the listing was removed once but two transfers were scheduled. The first succeeded; the second failed, and its callback restored a listing whose NFT the contract no longer held. That listing could then be neither bought, nor cancelled, nor relisted. Duplicates are now rejected before any state changes.

**A buyer could lose funds on a failed transfer.** When an NFT transfer failed, the listing returned to sale but the buyer's deposit stayed on the contract — unreachable, since `withdraw_fees` is bounded by `fees_collected` and the funds never entered it. The buyer now receives a full refund.

**Multi-item purchases never worked at all.** `buy_multiple` assembled transfers with `.and()` and returned the result, which near-sdk refuses: *"Returning joint promise is currently prohibited."* Single-item purchases worked because `.and()` was never reached. This was not caught by code review — it only manifests at runtime — and was found by testing the flow on mainnet. The chain now always terminates with `.then()`.

**Privileged methods lacked a 1 yoctoNEAR requirement**, so they could in principle be invoked with a function-call access key. Added across both contracts, with the deliberate exception of `deposit_rewards`, which accepts an arbitrary deposit by design.

**View methods panicked on malformed data.** A record with a non-standard period would take down contract-wide statistics for every user — and, worse, the same call sat in `unstake`, meaning the owner of such a record could not retrieve their own NFT. Reads and `unstake` now degrade gracefully.

## Design decisions

### No private keys on the server

The owner of both contracts is the contract account itself, and each holds a single full-access key. An environment variable containing that key does not grant "pool management" — it grants redeployment, state wipe, and fund withdrawal.

The bot therefore performs no state-changing contract calls. Its admin commands generate a ready-to-paste `near-cli` command with the amount already converted to yoctoNEAR, and the administrator signs it from their own machine. Adding `assert_one_yocto` does not help here: that defends against function-call keys, and a full-access key attaches deposits freely.

### Emergency recovery, deliberately constrained

Both contracts gained a method to return a stuck asset, because neither had one. Common practice is `admin_return_asset(id, recipient)`. That was not the choice made here:

- **The recipient is not a parameter.** It is read from contract state — `stake.owner` or `listing.seller`. The owner can initiate a return but cannot redirect the asset.
- **Staking adds a 90-day grace period** past unlock. A live or recently matured position cannot be touched.

This gives up flexibility in exchange for removing the theft vector that the canonical form carries.

### Gas budgets from measurement

Gas was originally reserved by estimate — 50 TGas for `nft_transfer`, 10 for callbacks. Measured against mainnet, `nft_transfer` on this collection burns **3.53 TGas** and a comparable callback **1.37 TGas**.

The over-reservation was not merely wasteful: at 50 TGas per item, four cart positions did not fit inside the 300 TGas transaction ceiling once the seller-payout callback was added. Budgets were recalculated from the measured figures, which kept the cart at four items instead of dropping it to three.

### Build profile

Both crates are standalone — there is no workspace above them, so `[profile.release]` actually applies. This was verified rather than assumed, because a workspace silently discards member profiles and would have disabled:

```toml
overflow-checks = true
panic = "abort"
```

## Reporting

Found something? Open an issue, or reach the maintainer privately for anything exploitable.
