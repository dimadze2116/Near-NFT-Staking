//! The full staking cycle, which is the path real holders use: stake, wait out
//! the period, take the NFT and the reward back.

mod common;

use near_workspaces::types::{Gas, NearToken};

#[tokio::test]
async fn stake_matures_then_unstake_returns_nft_and_reward() -> anyhow::Result<()> {
    let ctx = common::setup().await?;
    ctx.mint_to_alice("2001").await?;
    ctx.deposit_rewards(NearToken::from_millinear(500)).await?;

    ctx.stake("2001", 7).await?;
    assert_eq!(ctx.owner_of("2001").await?, ctx.staking.id().to_string());

    // Before the period elapses the position must not be claimable.
    let early = ctx
        .alice
        .call(ctx.staking.id(), "unstake")
        .args_json(serde_json::json!({ "token_id": "2001" }))
        .deposit(NearToken::from_yoctonear(1))
        .gas(Gas::from_tgas(100))
        .transact()
        .await?;
    assert!(
        early.is_failure(),
        "unstake succeeded before the period elapsed: {early:#?}"
    );
    assert_eq!(
        ctx.owner_of("2001").await?,
        ctx.staking.id().to_string(),
        "a refused unstake must leave the NFT with the contract"
    );

    // Eight days back is past the 7-day unlock with room to spare.
    ctx.backdate_stake("2001", 8 * 24 * 60 * 60).await?;

    let stake: serde_json::Value = ctx
        .staking
        .view("get_stake")
        .args_json(serde_json::json!({ "account_id": ctx.alice.id(), "token_id": "2001" }))
        .await?
        .json()?;
    assert_eq!(stake["is_mature"], true, "stake should read as matured: {stake}");
    assert_eq!(stake["claimed"], false);
    let reward: u128 = stake["reward_near"].as_str().unwrap().parse()?;
    assert!(reward > 0, "a stake must carry a reserved reward");

    let before = ctx.alice_balance().await?;

    ctx.alice
        .call(ctx.staking.id(), "unstake")
        .args_json(serde_json::json!({ "token_id": "2001" }))
        .deposit(NearToken::from_yoctonear(1))
        .gas(Gas::from_tgas(300))
        .transact()
        .await?
        .into_result()?;

    assert_eq!(
        ctx.owner_of("2001").await?,
        ctx.alice.id().to_string(),
        "the NFT should be back with its owner"
    );

    // The reward is ~0.046 NEAR for a 7-day stake in the first tier, orders of
    // magnitude more than the gas this costs, so the balance must have grown.
    let after = ctx.alice_balance().await?;
    assert!(
        after > before,
        "balance did not grow: before={before} after={after} reward={reward}"
    );

    let stake: serde_json::Value = ctx
        .staking
        .view("get_stake")
        .args_json(serde_json::json!({ "account_id": ctx.alice.id(), "token_id": "2001" }))
        .await?
        .json()?;
    assert_eq!(stake["claimed"], true, "the record should be marked claimed");

    let claimed: String = ctx
        .staking
        .view("get_claimed_stakes_count")
        .args_json(serde_json::json!({}))
        .await?
        .json()?;
    assert_eq!(claimed, "1");

    Ok(())
}
