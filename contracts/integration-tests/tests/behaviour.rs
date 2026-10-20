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

    // The payout is measured on the contract's side, not the staker's. A staker
    // who attaches 300 TGas to unstake can easily burn more in gas than a
    // 7-day first-tier reward is worth, so "did Alice's balance grow" says
    // nothing about whether the reward was paid.
    let pool_before = ctx.reward_pool().await?;
    let contract_before = ctx.staking_balance().await?;

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

    // The contract's balance drops by the reward, give or take gas: it pays the
    // reward out and gets unused gas from its own promises refunded back, which
    // is worth a fraction of a thousandth of a NEAR. Hence a tolerance rather
    // than an exact figure — and a tolerance three orders of magnitude below
    // the reward, so it cannot hide a missing payout.
    let contract_after = ctx.staking_balance().await?;
    let moved = contract_before - contract_after;
    let tolerance = NearToken::from_millinear(1).as_yoctonear();
    assert!(
        moved.abs_diff(reward) < tolerance,
        "the contract's balance moved by {moved}, expected about {reward}"
    );

    // The reward was taken out of the pool when the stake was created, so
    // paying it out must not touch the pool a second time.
    let pool_after = ctx.reward_pool().await?;
    assert_eq!(
        pool_before, pool_after,
        "the pool changed during payout, so the reward was charged twice"
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
