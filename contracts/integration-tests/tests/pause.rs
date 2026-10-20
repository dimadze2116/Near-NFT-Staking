//! The pause blocks the way in and never the way out. A pause that could trap
//! other people's NFTs in the contract would not be a safety measure.

mod common;

use near_workspaces::types::{Gas, NearToken};

#[tokio::test]
async fn pause_refuses_new_stakes_but_never_blocks_unstake() -> anyhow::Result<()> {
    let ctx = common::setup().await?;
    ctx.deposit_rewards(NearToken::from_near(2)).await?;

    // An existing position, created before the pause.
    ctx.mint_to_alice("6001").await?;
    ctx.stake("6001", 7).await?;
    ctx.backdate_stake("6001", 8 * 24 * 60 * 60).await?;

    assert_eq!(
        ctx.staking
            .view("is_paused")
            .args_json(serde_json::json!({}))
            .await?
            .json::<bool>()?,
        false
    );

    ctx.staking
        .call("pause")
        .deposit(NearToken::from_yoctonear(1))
        .gas(Gas::from_tgas(50))
        .transact()
        .await?
        .into_result()?;
    assert_eq!(
        ctx.staking
            .view("is_paused")
            .args_json(serde_json::json!({}))
            .await?
            .json::<bool>()?,
        true
    );

    // Way in: closed. The NFT comes back rather than being held.
    ctx.mint_to_alice("6002").await?;
    ctx.stake("6002", 7).await?;
    assert_eq!(
        ctx.owner_of("6002").await?,
        ctx.alice.id().to_string(),
        "a paused contract must return the NFT, not keep it"
    );
    let stakes: serde_json::Value = ctx
        .staking
        .view("get_stakes_for_account")
        .args_json(serde_json::json!({ "account_id": ctx.alice.id() }))
        .await?
        .json()?;
    assert_eq!(
        stakes.as_array().map(|a| a.len()),
        Some(1),
        "no second stake should exist: {stakes}"
    );

    // Way out: open, exactly as when unpaused.
    ctx.alice
        .call(ctx.staking.id(), "unstake")
        .args_json(serde_json::json!({ "token_id": "6001" }))
        .deposit(NearToken::from_yoctonear(1))
        .gas(Gas::from_tgas(300))
        .transact()
        .await?
        .into_result()?;
    assert_eq!(ctx.owner_of("6001").await?, ctx.alice.id().to_string());

    // Unpausing restores staking.
    ctx.staking
        .call("unpause")
        .deposit(NearToken::from_yoctonear(1))
        .gas(Gas::from_tgas(50))
        .transact()
        .await?
        .into_result()?;
    ctx.stake("6002", 7).await?;
    assert_eq!(ctx.owner_of("6002").await?, ctx.staking.id().to_string());

    Ok(())
}

#[tokio::test]
async fn only_the_owner_can_pause() -> anyhow::Result<()> {
    let ctx = common::setup().await?;

    let outcome = ctx
        .alice
        .call(ctx.staking.id(), "pause")
        .deposit(NearToken::from_yoctonear(1))
        .gas(Gas::from_tgas(50))
        .transact()
        .await?;
    assert!(outcome.is_failure(), "a stranger paused the contract");
    assert_eq!(
        ctx.staking
            .view("is_paused")
            .args_json(serde_json::json!({}))
            .await?
            .json::<bool>()?,
        false
    );

    Ok(())
}
