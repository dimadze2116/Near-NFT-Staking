//! The F-3 fix: counters that replace two full scans in the write path.
//!
//! What matters here is not that the counters are fast but that they are
//! *correct in both states* — absent, where the contract scans exactly as it
//! always did, and present, where every write path keeps them in step.

mod common;

use near_workspaces::types::{Gas, NearToken};

async fn audit(ctx: &common::Ctx) -> anyhow::Result<serde_json::Value> {
    Ok(ctx
        .staking
        .view("audit_counters")
        .args_json(serde_json::json!({}))
        .await?
        .json()?)
}

async fn init_counters(ctx: &common::Ctx) -> anyhow::Result<()> {
    ctx.staking
        .call("init_counters")
        .deposit(NearToken::from_yoctonear(1))
        .gas(Gas::from_tgas(150))
        .transact()
        .await?
        .into_result()?;
    Ok(())
}

#[tokio::test]
async fn counters_stay_in_step_with_a_full_scan() -> anyhow::Result<()> {
    let ctx = common::setup().await?;
    ctx.deposit_rewards(NearToken::from_near(2)).await?;

    // Before initialisation the contract must behave as the previous build:
    // nothing authoritative, reads come from scanning.
    let a = audit(&ctx).await?;
    assert_eq!(a["ready"], false);

    for id in ["5001", "5002", "5003"] {
        ctx.mint_to_alice(id).await?;
        ctx.stake(id, 7).await?;
    }
    let active: String = ctx
        .staking
        .view("get_active_stakes_count")
        .args_json(serde_json::json!({}))
        .await?
        .json()?;
    assert_eq!(active, "3", "scanning must still report correctly");

    // Counters computed from existing state, including stakes made before they
    // existed — this is the step that would otherwise need a state migration.
    init_counters(&ctx).await?;
    let a = audit(&ctx).await?;
    assert_eq!(a["ready"], true);
    assert_eq!(a["active_counter"], a["active_real"], "audit: {a}");
    assert_eq!(a["claimed_counter"], a["claimed_real"], "audit: {a}");
    assert_eq!(a["active_counter"], "3");

    // A write with counters on.
    ctx.mint_to_alice("5004").await?;
    ctx.stake("5004", 14).await?;
    let a = audit(&ctx).await?;
    assert_eq!(a["active_counter"], "4");
    assert_eq!(a["active_counter"], a["active_real"], "after a stake: {a}");

    // And a claim, which moves a record from one counter to the other.
    ctx.backdate_stake("5001", 8 * 24 * 60 * 60).await?;
    ctx.alice
        .call(ctx.staking.id(), "unstake")
        .args_json(serde_json::json!({ "token_id": "5001" }))
        .deposit(NearToken::from_yoctonear(1))
        .gas(Gas::from_tgas(300))
        .transact()
        .await?
        .into_result()?;

    let a = audit(&ctx).await?;
    assert_eq!(a["active_counter"], a["active_real"], "after unstake: {a}");
    assert_eq!(a["claimed_counter"], a["claimed_real"], "after unstake: {a}");
    assert_eq!(a["active_counter"], "3");
    assert_eq!(a["claimed_counter"], "1");

    Ok(())
}

#[tokio::test]
async fn reset_returns_the_contract_to_scanning() -> anyhow::Result<()> {
    let ctx = common::setup().await?;
    ctx.deposit_rewards(NearToken::from_near(1)).await?;
    ctx.mint_to_alice("5101").await?;
    ctx.stake("5101", 7).await?;

    init_counters(&ctx).await?;
    assert_eq!(audit(&ctx).await?["ready"], true);

    // The way back, in one call.
    ctx.staking
        .call("reset_counters")
        .deposit(NearToken::from_yoctonear(1))
        .gas(Gas::from_tgas(150))
        .transact()
        .await?
        .into_result()?;

    let a = audit(&ctx).await?;
    assert_eq!(a["ready"], false);
    assert_eq!(a["active_real"], "1", "the stake itself is untouched: {a}");

    // Reads still work, by scanning.
    let active: String = ctx
        .staking
        .view("get_active_stakes_count")
        .args_json(serde_json::json!({}))
        .await?
        .json()?;
    assert_eq!(active, "1");

    // And staking still works, with counters off.
    ctx.mint_to_alice("5102").await?;
    ctx.stake("5102", 7).await?;
    let active: String = ctx
        .staking
        .view("get_active_stakes_count")
        .args_json(serde_json::json!({}))
        .await?
        .json()?;
    assert_eq!(active, "2");

    Ok(())
}

#[tokio::test]
async fn only_the_owner_can_touch_the_counters() -> anyhow::Result<()> {
    let ctx = common::setup().await?;

    let outcome = ctx
        .alice
        .call(ctx.staking.id(), "init_counters")
        .deposit(NearToken::from_yoctonear(1))
        .gas(Gas::from_tgas(150))
        .transact()
        .await?;
    assert!(outcome.is_failure(), "a stranger initialised the counters");

    // And the 1 yoctoNEAR requirement holds for the owner too: it is what
    // forces the call to be signed with a full-access key.
    let outcome = ctx
        .staking
        .call("init_counters")
        .gas(Gas::from_tgas(150))
        .transact()
        .await?;
    assert!(outcome.is_failure(), "init_counters ran without 1 yoctoNEAR");

    Ok(())
}
