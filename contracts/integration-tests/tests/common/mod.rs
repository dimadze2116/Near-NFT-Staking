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

    // nearlegion.nfts.tg, one level at a time.
    let sk = SecretKey::from_random(KeyType::ED25519);
    let tg = worker.create_tla("tg".parse()?, sk).await?.into_result()?;
    let nfts = tg
        .create_subaccount("nfts")
        .initial_balance(NearToken::from_near(100))
        .transact()
        .await?
        .into_result()?;
    let legion = nfts
        .create_subaccount("nearlegion")
        .initial_balance(NearToken::from_near(50))
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
}
