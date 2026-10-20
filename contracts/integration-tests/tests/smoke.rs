//! The first thing to get green: the sandbox can reproduce the mainnet shape of
//! this system at all — the collection at its real account name, the staking
//! contract owning itself, and a stake created through nft_transfer_call.

mod common;

use near_workspaces::types::NearToken;

#[tokio::test]
async fn stake_is_created_through_nft_transfer_call() -> anyhow::Result<()> {
    let ctx = common::setup().await?;
    ctx.mint_to_alice("1001").await?;

    // A 7-day stake in the first tier reserves 0.046411 NEAR; fund the pool
    // with more than that so the solvency guard lets the stake through.
    ctx.deposit_rewards(NearToken::from_millinear(100)).await?;

    assert_eq!(ctx.owner_of("1001").await?, ctx.alice.id().to_string());

    ctx.alice
        .call(ctx.nft.id(), "nft_transfer_call")
        .args_json(serde_json::json!({
            "receiver_id": ctx.staking.id(),
            "token_id": "1001",
            "msg": "7",
        }))
        .deposit(NearToken::from_yoctonear(1))
        .gas(near_workspaces::types::Gas::from_tgas(300))
        .transact()
        .await?
        .into_result()?;

    // The contract now holds the NFT, and the stake is on record.
    assert_eq!(
        ctx.owner_of("1001").await?,
        ctx.staking.id().to_string(),
        "the staking contract should hold the NFT after a successful stake"
    );

    let stakes: serde_json::Value = ctx
        .staking
        .view("get_stakes_for_account")
        .args_json(serde_json::json!({ "account_id": ctx.alice.id() }))
        .await?
        .json()?;
    let list = stakes.as_array().expect("get_stakes_for_account returns an array");
    assert_eq!(list.len(), 1, "exactly one stake expected, got: {stakes}");
    assert_eq!(list[0]["token_id"], "1001");
    assert_eq!(list[0]["period_days"], 7);
    assert_eq!(list[0]["claimed"], false);

    let active: String = ctx
        .staking
        .view("get_active_stakes_count")
        .args_json(serde_json::json!({}))
        .await?
        .json()?;
    assert_eq!(active, "1");

    Ok(())
}
