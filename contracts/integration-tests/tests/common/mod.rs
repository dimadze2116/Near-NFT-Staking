// Each test binary compiles this module in full, so helpers used by only some
// of them read as dead code in the others. That is how a shared `common` module
// always behaves with Cargo's integration tests.
#![allow(dead_code)]

//! Shared sandbox setup for the staking integration tests.
//!
//! Two things here are not obvious and are the reason this file exists.
//!
//! **The collection's account name is load-bearing.** The staking contract
//! hardcodes `NFT_CONTRACT_ID = "nearlegion.nfts.tg"` and refuses
//! `nft_on_transfer` from anywhere else. So the sandbox has to reproduce that
//! exact name: a top-level `tg`, then `nfts.tg`, then `nearlegion.nfts.tg`.
//! Patching the constant for tests was the alternative and was rejected — it
//! would mean testing a binary that is not the one deployed.
//!
//! **The wasm is built outside cargo test.** Since near-sdk 5.27 a contract has
//! to be compiled with `cargo near build`, which supplies `--cfg near`; a plain
//! cargo build produces a binary that deploys and then traps. near-workspaces'
//! own `compile_project` does a plain build, so it cannot be used here. Both
//! artefacts are built beforehand (see tests/README.md) and read from disk.

use near_workspaces::types::{KeyType, NearToken, SecretKey};
use near_workspaces::{Account, Contract, Worker};
use near_workspaces::network::Sandbox;

/// Default artefact locations, relative to this crate's root (cargo test's cwd).
const STAKING_WASM: &str = "../staking/target/near/nft_staking.wasm";
const MOCK_NFT_WASM: &str = "../mock-nft/target/near/mock_nft.wasm";

pub struct Ctx {
    /// Used by tests that rewrite state directly — patching `staked_at` is how
    /// a matured stake is produced without waiting out 604,800 blocks.
    pub worker: Worker<Sandbox>,
    /// The collection, deployed at nearlegion.nfts.tg
    pub nft: Contract,
    /// The staking contract. Its owner is itself, as on mainnet.
    pub staking: Contract,
    pub alice: Account,
}

fn read_wasm(env_key: &str, default_path: &str) -> anyhow::Result<Vec<u8>> {
    let path = std::env::var(env_key).unwrap_or_else(|_| default_path.to_string());
    std::fs::read(&path).map_err(|e| {
        anyhow::anyhow!(
            "cannot read {path}: {e}\n\
             Build it first with `cargo near build non-reproducible-wasm --no-abi`, \
             or point {env_key} at the artefact."
        )
    })
}

pub async fn setup() -> anyhow::Result<Ctx> {
    let worker = near_workspaces::sandbox().await?;

    // nearlegion.nfts.tg, one level at a time. The balances shrink down the
    // chain on purpose: a new top-level account is funded with 100 NEAR, so
    // handing all of it to the child leaves nothing for the gas of that very
    // transaction and fails with NotEnoughBalance.
    let sk = SecretKey::from_random(KeyType::ED25519);
    let tg = worker.create_tla("tg".parse()?, sk).await?.into_result()?;
    let nfts = tg
        .create_subaccount("nfts")
        .initial_balance(NearToken::from_near(50))
        .transact()
        .await?
        .into_result()?;
    let legion = nfts
        .create_subaccount("nearlegion")
        .initial_balance(NearToken::from_near(25))
        .transact()
        .await?
        .into_result()?;

    let nft = legion
        .deploy(&read_wasm("MOCK_NFT_WASM", MOCK_NFT_WASM)?)
        .await?
        .into_result()?;
    nft.call("new").transact().await?.into_result()?;
    assert_eq!(
        nft.id().as_str(),
        "nearlegion.nfts.tg",
        "the collection must sit at the account the staking contract expects"
    );

    let staking = worker
        .dev_deploy(&read_wasm("STAKING_WASM", STAKING_WASM)?)
        .await?;
    // On mainnet the contract owns itself; keep that, so the owner-only paths
    // are exercised the same way.
    staking
        .call("new")
        .args_json(serde_json::json!({ "owner_id": staking.id() }))
        .transact()
        .await?
        .into_result()?;

    let alice = worker
        .dev_create_account()
        .await?;

    Ok(Ctx { worker, nft, staking, alice })
}

impl Ctx {
    /// Mints `token_id` to `alice`.
    pub async fn mint_to_alice(&self, token_id: &str) -> anyhow::Result<()> {
        self.nft
            .call("nft_mint")
            .args_json(serde_json::json!({
                "token_id": token_id,
                "receiver_id": self.alice.id(),
            }))
            .transact()
            .await?
            .into_result()?;
        Ok(())
    }

    /// Tops the reward pool up from the contract's own balance, the way it is
    /// done on mainnet: the contract is its own owner, so it signs for itself.
    pub async fn deposit_rewards(&self, amount: NearToken) -> anyhow::Result<()> {
        self.staking
            .call("deposit_rewards")
            .deposit(amount)
            .gas(near_workspaces::types::Gas::from_tgas(30))
            .transact()
            .await?
            .into_result()?;
        Ok(())
    }

    pub async fn owner_of(&self, token_id: &str) -> anyhow::Result<String> {
        let token: serde_json::Value = self
            .nft
            .view("nft_token")
            .args_json(serde_json::json!({ "token_id": token_id }))
            .await?
            .json()?;
        Ok(token
            .get("owner_id")
            .and_then(|v| v.as_str())
            .unwrap_or("<none>")
            .to_string())
    }

    /// Stakes `token_id` for `period_days` through nft_transfer_call, the way
    /// the Mini App does it.
    pub async fn stake(&self, token_id: &str, period_days: u8) -> anyhow::Result<()> {
        self.alice
            .call(self.nft.id(), "nft_transfer_call")
            .args_json(serde_json::json!({
                "receiver_id": self.staking.id(),
                "token_id": token_id,
                "msg": period_days.to_string(),
            }))
            .deposit(NearToken::from_yoctonear(1))
            .gas(near_workspaces::types::Gas::from_tgas(300))
            .transact()
            .await?
            .into_result()?;
        Ok(())
    }

    pub async fn alice_balance(&self) -> anyhow::Result<u128> {
        Ok(self.alice.view_account().await?.balance.as_yoctonear())
    }

    pub async fn staking_balance(&self) -> anyhow::Result<u128> {
        Ok(self
            .staking
            .as_account()
            .view_account()
            .await?
            .balance
            .as_yoctonear())
    }

    pub async fn reward_pool(&self) -> anyhow::Result<u128> {
        let pool: String = self
            .staking
            .view("get_reward_pool")
            .args_json(serde_json::json!({}))
            .await?
            .json()?;
        Ok(pool.parse()?)
    }

    /// Moves a stake's `staked_at` back by `seconds`, which is how a matured
    /// position is produced in a test.
    ///
    /// Waiting is not an option: the shortest period is 7 days, about 604,800
    /// blocks, and `fast_forward` costs roughly five seconds per epoch crossed.
    /// So the record is rewritten directly through the sandbox's patch_state.
    ///
    /// The record is found by its contents rather than by computing its storage
    /// key, which keeps this independent of how near-sdk lays out collection
    /// prefixes. StakeInfo is borsh-encoded as:
    ///
    ///   owner       u32 little-endian length + utf8
    ///   token_id    u32 little-endian length + utf8
    ///   staked_at   u64 little-endian      <- the field being rewritten
    ///   period_days u8
    ///   reward_near u128 little-endian
    ///   claimed     u8
    ///
    /// Matching owner and token_id *and* requiring the value to end exactly
    /// where that layout says it should makes the identification unambiguous.
    pub async fn backdate_stake(&self, token_id: &str, seconds: u64) -> anyhow::Result<()> {
        let owner = self.alice.id().to_string();
        let state = self.staking.view_state().await?;

        let mut seen: Vec<String> = Vec::new();
        for (key, value) in &state {
            match staked_at_offset(value, &owner, token_id) {
                Some(offset) => {
                    let mut patched = value.clone();
                    let current = u64::from_le_bytes(value[offset..offset + 8].try_into()?);
                    let moved = current.saturating_sub(seconds);
                    patched[offset..offset + 8].copy_from_slice(&moved.to_le_bytes());
                    self.worker
                        .patch_state(self.staking.id(), key, &patched)
                        .await?;
                    return Ok(());
                }
                None => seen.push(describe_entry(key, value)),
            }
        }
        anyhow::bail!(
            "no stake record for {owner}:{token_id} among {} state entries\n{}",
            state.len(),
            seen.join("\n")
        )
    }
}

/// Offset of `staked_at` inside a borsh-encoded StakeInfo, or None if this
/// state entry is something else.
///
/// Layout:
///   owner       u32 little-endian length + utf8
///   token_id    u32 little-endian length + utf8
///   staked_at   u64 little-endian      <- what gets rewritten
///   period_days u8
///   reward_near u128 little-endian
///   claimed     u8
///
/// Anything after that is left alone and not assumed: near-sdk's IterableMap
/// stores a value together with bookkeeping of its own, and the exact shape of
/// that tail is an implementation detail. Identification rests on owner and
/// token_id matching and on period_days being one of the three the contract
/// accepts, which no other record in this contract's state satisfies.
fn staked_at_offset(value: &[u8], owner: &str, token_id: &str) -> Option<usize> {
    let (found_owner, next) = read_str(value, 0)?;
    if found_owner != owner {
        return None;
    }
    let (found_token, staked_at) = read_str(value, next)?;
    if found_token != token_id {
        return None;
    }
    // staked_at(8) + period_days(1) + reward_near(16) + claimed(1)
    if value.len() < staked_at + 26 {
        return None;
    }
    match value.get(staked_at + 8) {
        Some(7) | Some(14) | Some(30) => Some(staked_at),
        _ => None,
    }
}

fn read_str(v: &[u8], at: usize) -> Option<(String, usize)> {
    let len = u32::from_le_bytes(v.get(at..at + 4)?.try_into().ok()?) as usize;
    if len == 0 || len > 128 {
        return None;
    }
    let bytes = v.get(at + 4..at + 4 + len)?;
    Some((String::from_utf8(bytes.to_vec()).ok()?, at + 4 + len))
}

/// Printed only when the lookup fails, so one failing run is enough to see the
/// real layout instead of guessing at it again.
fn describe_entry(key: &[u8], value: &[u8]) -> String {
    let key_hex: String = key.iter().take(12).map(|b| format!("{b:02x}")).collect();
    let strings = match read_str(value, 0) {
        Some((first, next)) => match read_str(value, next) {
            Some((second, after)) => format!(
                "str1={first:?} str2={second:?} next_byte={:?}",
                value.get(after + 8)
            ),
            None => format!("str1={first:?}"),
        },
        None => "no leading string".to_string(),
    };
    format!("  key={key_hex} len={} {strings}", value.len())
}
