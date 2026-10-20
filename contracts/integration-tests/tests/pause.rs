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

#[tokio::test]
async fn marketplace_pause_blocks_listing_and_buying_but_never_cancel() -> anyhow::Result<()> {
    let ctx = common::setup().await?;
    ctx.mint_to_alice("6101").await?;
    ctx.mint_to_alice("6102").await?;

    // One listing exists before the pause.
    ctx.list("6101", NearToken::from_near(1)).await?;

    ctx.market
        .call("pause")
        .deposit(NearToken::from_yoctonear(1))
        .gas(Gas::from_tgas(50))
        .transact()
        .await?
        .into_result()?;
    assert_eq!(
        ctx.market
            .view("is_paused")
            .args_json(serde_json::json!({}))
            .await?
            .json::<bool>()?,
        true
    );

    // New listings: refused, and the NFT comes back rather than sitting in a
    // contract that has no listing for it.
    ctx.list("6102", NearToken::from_near(1)).await?;
    assert_eq!(
        ctx.owner_of("6102").await?,
        ctx.alice.id().to_string(),
        "a paused marketplace must return the NFT"
    );
    assert_eq!(ctx.active_listings().await?, 1);

    // Buying: refused. Here a panic is the right answer — nothing was charged.
    let outcome = ctx
        .bob
        .call(ctx.market.id(), "buy_multiple")
        .args_json(serde_json::json!({ "token_ids": ["6101"] }))
        .deposit(NearToken::from_near(1))
        .gas(Gas::from_tgas(300))
        .transact()
        .await?;
    assert!(outcome.is_failure(), "a purchase went through while paused");
    assert_eq!(ctx.owner_of("6101").await?, ctx.market.id().to_string());

    // Cancelling: always open. This is the property that makes the pause a
    // safety measure instead of a way to lock up other people's NFTs.
    ctx.alice
        .call(ctx.market.id(), "cancel_listing")
        .args_json(serde_json::json!({ "token_id": "6101" }))
        .deposit(NearToken::from_yoctonear(1))
        .gas(Gas::from_tgas(100))
        .transact()
        .await?
        .into_result()?;
    assert_eq!(ctx.owner_of("6101").await?, ctx.alice.id().to_string());
    assert_eq!(ctx.active_listings().await?, 0);

    // Unpausing restores both paths.
    ctx.market
        .call("unpause")
        .deposit(NearToken::from_yoctonear(1))
        .gas(Gas::from_tgas(50))
        .transact()
        .await?
        .into_result()?;
    ctx.list("6101", NearToken::from_near(1)).await?;
    ctx.bob
        .call(ctx.market.id(), "buy_multiple")
        .args_json(serde_json::json!({ "token_ids": ["6101"] }))
        .deposit(NearToken::from_near(1))
        .gas(Gas::from_tgas(300))
        .transact()
        .await?
        .into_result()?;
    assert_eq!(ctx.owner_of("6101").await?, ctx.bob.id().to_string());

    Ok(())
}

#[tokio::test]
async fn only_the_owner_can_pause_the_marketplace() -> anyhow::Result<()> {
    let ctx = common::setup().await?;

    let outcome = ctx
        .alice
        .call(ctx.market.id(), "pause")
        .deposit(NearToken::from_yoctonear(1))
        .gas(Gas::from_tgas(50))
        .transact()
        .await?;
    assert!(outcome.is_failure(), "a stranger paused the marketplace");
    assert_eq!(
        ctx.market
            .view("is_paused")
            .args_json(serde_json::json!({}))
            .await?
            .json::<bool>()?,
        false
    );

    Ok(())
}
