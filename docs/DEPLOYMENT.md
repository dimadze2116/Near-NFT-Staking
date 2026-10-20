# Deployment

Mostly ordinary, with a handful of non-obvious parts that cost real debugging time and are worth writing down.

## Building contracts

```bash
cd contracts/staking      # or contracts/marketplace
cargo near build non-reproducible-wasm --no-abi
```

**Only this command works.** A plain `cargo build --target wasm32-unknown-unknown` compiles cleanly, deploys successfully, and then fails at `new()` with `WasmTrap(Unreachable)`. Since near-sdk 5.27 the `--cfg near` flag is required, and only `cargo near build` supplies it. The failure mode is deceptive: ordinary calls work fine, so a contract built the wrong way can appear healthy right up until it needs to initialise or migrate state.

`--no-abi` is needed because ABI generation links against the host platform rather than WASM.

## Deploying over existing state

```bash
near contract deploy legion-staking.near \
  use-file target/near/nft_staking.wasm \
  without-init-call \
  network-config mainnet sign-with-keychain send
```

`without-init-call` is mandatory on redeploy — the contract is already initialised, and `new()` must not run again.

None of the changes shipped so far altered the state layout, so no migration was required and live positions survived every deployment untouched. Adding a field to the contract struct *would* require a migration, which is why pending work of that kind is deferred rather than bundled into a routine update.

## Verifying what is deployed

**Do not compare checksums.** `non-reproducible-wasm` means the same source yields different bytes on every build, so a local hash and the on-chain hash diverge legitimately. This is confusing enough to cost an afternoon if you assume otherwise.

Verify by behaviour instead — call a method that exists only in the new version:

```bash
near contract call-function as-read-only legion-staking.near admin_return_nft \
  json-args '{"stake_id": "anything"}' network-config mainnet now
```

The new build answers `attach exactly 1 yoctoNEAR` — the method resolved and failed its access check. An old build answers `MethodResolveError(MethodNotFound)`.

## Turning the counters on, and off

The staking contract keeps its counters under separate storage keys rather than
as struct fields, so a redeploy needs no migration and `without-init-call`
remains correct. The order matters, and each step is checkable before the next:

```bash
# 1. Ordinary redeploy. Nothing observable changes yet.
near contract deploy legion-staking.near use-file target/near/nft_staking.wasm \
  without-init-call network-config mainnet sign-with-keychain send

# 2. Confirm the new code is in and still scanning: ready must be false,
#    and the *_counter fields are meaningless while it is.
near contract call-function as-read-only legion-staking.near audit_counters \
  json-args '{}' network-config mainnet now

# 3. Compute the counters from existing state.
near contract call-function as-transaction legion-staking.near init_counters \
  json-args '{}' prepaid-gas '150 Tgas' attached-deposit '1 yoctoNEAR' \
  sign-as legion-staking.near network-config mainnet sign-with-keychain send

# 4. Confirm they agree with a full scan: active_counter == active_real and
#    claimed_counter == claimed_real.
near contract call-function as-read-only legion-staking.near audit_counters \
  json-args '{}' network-config mainnet now
```

If step 4 disagrees, or anything else looks wrong, `reset_counters` deletes the
keys and the contract goes back to scanning — the same behaviour as step 2. That
is the whole reason the counters are not struct fields: the way back is one
call, not a second migration.

`audit_counters` pays for a full scan, so it is for moments like these rather
than for routine monitoring.

## Pausing

Both contracts can be paused, and both refuse only the way in:

```bash
near contract call-function as-transaction legion-staking.near pause \
  json-args '{}' prepaid-gas '30 Tgas' attached-deposit '1 yoctoNEAR' \
  sign-as legion-staking.near network-config mainnet sign-with-keychain send
```

`unstake` and `cancel_listing` keep working while paused. That is deliberate and
tested: a pause that could hold someone else's NFT would be a hazard rather
than a safeguard.

## Hosting the backend

The bot needs to be always-on: it receives Telegram webhooks and runs cron at fixed times. A host that sleeps idle containers will silently skip scheduled notifications, since a sleeping process runs no timers and missed cron firings are not replayed.

It also needs **persistent storage**. The database is a file written through `sql.js`; on an ephemeral filesystem it resets on every deploy and every user has to re-link their wallet. The symptom in logs is a cheerful `[DB] Initialised` with an empty database.

## Operating the contracts

Administrative operations are run manually, by design — see [`SECURITY.md`](SECURITY.md) for why no key lives on the server.

```bash
# Top up the reward pool (opens staking if the pool was empty)
near contract call-function as-transaction legion-staking.near deposit_rewards \
  json-args '{}' prepaid-gas '30 Tgas' attached-deposit '10 NEAR' \
  sign-as legion-staking.near \
  network-config mainnet sign-with-keychain send

# Withdraw from the pool — amount in yoctoNEAR, requires 1 yoctoNEAR
near contract call-function as-transaction legion-staking.near withdraw_rewards \
  json-args '{"amount": "1000000000000000000000000"}' \
  prepaid-gas '50 Tgas' attached-deposit '1 yoctoNEAR' \
  sign-as legion-staking.near \
  network-config mainnet sign-with-keychain send
```

`sign-as` is mandatory on `as-transaction` and easy to miss: without it near-cli-rs
answers `unexpected argument 'network-config' found`, which reads like a syntax
error in the network options rather than a missing signer. Here the signer is the
contract account itself, because that account is its own owner.

Note the difference between `deposit_rewards` and a plain transfer to the contract account. The former increases `reward_pool` and re-opens staking; the latter only tops up the balance, which is what you want when covering existing obligations without accepting new positions.

## Checking solvency

Reserved rewards must stay covered by the account balance, accounting for the storage stake NEAR locks (1 NEAR per 100 KB):

```bash
near contract call-function as-read-only legion-staking.near get_leaderboard \
  json-args '{"limit": 50}' network-config mainnet now

near account view-account-summary legion-staking.near network-config mainnet now
```

The leaderboard sums rewards across unclaimed stakes only, which makes it the quickest way to total outstanding obligations. Compare that against `balance − (storage_KB / 100)`. The margin should be positive; if it is not, the last users to unstake will receive their NFT but see the reward payment fail — logged explicitly rather than lost silently.
