//! The test that makes the deployment safe to do at all.
//!
//! It takes the bytecode that was running on mainnet, puts state on it, then
//! runs the same sequence against the new build with its counters left
//! uninitialised — and requires every answer to match. That is the exact state
//! the contract is in the moment a redeploy lands: new code, no counters yet.
//! If the two agree there, deploying cannot change what live positions see.
//!
//! Comparison is limited to answers that do not embed account or token names,
//! since the two contracts necessarily hold different tokens of the same
//! collection. What remains is precisely the surface the counters touch:
//! aggregates, plus the reward arithmetic.

mod common;

use near_workspaces::types::{Gas, NearToken};
use near_workspaces::Contract;

async fn view_str(contract: &Contract, method: &str) -> anyhow::Result<String> {
    Ok(contract
        .view(method)
        .args_json(serde_json::json!({}))
        .await?
        .json()?)
}

/// Every answer that should be identical between the two builds.
async fn snapshot(contract: &Contract) -> anyhow::Result<Vec<(String, String)>> {
    let mut out = Vec::new();

    for method in [
        "get_reward_pool",
        "get_active_stakes_count",
        "get_matured_stakes_count",
        "get_total_staked",
        "get_claimed_stakes_count",
        "get_unique_claimers_count",
    ] {
        out.push((method.to_string(), view_str(contract, method).await?));
    }

    // Tier boundaries, which decide what a staker is actually paid.
    for position in [1u32, 10, 11, 20, 21, 30, 31, 40, 41, 50, 51, 300] {
        let apr: u64 = contract
            .view("get_apr_bps")
            .args_json(serde_json::json!({ "position": position }))
            .await?
            .json()?;
        out.push((format!("apr_bps[{position}]"), apr.to_string()));
    }

    // Reward preview across tier crossings and all three periods.
    for (current, add) in [(0u32, 1u32), (8, 5), (19, 3), (49, 4)] {
        for period in [7u8, 14, 30] {
            let reward: String = contract
                .view("preview_reward")
                .args_json(serde_json::json!({
                    "period_days": period,
                    "current_active_count": current,
                    "add_count": add,
                }))
                .await?
                .json()?;
            out.push((format!("preview[{current}+{add},{period}d]"), reward));
        }
    }

    // Stats, minus owner_id: the two contracts sit at different accounts.
    let stats: serde_json::Value = contract
        .view("get_contract_stats")
        .args_json(serde_json::json!({}))
        .await?
        .json()?;
    for field in [
        "reward_pool",
        "active_stakes",
        "matured_stakes",
        "total_staked",
        "unique_stakers",
    ] {
        out.push((
            format!("stats.{field}"),
            stats[field].as_str().unwrap_or("<missing>").to_string(),
        ));
    }

    Ok(out)
}

fn compare(stage: &str, old: &[(String, String)], new: &[(String, String)]) {
    assert_eq!(
        old.len(),
        new.len(),
        "{stage}: the two builds answered a different number of calls"
    );
    let mut differences = Vec::new();
    for ((name_old, value_old), (name_new, value_new)) in old.iter().zip(new) {
        assert_eq!(name_old, name_new, "{stage}: snapshots out of order");
        if value_old != value_new {
            differences.push(format!("  {name_old}: mainnet={value_old} new={value_new}"));
        }
    }
    assert!(
        differences.is_empty(),
        "{stage}: the new build answers differently with counters uninitialised:\n{}",
        differences.join("\n")
    );
}

#[tokio::test]
async fn new_build_without_counters_matches_the_mainnet_bytecode() -> anyhow::Result<()> {
    let ctx = common::setup().await?;
    let legacy = ctx.deploy_legacy_staking().await?;

    // Fresh, before anything happens.
    compare(
        "freshly initialised",
        &snapshot(&legacy).await?,
        &snapshot(&ctx.staking).await?,
    );

    for (contract, tokens) in [
        (&legacy, ["7001", "7002", "7003"]),
        (&ctx.staking, ["7101", "7102", "7103"]),
    ] {
        ctx.deposit_rewards_on(contract, NearToken::from_near(3)).await?;
        for (i, token) in tokens.iter().enumerate() {
            ctx.mint_to_alice(token).await?;
            let period = [7u8, 14, 30][i];
            ctx.stake_on(contract, token, period).await?;
        }
    }

    compare(
        "three stakes each",
        &snapshot(&legacy).await?,
        &snapshot(&ctx.staking).await?,
    );

    // Mature one on each side and claim it, so the claimed counters move too.
    for (contract, token) in [(&legacy, "7001"), (&ctx.staking, "7101")] {
        ctx.backdate_stake_on(contract, token, 8 * 24 * 60 * 60).await?;
        ctx.alice
            .call(contract.id(), "unstake")
            .args_json(serde_json::json!({ "token_id": token }))
            .deposit(NearToken::from_yoctonear(1))
            .gas(Gas::from_tgas(300))
            .transact()
            .await?
            .into_result()?;
    }

    compare(
        "after one claim each",
        &snapshot(&legacy).await?,
        &snapshot(&ctx.staking).await?,
    );

    // Only now are the counters switched on. From here the two are expected to
    // diverge in cost but not in answers, so the comparison is repeated.
    ctx.staking
        .call("init_counters")
        .deposit(NearToken::from_yoctonear(1))
        .gas(Gas::from_tgas(150))
        .transact()
        .await?
        .into_result()?;

    compare(
        "counters initialised",
        &snapshot(&legacy).await?,
        &snapshot(&ctx.staking).await?,
    );

    Ok(())
}
