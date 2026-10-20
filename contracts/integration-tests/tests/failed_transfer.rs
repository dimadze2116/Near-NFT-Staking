//! M-1: when the NFT transfer to a buyer fails, the buyer used to lose both the
//! NFT and the money. The listing went back on sale while the payment stayed on
//! the contract, reachable by nobody — `withdraw_fees` is bounded by
//! `fees_collected`, and this never reached it.

mod common;

use near_workspaces::types::{Gas, NearToken};

#[tokio::test]
async fn a_failed_transfer_refunds_the_buyer_in_full() -> anyhow::Result<()> {
    let ctx = common::setup().await?;
    ctx.mint_to_alice("8001").await?;
    ctx.list("8001", NearToken::from_near(5)).await?;

    let buyer_before = ctx.bob.view_account().await?.balance.as_yoctonear();

    // From here the collection refuses transfers, which is what a deleted
    // recipient account or a collection-side guard would look like.
    ctx.set_nft_transfers_failing(true).await?;

    // The purchase transaction itself succeeds — the failure happens inside the
    // transfer it schedules, and the callback is what has to clean up.
    ctx.bob
        .call(ctx.market.id(), "buy_multiple")
        .args_json(serde_json::json!({ "token_ids": ["8001"] }))
        .deposit(NearToken::from_near(5))
        .gas(Gas::from_tgas(300))
        .transact()
        .await?
        .into_result()?;

    // The NFT never moved.
    assert_eq!(ctx.owner_of("8001").await?, ctx.market.id().to_string());

    // The listing is back on sale rather than lost.
    let listing = ctx.listing("8001").await?;
    assert!(!listing.is_null(), "the listing was not restored");
    assert_eq!(listing["seller"], ctx.alice.id().as_str());
    assert_eq!(ctx.active_listings().await?, 1);

    // And the buyer got the whole price back, give or take gas.
    //
    // The threshold is deliberately coarse. Attaching 300 TGas costs the signer
    // on the order of 0.09 NEAR here, so a refunded buyer is out about that
    // much and an unrefunded one about 5.09 — the price was chosen to make the
    // two impossible to confuse.
    let buyer_after = ctx.bob.view_account().await?.balance.as_yoctonear();
    let lost = buyer_before.saturating_sub(buyer_after);
    assert!(
        lost < NearToken::from_near(1).as_yoctonear(),
        "the buyer is out {lost} yoctoNEAR, far more than gas, so the 5 NEAR \
         was not refunded: before={buyer_before} after={buyer_after}"
    );

    // No fee was booked on a sale that did not happen.
    let fees: String = ctx
        .market
        .view("get_fees_collected")
        .args_json(serde_json::json!({}))
        .await?
        .json()?;
    assert_eq!(fees, "0", "a fee was taken for a failed sale");

    // With transfers working again the listing sells normally, so nothing about
    // it was left in a broken state.
    ctx.set_nft_transfers_failing(false).await?;
    ctx.bob
        .call(ctx.market.id(), "buy_multiple")
        .args_json(serde_json::json!({ "token_ids": ["8001"] }))
        .deposit(NearToken::from_near(5))
        .gas(Gas::from_tgas(300))
        .transact()
        .await?
        .into_result()?;
    assert_eq!(ctx.owner_of("8001").await?, ctx.bob.id().to_string());

    Ok(())
}
