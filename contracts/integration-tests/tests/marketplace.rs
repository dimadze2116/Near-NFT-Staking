//! Marketplace behaviour, including the two findings from the security review
//! that are easiest to regress: duplicate cart entries (M-2) and the cart size
//! ceiling derived from measured gas (M-4).

mod common;

use near_workspaces::types::{Gas, NearToken};

async fn list_four(ctx: &common::Ctx) -> anyhow::Result<Vec<String>> {
    let ids: Vec<String> = (1..=4).map(|i| format!("41{i:02}")).collect();
    for id in &ids {
        ctx.mint_to_alice(id).await?;
        ctx.list(id, NearToken::from_millinear(100)).await?;
    }
    assert_eq!(ctx.active_listings().await?, 4);
    Ok(ids)
}

#[tokio::test]
async fn listing_moves_the_nft_into_escrow_and_cancel_returns_it() -> anyhow::Result<()> {
    let ctx = common::setup().await?;
    ctx.mint_to_alice("4001").await?;

    ctx.list("4001", NearToken::from_near(2)).await?;

    // Escrow is the point: while listed, the contract owns the token, not the
    // seller. That is what makes a sale atomic.
    assert_eq!(ctx.owner_of("4001").await?, ctx.market.id().to_string());
    let listing = ctx.listing("4001").await?;
    assert_eq!(listing["seller"], ctx.alice.id().as_str());
    assert_eq!(listing["price"], NearToken::from_near(2).as_yoctonear().to_string());

    ctx.alice
        .call(ctx.market.id(), "cancel_listing")
        .args_json(serde_json::json!({ "token_id": "4001" }))
        .deposit(NearToken::from_yoctonear(1))
        .gas(Gas::from_tgas(100))
        .transact()
        .await?
        .into_result()?;

    assert_eq!(ctx.owner_of("4001").await?, ctx.alice.id().to_string());
    assert_eq!(ctx.active_listings().await?, 0);
    assert!(ctx.listing("4001").await?.is_null());

    // And it can go back on sale afterwards — a cancelled listing must not
    // leave anything behind that blocks relisting.
    ctx.list("4001", NearToken::from_near(1)).await?;
    assert_eq!(ctx.active_listings().await?, 1);

    Ok(())
}

#[tokio::test]
async fn only_the_seller_can_cancel() -> anyhow::Result<()> {
    let ctx = common::setup().await?;
    ctx.mint_to_alice("4002").await?;
    ctx.list("4002", NearToken::from_near(1)).await?;

    let outcome = ctx
        .bob
        .call(ctx.market.id(), "cancel_listing")
        .args_json(serde_json::json!({ "token_id": "4002" }))
        .deposit(NearToken::from_yoctonear(1))
        .gas(Gas::from_tgas(100))
        .transact()
        .await?;

    assert!(outcome.is_failure(), "a stranger cancelled someone's listing");
    assert_eq!(ctx.owner_of("4002").await?, ctx.market.id().to_string());

    Ok(())
}

#[tokio::test]
async fn four_nfts_are_bought_in_one_transaction() -> anyhow::Result<()> {
    let ctx = common::setup().await?;
    let ids = list_four(&ctx).await?;

    // 4 x 0.1 NEAR, paid with a deliberate surplus so the refund path runs too.
    ctx.bob
        .call(ctx.market.id(), "buy_multiple")
        .args_json(serde_json::json!({ "token_ids": ids }))
        .deposit(NearToken::from_millinear(450))
        .gas(Gas::from_tgas(300))
        .transact()
        .await?
        .into_result()?;

    for id in &ids {
        assert_eq!(
            ctx.owner_of(id).await?,
            ctx.bob.id().to_string(),
            "token {id} did not reach the buyer"
        );
    }
    assert_eq!(ctx.active_listings().await?, 0);

    // 0.5% of 0.4 NEAR
    let fees: String = ctx
        .market
        .view("get_fees_collected")
        .args_json(serde_json::json!({}))
        .await?
        .json()?;
    assert_eq!(fees, NearToken::from_millinear(2).as_yoctonear().to_string());

    Ok(())
}

#[tokio::test]
async fn a_fifth_item_is_refused() -> anyhow::Result<()> {
    let ctx = common::setup().await?;
    let mut ids = list_four(&ctx).await?;
    ctx.mint_to_alice("4105").await?;
    ctx.list("4105", NearToken::from_millinear(100)).await?;
    ids.push("4105".to_string());

    // The ceiling is a gas budget, not a product choice: each item costs
    // GAS_NFT_TRANSFER + GAS_PURCHASE_CALLBACK = 45 TGas, and five of them no
    // longer fit under the 300 TGas transaction limit.
    let outcome = ctx
        .bob
        .call(ctx.market.id(), "buy_multiple")
        .args_json(serde_json::json!({ "token_ids": ids }))
        .deposit(NearToken::from_millinear(500))
        .gas(Gas::from_tgas(300))
        .transact()
        .await?;

    assert!(outcome.is_failure(), "five items were accepted");
    assert_eq!(
        ctx.active_listings().await?,
        5,
        "a refused purchase must leave every listing on sale"
    );

    Ok(())
}

#[tokio::test]
async fn duplicate_cart_entries_are_refused() -> anyhow::Result<()> {
    let ctx = common::setup().await?;
    ctx.mint_to_alice("4201").await?;
    ctx.list("4201", NearToken::from_millinear(100)).await?;

    // M-2: the same token twice removed one listing but scheduled two
    // transfers. The second failed, and its callback restored a listing whose
    // NFT the contract no longer held — unbuyable, uncancellable, unrelistable.
    let outcome = ctx
        .bob
        .call(ctx.market.id(), "buy_multiple")
        .args_json(serde_json::json!({ "token_ids": ["4201", "4201"] }))
        .deposit(NearToken::from_millinear(200))
        .gas(Gas::from_tgas(300))
        .transact()
        .await?;

    assert!(outcome.is_failure(), "duplicates were accepted");
    assert_eq!(ctx.active_listings().await?, 1);
    assert_eq!(ctx.owner_of("4201").await?, ctx.market.id().to_string());

    // The listing is still healthy: it can be bought normally.
    ctx.bob
        .call(ctx.market.id(), "buy_multiple")
        .args_json(serde_json::json!({ "token_ids": ["4201"] }))
        .deposit(NearToken::from_millinear(100))
        .gas(Gas::from_tgas(300))
        .transact()
        .await?
        .into_result()?;
    assert_eq!(ctx.owner_of("4201").await?, ctx.bob.id().to_string());

    Ok(())
}

#[tokio::test]
async fn underpaying_is_refused() -> anyhow::Result<()> {
    let ctx = common::setup().await?;
    ctx.mint_to_alice("4301").await?;
    ctx.list("4301", NearToken::from_near(1)).await?;

    let outcome = ctx
        .bob
        .call(ctx.market.id(), "buy_multiple")
        .args_json(serde_json::json!({ "token_ids": ["4301"] }))
        .deposit(NearToken::from_millinear(999))
        .gas(Gas::from_tgas(300))
        .transact()
        .await?;

    assert!(outcome.is_failure(), "a short payment was accepted");
    assert_eq!(ctx.owner_of("4301").await?, ctx.market.id().to_string());
    assert_eq!(ctx.active_listings().await?, 1);

    Ok(())
}
