//! The core solvency property: a reward is deducted from the pool when the
//! stake is created, so a pool that cannot cover it must refuse the stake
//! rather than lock an NFT it cannot pay for.

mod common;

use near_workspaces::types::NearToken;

#[tokio::test]
async fn empty_pool_returns_the_nft_instead_of_locking_it() -> anyhow::Result<()> {
    let ctx = common::setup().await?;
    ctx.mint_to_alice("3001").await?;

    // No deposit_rewards at all: a freshly initialised contract starts at zero.
    let pool: String = ctx
        .staking
        .view("get_reward_pool")
        .args_json(serde_json::json!({}))
        .await?
        .json()?;
    assert_eq!(pool, "0");

    // The transaction itself succeeds — nft_on_transfer returns true and the
    // collection sends the token back. Failing loudly here would be worse: a
    // panic inside nft_on_transfer leaves the NFT on the contract with no
    // record of it.
    ctx.stake("3001", 7).await?;

    assert_eq!(
        ctx.owner_of("3001").await?,
        ctx.alice.id().to_string(),
        "an unaffordable stake must return the NFT, not keep it"
    );

    let stakes: serde_json::Value = ctx
        .staking
        .view("get_stakes_for_account")
        .args_json(serde_json::json!({ "account_id": ctx.alice.id() }))
        .await?
        .json()?;
    assert_eq!(
        stakes.as_array().map(|a| a.len()),
        Some(0),
        "no stake should have been recorded: {stakes}"
    );

    let active: String = ctx
        .staking
        .view("get_active_stakes_count")
        .args_json(serde_json::json!({}))
        .await?
        .json()?;
    assert_eq!(active, "0");

    // With the pool funded, the same token stakes normally — proving the refusal
    // was about solvency and nothing else.
    ctx.deposit_rewards(NearToken::from_millinear(500)).await?;
    ctx.stake("3001", 7).await?;
    assert_eq!(ctx.owner_of("3001").await?, ctx.staking.id().to_string());

    Ok(())
}
