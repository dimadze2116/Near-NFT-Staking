use near_sdk::store::{IterableMap, IterableSet, LookupMap};
use near_sdk::json_types::U128;
use near_sdk::{
    env, near, require, AccountId, BorshStorageKey,
    Gas, NearToken, PanicOnDefault, Promise, PromiseOrValue, ext_contract,
};

const NFT_CONTRACT_ID: &str = "nearlegion.nfts.tg";

const PERIOD_7_DAYS: u64  = 7  * 24 * 60 * 60;
const PERIOD_14_DAYS: u64 = 14 * 24 * 60 * 60;
const PERIOD_30_DAYS: u64 = 30 * 24 * 60 * 60;

/// APR tiers (in basis points, 1% = 100 bps) by the POSITION of an NFT for a staker
/// (their 1st, 2nd, ... Nth staked NFT - not the number of NFTs in this transaction).
/// Chosen so that a budget of 100 NEAR/month cannot be exceeded even in the worst
/// case: a single staker occupies the entire MAX_ACTIVE_STAKES ceiling of NFTs for
/// the longest period (30 days). See change-apr-and-batch.md.
///
/// IMPORTANT (bug fix): the formula used to multiply the final reward by the NFT's
/// POSITION (nft_count), so a staker's 50th NFT was computed as if 50 had been
/// staked at once. The position now only selects the tier - the reward for EACH
/// individual NFT comes from TIER_APR_BPS[tier], with no multiplication by the
/// position itself.
const TIER_APR_BPS: [u64; 6] = [
    2420, // position 1-10
    2904, // position 11-20
    3319, // position 21-30
    3733, // position 31-40
    4148, // position 41-50
    4205, // position 51+ (the cap; it does not grow further)
];

/// Hard ceiling on simultaneously active (unclaimed) NFTs across the whole contract.
/// This is what makes the budget guarantee mathematical - see change-apr-and-batch.md.
const MAX_ACTIVE_STAKES: u64 = 300;

/// Grace period after which a stake counts as abandoned and the contract owner may
/// force the NFT back to its owner (admin_return_nft). 90 days beyond the unlock
/// time: comfortably longer than the longest period (30 days), so the method can
/// never be applied to a live position.
const ABANDONED_GRACE_SEC: u64 = 90 * 24 * 60 * 60;

const NFT_VALUE_YOCTO: u128 = 10 * 1_000_000_000_000_000_000_000_000u128;

const GAS_NFT_TRANSFER: Gas  = Gas::from_tgas(50);
const GAS_CALLBACK:     Gas  = Gas::from_tgas(10);
const GAS_REWARD_PAY:   Gas  = Gas::from_tgas(10);

// ─── External contract interfaces ─────────────────────────────────────────────

/// Interface of the NFT contract (NEP-171)
#[ext_contract(ext_nft)]
trait NftContract {
    fn nft_transfer(
        &mut self,
        receiver_id: AccountId,
        token_id: String,
        memo: Option<String>,
    );
}

/// Callback interface of this contract
#[ext_contract(ext_self)]
trait SelfCallback {
    fn on_unstake_complete(
        &mut self,
        caller:   AccountId,
        token_id: String,
        stake_id: String,
        reward:   U128,
    );
    fn on_withdraw_complete(
        &mut self,
        amount: U128,
    );
    fn on_reward_paid(
        &mut self,
        caller:   AccountId,
        stake_id: String,
        reward:   U128,
    );
    fn on_admin_return_complete(
        &mut self,
        stake_id: String,
        stake:    StakeInfo,
    );
}

// ─── Storage Keys ─────────────────────────────────────────────────────────────

#[derive(BorshStorageKey)]
#[near]
enum StorageKey {
    Stakes,
    StakesByAccount,
    StakesByAccountInner { account_hash: Vec<u8> },
}

// ─── Counters and the pause flag, kept outside the contract struct ───────────
//
// F-3: creating a stake used to scan the whole stake map twice — once for the
// global MAX_ACTIVE_STAKES ceiling and once for the staker's own position. That
// is in the write path, so the cost of staking grew with the number of records
// ever created.
//
// The counters that replace those scans live under their own storage keys
// rather than as fields of the contract struct, and that is the whole point:
// adding a field changes the borsh layout of the state, which means the new
// code cannot read the old bytes and a migration has to run before anything
// works again. On a contract holding other people's NFTs that is the single
// most dangerous operation available. Separate keys need no migration, and the
// read path falls back to scanning while they are absent, so a deployment
// behaves exactly as the previous build until someone calls init_counters —
// and reset_counters puts it back.
//
// Keys are text on purpose. The collections occupy the prefixes produced by
// StorageKey's discriminants (0x00, 0x01, 0x02) and the struct itself sits
// under "STATE"; a key beginning with 'c' or 'f' cannot collide with any of
// them on the first byte.
const K_READY:   &[u8] = b"ctr:ready";    // present ⇒ counters are authoritative
const K_ACTIVE:  &[u8] = b"ctr:active";   // u64: active stakes, contract-wide
const K_CLAIMED: &[u8] = b"ctr:claimed";  // u64: claimed records, contract-wide
const K_PAUSED:  &[u8] = b"flag:paused";  // present ⇒ new stakes are refused

fn ctr_key_for(account: &AccountId) -> Vec<u8> {
    [b"ctr:acc:".as_slice(), account.as_bytes()].concat()
}

/// One flag for the whole set. Without it, a missing per-account key would be
/// ambiguous: zero active stakes, or counters never computed?
fn counters_ready() -> bool {
    env::storage_has_key(K_READY)
}

fn read_counter(key: &[u8]) -> u64 {
    env::storage_read(key)
        .and_then(|bytes| <[u8; 8]>::try_from(bytes.as_slice()).ok())
        .map(u64::from_le_bytes)
        .unwrap_or(0)
}

fn write_counter(key: &[u8], value: u64) {
    env::storage_write(key, &value.to_le_bytes());
}

fn bump_counter(key: &[u8], delta: i64) {
    let current = read_counter(key);
    let next = if delta >= 0 {
        current.saturating_add(delta as u64)
    } else {
        current.saturating_sub(delta.unsigned_abs())
    };
    write_counter(key, next);
}

/// Result of comparing the counters against a full scan. Exists because a
/// counter that drifts does not announce itself: the contract keeps working
/// until active_stakes reaches MAX_ACTIVE_STAKES with room actually to spare,
/// and then refuses stakes for no visible reason.
#[near(serializers = [json])]
pub struct CounterAudit {
    pub ready:           bool,
    pub active_counter:  U128,
    pub active_real:     U128,
    pub claimed_counter: U128,
    pub claimed_real:    U128,
}

// ─── Data Structures ──────────────────────────────────────────────────────────

#[near(serializers = [borsh, json])]
#[derive(Clone)]
pub struct StakeInfo {
    pub owner:       AccountId,
    pub token_id:    String,
    pub staked_at:   u64,
    pub period_days: u8,
    pub reward_near: U128,
    pub claimed:     bool,
}

#[near(serializers = [json])]
pub struct StakeView {
    pub stake_id:    String,
    pub token_id:    String,
    pub staked_at:   u64,
    pub period_days: u8,
    pub unlocks_at:  u64,
    pub reward_near: U128,
    pub is_mature:   bool,
    pub claimed:     bool,
}

/// A separate view struct for the global list of claimed stakes (admin panel):
/// unlike StakeView it carries owner, because the selection is not made per account
/// but across the whole contract at once.
/// owner: String (not AccountId) - this choice is kept from the previous version; it
/// is tested and works, and there is no reason to change it without a clear need.
#[near(serializers = [json])]
pub struct ClaimedStakeView {
    pub stake_id:    String,
    pub owner:       String,
    pub token_id:    String,
    pub staked_at:   u64,
    pub period_days: u8,
    pub unlocks_at:  u64,
    pub reward_near: U128,
}

/// An entry in the leaderboard - ranked by number of active (unclaimed) NFTs.
#[near(serializers = [json])]
pub struct LeaderboardEntry {
    pub owner:        String,
    pub nft_count:     u32,
    pub total_reward: U128,
}

/// Position of a specific account in the overall ranking (for the "your rank" card).
#[near(serializers = [json])]
pub struct MyRankEntry {
    pub position:     u32,
    pub owner:        String,
    pub nft_count:    u32,
    pub total_reward: U128,
}

#[near(serializers = [json])]
pub struct ContractStats {
    pub owner_id:       AccountId,
    pub reward_pool:    U128,
    pub active_stakes:  U128,
    pub matured_stakes: U128,
    pub total_staked:   U128,
    pub unique_stakers: U128,
}

// ─── Contract ─────────────────────────────────────────────────────────────────

#[near(contract_state)]
#[derive(PanicOnDefault)]
pub struct NftStakingContract {
    pub owner_id:          AccountId,
    pub stakes:            IterableMap<String, StakeInfo>,
    pub stakes_by_account: LookupMap<AccountId, IterableSet<String>>,
    pub reward_pool:       u128,
}

#[near]
impl NftStakingContract {

    // ── Init ───────────────────────────────────────────────────────────────

    #[init]
    pub fn new(owner_id: AccountId) -> Self {
        require!(!env::state_exists(), "Already initialized");
        Self {
            owner_id,
            stakes:            IterableMap::new(StorageKey::Stakes),
            stakes_by_account: LookupMap::new(StorageKey::StakesByAccount),
            reward_pool:       0,
        }
    }

    // ── Owner ──────────────────────────────────────────────────────────────

    #[payable]
    pub fn deposit_rewards(&mut self) {
        self.assert_owner();
        let amount = env::attached_deposit().as_yoctonear();
        require!(amount > 0, "Must attach NEAR to deposit");
        self.reward_pool += amount;
        env::log_str(&format!(
            "DEPOSIT | amount={} yN | pool_total={} yN",
            amount, self.reward_pool
        ));
    }

    /// Emergency return of an NFT from an abandoned stake.
    ///
    /// Two deliberate safeguards against abuse of the contract owner's rights:
    ///
    /// 1. The recipient is NOT a parameter. The NFT always goes to the `stake.owner`
    ///    recorded in state. The contract owner can trigger a return but cannot
    ///    choose to whom. That removes the theft vector present in the usual form
    ///    `admin_return_asset(id, to)`.
    /// 2. The method only works on stakes where more than ABANDONED_GRACE_SEC has
    ///    passed since unlock. It is unavailable for active and recently matured
    ///    positions - someone else's working stake cannot be taken.
    ///
    /// The reserved reward goes back to the pool (this closes F-6: the reserve of an
    /// abandoned stake used to hang outside the pool forever).
    #[payable]
    pub fn admin_return_nft(&mut self, stake_id: String) -> Promise {
        self.assert_owner_one_yocto();

        let stake = self
            .stakes
            .get(&stake_id)
            .unwrap_or_else(|| env::panic_str("admin_return_nft: stake not found"))
            .clone();

        require!(!stake.claimed, "admin_return_nft: stake already claimed");

        let now_sec   = env::block_timestamp() / 1_000_000_000;
        let unlock_at = stake.staked_at + Self::period_sec_lenient(stake.period_days);
        require!(
            now_sec >= unlock_at + ABANDONED_GRACE_SEC,
            "admin_return_nft: stake is not abandoned yet — grace period has not passed"
        );

        // Mark it BEFORE the promise - otherwise a repeat call in the window before the
        // callback would pass the checks again and send a second nft_transfer.
        let mut updated = stake.clone();
        updated.claimed = true;
        self.stakes.insert(stake_id.clone(), updated);
        self.count_stake_claimed(&stake.owner);

        // The reserve returns to the pool: the owner never came for the reward.
        self.reward_pool += stake.reward_near.0;

        let nft_contract: AccountId = NFT_CONTRACT_ID.parse().unwrap();

        env::log_str(&format!(
            "ADMIN_RETURN_INIT | nft={} | owner={} | reward_returned_to_pool={} yN",
            stake.token_id, stake.owner, stake.reward_near.0
        ));

        ext_nft::ext(nft_contract)
            .with_attached_deposit(NearToken::from_yoctonear(1))
            .with_static_gas(GAS_NFT_TRANSFER)
            .nft_transfer(
                stake.owner.clone(),
                stake.token_id.clone(),
                Some("NFT returned by contract owner — abandoned stake".to_string()),
            )
            .then(
                ext_self::ext(env::current_account_id())
                    .with_static_gas(GAS_CALLBACK)
                    .on_admin_return_complete(stake_id, stake)
            )
    }

    /// Withdrawal with a callback - if the Promise fails, reward_pool is restored
    #[payable]
    pub fn withdraw_rewards(&mut self, amount: U128) -> Promise {
        self.assert_owner_one_yocto();
        let yocto = amount.0;
        require!(yocto > 0, "Amount must be greater than zero");
        require!(
            self.reward_pool >= yocto,
            "Requested amount exceeds reward pool balance"
        );
        // FIX 3: deduct AFTER the callback confirms
        // Reserve it for now - the deduction happens in on_withdraw_complete
        env::log_str(&format!(
            "WITHDRAW_INIT | amount={} yN | pool_before={} yN",
            yocto, self.reward_pool
        ));
        self.reward_pool -= yocto; // deduct right away (guards against double-spend)

        Promise::new(self.owner_id.clone())
            .transfer(NearToken::from_yoctonear(yocto))
            .then(
                ext_self::ext(env::current_account_id())
                    .with_static_gas(GAS_CALLBACK)
                    .on_withdraw_complete(amount)
            )
    }

    /// F-1 (was clear_all_stakes): the previous method wiped ALL stakes at once and
    /// zeroed reward_pool. Since the NFTs physically sit on the contract balance and the
    /// only way back is unstake, which requires the record to exist, a single call locked
    /// every user's staked NFT away forever. The contract has no emergency withdrawal.
    /// The method was deleted and replaced by the safe equivalent below.
    ///
    /// Deletes ONLY claimed records - their NFTs have already gone back to their owners,
    /// so there is nothing to lose. Active stakes and reward_pool are left untouched.
    /// F-3: claimed records used to accumulate in self.stakes without limit and made
    /// every new stake more expensive (a full scan of the map in nft_on_transfer). This
    /// method is a way to prune them without a state migration.
    /// limit bounds the work per call - the method is meant to be run repeatedly.
    #[payable]
    pub fn prune_claimed_stakes(&mut self, limit: u32) -> u32 {
        self.assert_owner_one_yocto();
        require!(limit > 0 && limit <= 100, "prune: limit must be between 1 and 100");

        // Collect first, delete after - the map must not be mutated while iterating.
        let to_remove: Vec<(String, AccountId)> = self
            .stakes
            .iter()
            .filter(|(_, s)| s.claimed)
            .take(limit as usize)
            .map(|(id, s)| (id.clone(), s.owner.clone()))
            .collect();

        let count = to_remove.len() as u32;

        for (stake_id, owner) in to_remove {
            self.stakes.remove(&stake_id);
            // Clean the account index too, otherwise the set grows forever and makes
            // active_stake_count more expensive for that user.
            if let Some(ids) = self.stakes_by_account.get_mut(&owner) {
                ids.remove(&stake_id);
            }
        }

        if counters_ready() {
            bump_counter(K_CLAIMED, -(count as i64));
        }

        env::log_str(&format!("PRUNE | removed {} claimed stakes", count));
        count
    }

    /// Computes the counters from scratch and marks them authoritative.
    /// Idempotent: calling it again recomputes everything, so activity between
    /// deployment and this call cannot throw the numbers off.
    #[payable]
    pub fn init_counters(&mut self) -> CounterAudit {
        self.assert_owner_one_yocto();

        let mut active  = 0u64;
        let mut claimed = 0u64;
        let mut per_account: std::collections::HashMap<AccountId, u64> =
            std::collections::HashMap::new();

        for (_, s) in self.stakes.iter() {
            if s.claimed {
                claimed += 1;
            } else {
                active += 1;
                *per_account.entry(s.owner.clone()).or_insert(0) += 1;
            }
        }

        write_counter(K_ACTIVE, active);
        write_counter(K_CLAIMED, claimed);
        for (account, n) in per_account {
            write_counter(&ctr_key_for(&account), n);
        }
        env::storage_write(K_READY, &[1u8]);

        env::log_str(&format!(
            "COUNTERS_INIT | active={} | claimed={}",
            active, claimed
        ));

        self.audit_counters()
    }

    /// Removes every counter key. Afterwards the contract reads by scanning,
    /// exactly as it did before the counters existed — this is the way back if
    /// anything about them looks wrong.
    #[payable]
    pub fn reset_counters(&mut self) {
        self.assert_owner_one_yocto();

        let owners: std::collections::HashSet<AccountId> =
            self.stakes.iter().map(|(_, s)| s.owner.clone()).collect();
        for account in owners {
            env::storage_remove(&ctr_key_for(&account));
        }
        env::storage_remove(K_ACTIVE);
        env::storage_remove(K_CLAIMED);
        env::storage_remove(K_READY);

        env::log_str("COUNTERS_RESET | reads fall back to scanning");
    }

    #[payable]
    pub fn pause(&mut self) {
        self.assert_owner_one_yocto();
        env::storage_write(K_PAUSED, &[1u8]);
        env::log_str("PAUSED | new stakes refused, unstake unaffected");
    }

    #[payable]
    pub fn unpause(&mut self) {
        self.assert_owner_one_yocto();
        env::storage_remove(K_PAUSED);
        env::log_str("UNPAUSED");
    }

    // ── NFT Receiver ───────────────────────────────────────────────────────

    pub fn nft_on_transfer(
        &mut self,
        #[allow(unused_variables)]
        sender_id: AccountId,
        previous_owner_id: AccountId,
        token_id: String,
        msg: String,
    ) -> PromiseOrValue<bool> {
        // F-7: the stake owner must be the PREVIOUS OWNER of the NFT, not whoever called
        // nft_transfer_call. Under NEP-171 these are different accounts: with approvals
        // (NEP-178) an approved account can move someone else's NFT, and it used to become
        // the stake owner - meaning on unstake it could take that NFT along with the
        // reward. In nft-marketplace this choice was made correctly from the start.
        let owner_id = previous_owner_id;
        let nft_contract: AccountId = NFT_CONTRACT_ID
            .parse()
            .expect("Invalid NFT contract ID in code");
        require!(
            env::predecessor_account_id() == nft_contract,
            "nft_on_transfer: caller is not the authorized NFT contract"
        );

        // Paused means no new positions. Returning true sends the NFT back;
        // panicking here would leave it on the contract with no record of it.
        if env::storage_has_key(K_PAUSED) {
            env::log_str(&format!(
                "STAKE_REJECTED | reason=paused | nft={} | owner={}",
                token_id, owner_id
            ));
            return PromiseOrValue::Value(true);
        }

        let period_days: u8 = match msg.trim().parse::<u8>() {
            Ok(d) if d == 7 || d == 14 || d == 30 => d,
            Ok(d) => {
                env::log_str(&format!(
                    "STAKE_REJECTED | reason=invalid_period | period={} | nft={} | owner={}",
                    d, token_id, owner_id
                ));
                return PromiseOrValue::Value(true);
            }
            Err(_) => {
                env::log_str(&format!(
                    "STAKE_REJECTED | reason=period_not_a_number | msg={} | nft={} | owner={}",
                    msg, token_id, owner_id
                ));
                return PromiseOrValue::Value(true);
            }
        };

        let stake_id = format!("{}:{}", owner_id, token_id);
        if let Some(existing) = self.stakes.get(&stake_id) {
            if !existing.claimed {
                env::log_str(&format!(
                    "STAKE_REJECTED | reason=already_staked | nft={} | owner={}",
                    token_id, owner_id
                ));
                return PromiseOrValue::Value(true);
            }
        }

        // Ceiling on active NFTs across the whole contract - this is what guarantees the
        // pool budget mathematically, however the periods and stakers happen to spread.
        let total_active = self.active_total();
        if total_active >= MAX_ACTIVE_STAKES {
            env::log_str(&format!(
                "STAKE_REJECTED | reason=pool_full | active={} | max={} | nft={} | owner={}",
                total_active, MAX_ACTIVE_STAKES, token_id, owner_id
            ));
            return PromiseOrValue::Value(true);
        }

        let active_count = self.active_for(&owner_id);
        let reward = self.calc_reward(period_days, active_count + 1);

        if self.reward_pool < reward {
            env::log_str(&format!(
                "STAKE_REJECTED | reason=insufficient_pool | pool={} | required={} | nft={} | owner={}",
                self.reward_pool, reward, token_id, owner_id
            ));
            return PromiseOrValue::Value(true);
        }

        self.reward_pool -= reward;

        let now_sec = env::block_timestamp() / 1_000_000_000;

        let stake = StakeInfo {
            owner:       owner_id.clone(),
            token_id:    token_id.clone(),
            staked_at:   now_sec,
            period_days,
            reward_near: U128(reward),
            claimed:     false,
        };

        self.stakes.insert(stake_id.clone(), stake);

        if self.stakes_by_account.get(&owner_id).is_none() {
            self.stakes_by_account.insert(
                owner_id.clone(),
                IterableSet::new(StorageKey::StakesByAccountInner {
                    account_hash: env::sha256(owner_id.as_bytes()),
                }),
            );
        }
        self.stakes_by_account
            .get_mut(&owner_id)
            .unwrap()
            .insert(stake_id);

        self.count_stake_created(&owner_id);

        env::log_str(&format!(
            "STAKE_CREATED | nft={} | owner={} | period_days={} | reward={} yN | unlocks_at={}",
            token_id, owner_id, period_days, reward,
            now_sec + Self::period_sec(period_days)
        ));

        PromiseOrValue::Value(false)
    }

    // ── Unstake with a callback ────────────────────────────────────────────

    /// FIX 1: unstake now goes through the on_unstake_complete callback.
    /// If nft_transfer fails the stake is restored - the user loses neither NFT nor reward
    #[payable]
    pub fn unstake(&mut self, token_id: String) -> Promise {
        require!(
            env::attached_deposit().as_yoctonear() == 1,
            "unstake: attach exactly 1 yoctoNEAR"
        );

        let caller   = env::predecessor_account_id();
        let stake_id = format!("{}:{}", caller, token_id);

        require!(
            self.stakes.contains_key(&stake_id),
            "unstake: stake not found — check token_id and account"
        );

        let stake = self.stakes.get(&stake_id).unwrap().clone();

        require!(stake.owner == caller, "unstake: caller is not the stake owner");
        require!(!stake.claimed, "unstake: this stake has already been claimed");

        let now_sec    = env::block_timestamp() / 1_000_000_000;
        let period_sec = Self::period_sec_lenient(stake.period_days);
        let unlock_at  = stake.staked_at + period_sec;

        require!(
            now_sec >= unlock_at,
            &format!(
                "unstake: staking period not complete — unlocks at {} (now {}), {} seconds remaining",
                unlock_at, now_sec, unlock_at - now_sec
            )
        );

        // Mark as claimed BEFORE the promise (reentrancy guard)
        let mut updated = stake.clone();
        updated.claimed = true;
        self.stakes.insert(stake_id.clone(), updated);
        self.count_stake_claimed(&caller);

        let reward       = stake.reward_near;
        let nft_contract: AccountId = NFT_CONTRACT_ID.parse().unwrap();

        env::log_str(&format!(
            "UNSTAKE_INIT | nft={} | owner={} | reward={} yN",
            token_id, caller, reward.0
        ));

        // nft_transfer → callback on_unstake_complete
        ext_nft::ext(nft_contract)
            .with_attached_deposit(NearToken::from_yoctonear(1))
            .with_static_gas(GAS_NFT_TRANSFER)
            .nft_transfer(
                caller.clone(),
                token_id.clone(),
                Some("NFT unstaked — staking reward claimed".to_string()),
            )
            .then(
                ext_self::ext(env::current_account_id())
                    // 30 TGas instead of the previous 20: this callback now creates
                    // another promise (the reward transfer) with its own GAS_REWARD_PAY callback.
                    .with_static_gas(Gas::from_tgas(30))
                    .on_unstake_complete(caller, token_id, stake_id, reward)
            )
    }

    // ── Callbacks (invoked only by the contract itself) ────────────────────

    /// Callback after nft_transfer in unstake.
    /// If the transfer succeeded - pay out the reward.
    /// If the transfer failed - roll claimed back to false and return the reward to the pool.
    #[private]
    #[allow(deprecated)]
    pub fn on_unstake_complete(
        &mut self,
        caller:   AccountId,
        token_id: String,
        stake_id: String,
        reward:   U128,
    ) -> Option<Promise> {
        let transfer_succeeded = matches!(
            env::promise_result(0),
            near_sdk::PromiseResult::Successful(_)
        );

        if transfer_succeeded {
            // The NFT came back successfully - pay out the NEAR reward.
            // F-4: the transfer goes under a callback. It used to go out "into the void":
            // if it did not land (a deleted recipient account, say), the reward was lost
            // silently, while reward_pool had already been reduced at stake time.
            env::log_str(&format!(
                "UNSTAKE_SUCCESS | nft={} | owner={} | reward={} yN",
                token_id, caller, reward.0
            ));
            Some(
                Promise::new(caller.clone())
                    .transfer(NearToken::from_yoctonear(reward.0))
                    .then(
                        ext_self::ext(env::current_account_id())
                            .with_static_gas(GAS_REWARD_PAY)
                            .on_reward_paid(caller, stake_id, reward)
                    )
            )
        } else {
            // The NFT transfer failed - roll the stake back
            env::log_str(&format!(
                "UNSTAKE_FAILED | nft={} | owner={} | reason=nft_transfer_failed — rolling back",
                token_id, caller
            ));
            // Restore the stake
            if let Some(mut stake) = self.stakes.get(&stake_id).cloned() {
                stake.claimed = false;
                self.stakes.insert(stake_id, stake);
                self.count_claim_rolled_back(&caller);
            }
            // Return the reward to the pool
            self.reward_pool += reward.0;
            None
        }
    }

    /// Callback of the emergency return. On failure, roll back everything deducted on
    /// the way in: clear the claimed flag and take the reward back out of the pool.
    #[private]
    #[allow(deprecated)]
    pub fn on_admin_return_complete(&mut self, stake_id: String, stake: StakeInfo) {
        let returned = matches!(
            env::promise_result(0),
            near_sdk::PromiseResult::Successful(_)
        );

        if returned {
            env::log_str(&format!(
                "ADMIN_RETURN_SUCCESS | nft={} | owner={}",
                stake.token_id, stake.owner
            ));
        } else {
            self.reward_pool -= stake.reward_near.0;
            let restored = stake.clone();
            self.stakes.insert(stake_id, restored);
            self.count_claim_rolled_back(&stake.owner);
            env::log_str(&format!(
                "ADMIN_RETURN_FAILED | nft={} | owner={} — stake restored",
                stake.token_id, stake.owner
            ));
        }
    }

    /// F-4: callback after the reward transfer in on_unstake_complete.
    /// By this point the NFT is already with the user, so the stake must NOT be rolled
    /// back: resetting claimed = false would create a record that looks active while the
    /// NFT is no longer held by the contract - and it could be neither claimed nor
    /// deleted (the same class of problem as phantom listings in the marketplace).
    /// So on failure only the funds go back to the pool, and the event is logged
    /// explicitly so the owner can resolve the case by hand.
    #[private]
    #[allow(deprecated)]
    pub fn on_reward_paid(&mut self, caller: AccountId, stake_id: String, reward: U128) {
        let paid = matches!(
            env::promise_result(0),
            near_sdk::PromiseResult::Successful(_)
        );

        if paid {
            env::log_str(&format!(
                "REWARD_PAID | stake={} | owner={} | reward={} yN",
                stake_id, caller, reward.0
            ));
        } else {
            self.reward_pool += reward.0;
            env::log_str(&format!(
                "REWARD_PAY_FAILED | stake={} | owner={} | reward={} yN returned to pool — NFT already delivered, manual review required",
                stake_id, caller, reward.0
            ));
        }
    }

    /// Callback after withdraw_rewards.
    /// If the transfer failed - restore reward_pool.
    #[private]
    #[allow(deprecated)]
    pub fn on_withdraw_complete(&mut self, amount: U128) {
        let transfer_succeeded = matches!(
            env::promise_result(0),
            near_sdk::PromiseResult::Successful(_)
        );

        if transfer_succeeded {
            env::log_str(&format!(
                "WITHDRAW_SUCCESS | amount={} yN | pool_remaining={} yN",
                amount.0, self.reward_pool
            ));
        } else {
            // Return the funds to the pool
            self.reward_pool += amount.0;
            env::log_str(&format!(
                "WITHDRAW_FAILED | amount={} yN | pool_restored={} yN",
                amount.0, self.reward_pool
            ));
        }
    }

    // ── View Methods ───────────────────────────────────────────────────────

    pub fn get_stake(&self, account_id: AccountId, token_id: String) -> Option<StakeView> {
        let stake_id = format!("{}:{}", account_id, token_id);
        self.stakes.get(&stake_id).map(|s| self.to_view(&stake_id, s))
    }

    pub fn get_stakes_for_account(&self, account_id: AccountId) -> Vec<StakeView> {
        match self.stakes_by_account.get(&account_id) {
            None      => vec![],
            Some(ids) => ids.iter()
                .filter_map(|id| self.stakes.get(id).map(|s| self.to_view(id, s)))
                .collect(),
        }
    }

    pub fn get_reward_pool(&self) -> U128 {
        U128(self.reward_pool)
    }

    pub fn get_active_stakes_count(&self) -> U128 {
        U128(self.active_total() as u128)
    }

    pub fn is_paused(&self) -> bool {
        env::storage_has_key(K_PAUSED)
    }

    /// Counters against a full scan. Pays for the scan, so it is meant to be
    /// called by hand when something looks off, not on every read.
    pub fn audit_counters(&self) -> CounterAudit {
        let mut active_real  = 0u128;
        let mut claimed_real = 0u128;
        for (_, s) in self.stakes.iter() {
            if s.claimed {
                claimed_real += 1;
            } else {
                active_real += 1;
            }
        }
        CounterAudit {
            ready:           counters_ready(),
            active_counter:  U128(read_counter(K_ACTIVE) as u128),
            active_real:     U128(active_real),
            claimed_counter: U128(read_counter(K_CLAIMED) as u128),
            claimed_real:    U128(claimed_real),
        }
    }

    pub fn get_matured_stakes_count(&self) -> U128 {
        let now = env::block_timestamp() / 1_000_000_000;
        U128(self.stakes.iter().filter(|(_, s)| {
            !s.claimed && now >= s.staked_at + Self::period_sec_lenient(s.period_days)
        }).count() as u128)
    }

    pub fn get_total_staked(&self) -> U128 {
        U128(self.active_total() as u128)
    }

    pub fn get_contract_stats(&self) -> ContractStats {
        let now = env::block_timestamp() / 1_000_000_000;
        let unique = self.stakes.iter()
            .filter(|(_, s)| !s.claimed)
            .map(|(_, s)| s.owner.clone())
            .collect::<std::collections::HashSet<_>>()
            .len() as u128;
        ContractStats {
            owner_id:       self.owner_id.clone(),
            reward_pool:    U128(self.reward_pool),
            active_stakes:  U128(self.stakes.iter().filter(|(_, s)| !s.claimed).count() as u128),
            matured_stakes: U128(self.stakes.iter().filter(|(_, s)| {
                !s.claimed && now >= s.staked_at + Self::period_sec_lenient(s.period_days)
            }).count() as u128),
            total_staked:   U128(self.stakes.iter().filter(|(_, s)| !s.claimed).count() as u128),
            unique_stakers: U128(unique),
        }
    }

    /// Number of claimed (already paid out) stakes across the whole contract -
    /// unlike get_stakes_for_account it does not require knowing account_id up front.
    pub fn get_claimed_stakes_count(&self) -> U128 {
        U128(self.stakes.iter().filter(|(_, s)| s.claimed).count() as u128)
    }

    /// List of claimed stake keys in the form "owner:token_id" (the same format as the
    /// internal stake_id - see how stake_id is built in fn stake()).
    /// A light variant without details, kept as a simple standalone method; for the full
    /// information on each stake see get_claimed_stakes() below.
    pub fn get_claimed_stake_ids(&self) -> Vec<String> {
        let mut result: Vec<String> = Vec::new();
        for (id, s) in self.stakes.iter() {
            if s.claimed {
                result.push(id.clone());
            }
        }
        result
    }

    /// Unique accounts that have already claimed something.
    pub fn get_unique_claimers_count(&self) -> U128 {
        let unique = self.stakes.iter()
            .filter(|(_, s)| s.claimed)
            .map(|(_, s)| s.owner.clone())
            .collect::<std::collections::HashSet<_>>()
            .len() as u128;
        U128(unique)
    }

    /// Paginated list of claimed stakes with full details (amount, dates, period).
    /// from_index/limit - pagination. This variant (struct + pagination) was believed to
    /// fail reliably with WasmTrap on this near-sdk/toolchain combination - but the cause
    /// turned out to be a bug in near-cli-rs itself when passing non-empty arguments
    /// through as-read-only, not a problem in the contract. Checked directly over RPC
    /// (bypassing near-cli-rs) - it works correctly.
    ///
    /// Do not panic on a non-standard period_days (legacy/corrupted records) - for those
    /// unlocks_at simply equals staked_at, instead of failing the whole selection.
    pub fn get_claimed_stakes(&self, from_index: u64, limit: u64) -> Vec<ClaimedStakeView> {
        let mut result: Vec<ClaimedStakeView> = Vec::new();
        let mut idx: u64 = 0;
        for (id, s) in self.stakes.iter() {
            if !s.claimed {
                continue;
            }
            if idx < from_index {
                idx += 1;
                continue;
            }
            if (result.len() as u64) >= limit {
                break;
            }
            let period_sec = match s.period_days {
                7  => PERIOD_7_DAYS,
                14 => PERIOD_14_DAYS,
                30 => PERIOD_30_DAYS,
                _  => 0,
            };
            result.push(ClaimedStakeView {
                stake_id:    id.clone(),
                owner:       s.owner.to_string(),
                token_id:    s.token_id.clone(),
                staked_at:   s.staked_at,
                period_days: s.period_days,
                unlocks_at:  s.staked_at + period_sec,
                reward_near: s.reward_near,
            });
            idx += 1;
        }
        result
    }

    /// Shared private aggregation for the leaderboard: (owner, active NFT count, total reward).
    /// stakes_by_account is a LookupMap and cannot enumerate its keys, so the count goes
    /// through self.stakes.iter() (an IterableMap, already used for the same kind of
    /// aggregation in get_unique_claimers_count/get_claimed_stakes_count).
    fn compute_rankings(&self) -> Vec<(AccountId, u32, u128)> {
        let mut agg: std::collections::HashMap<AccountId, (u32, u128)> = std::collections::HashMap::new();
        for (_, s) in self.stakes.iter() {
            if s.claimed {
                continue;
            }
            let e = agg.entry(s.owner.clone()).or_insert((0u32, 0u128));
            e.0 += 1;
            e.1 += s.reward_near.0;
        }
        let mut list: Vec<(AccountId, u32, u128)> = agg
            .into_iter()
            .map(|(owner, (count, reward))| (owner, count, reward))
            .collect();
        // The secondary criterion (owner) is required for determinism: without it, ties on
        // nft_count would depend on HashMap iteration order, which changes between separate
        // calls (get_leaderboard and get_my_rank could then disagree on the position of
        // accounts holding the same number of NFTs).
        list.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        list
    }

    /// Top stakers by number of active (unclaimed) NFTs, descending.
    pub fn get_leaderboard(&self, limit: u64) -> Vec<LeaderboardEntry> {
        self.compute_rankings()
            .into_iter()
            .take(limit as usize)
            .map(|(owner, nft_count, total_reward)| LeaderboardEntry {
                owner: owner.to_string(),
                nft_count,
                total_reward: U128(total_reward),
            })
            .collect()
    }

    /// Position of a given account in the overall ranking (None if it has no active NFTs).
    pub fn get_my_rank(&self, account_id: AccountId) -> Option<MyRankEntry> {
        let rankings = self.compute_rankings();
        rankings
            .iter()
            .position(|(owner, _, _)| owner == &account_id)
            .map(|idx| {
                let (owner, nft_count, total_reward) = &rankings[idx];
                MyRankEntry {
                    position:     (idx + 1) as u32,
                    owner:        owner.to_string(),
                    nft_count:    *nft_count,
                    total_reward: U128(*total_reward),
                }
            })
    }

    /// Reward preview for STAKING SEVERAL NFTs AT ONCE (a batch).
    /// current_active_count - how many NFTs this account already has actively staked
    /// (positions run from current_active_count+1 to current_active_count+add_count).
    /// add_count - how many NFTs are being added now.
    /// Returns the SUM of rewards over all added NFTs, correctly handling a crossing of
    /// tier boundaries (e.g. if a staker already holds 8 NFTs and adds 5, two of them land
    /// in tier 1-10 and three in tier 11-20).
    pub fn preview_reward(&self, period_days: u8, current_active_count: u32, add_count: u32) -> U128 {
        require!(
            period_days == 7 || period_days == 14 || period_days == 30,
            "preview_reward: period_days must be 7, 14, or 30"
        );
        require!(add_count > 0, "preview_reward: add_count must be > 0");
        let mut total: u128 = 0;
        for i in 0..add_count {
            let position = current_active_count + i + 1;
            total += self.calc_reward(period_days, position);
        }
        U128(total)
    }

    /// APR (bps) for a specific NFT position of a staker (1st, 2nd, ... Nth).
    /// The same for every period (7/14/30 days) - the reward scales with the term inside
    /// calc_reward rather than through a separate APR per period.
    pub fn get_apr_bps(&self, position: u32) -> u64 {
        require!(position > 0, "get_apr_bps: position must be > 0");
        Self::apr_bps_for_position(position)
    }

    // ── Internal ───────────────────────────────────────────────────────────

    fn assert_owner(&self) {
        require!(
            env::predecessor_account_id() == self.owner_id,
            "Access denied: caller is not the contract owner"
        );
    }

    /// F-2: a stronger check for the methods that dispose of funds and state.
    /// Requiring exactly 1 yoctoNEAR forces the call to be signed with a full-access key:
    /// function-call keys (the kind issued to frontends and bots) cannot attach a deposit.
    /// At the time of this fix the contract account holds only full-access keys, so there
    /// is no live vector - this guards the future, should such a key ever be added.
    /// Deliberately NOT applied to deposit_rewards: that one accepts an arbitrary deposit.
    fn assert_owner_one_yocto(&self) {
        require!(
            env::attached_deposit().as_yoctonear() == 1,
            "attach exactly 1 yoctoNEAR"
        );
        self.assert_owner();
    }

    /// Strict version - only for write paths where period_days is already validated
    /// (exclusively 7/14/30 are accepted). A panic here means a bug in the code, not bad
    /// data.
    fn period_sec(days: u8) -> u64 {
        match days {
            7  => PERIOD_7_DAYS,
            14 => PERIOD_14_DAYS,
            30 => PERIOD_30_DAYS,
            _  => env::panic_str("period_sec: invalid period — must be 7, 14, or 30"),
        }
    }

    /// F-5: lenient version, for reads and for unstake.
    /// period_sec used to panic everywhere, and a single record with a non-standard
    /// period_days (legacy from early versions) took down get_stake,
    /// get_stakes_for_account, get_matured_stakes_count and get_contract_stats for ALL
    /// users at once. Worse, the same call sat inside unstake - so the owner of such a
    /// record could not retrieve their NFT at all. Returning 0 means "the term has already
    /// elapsed": for a broken record that is the safest outcome - the user gets the NFT back.
    fn period_sec_lenient(days: u8) -> u64 {
        match days {
            7  => PERIOD_7_DAYS,
            14 => PERIOD_14_DAYS,
            30 => PERIOD_30_DAYS,
            _  => 0,
        }
    }

    /// Active stakes contract-wide. Reads the counter when it is authoritative,
    /// otherwise scans, which is what the contract did before F-3 was fixed.
    fn active_total(&self) -> u64 {
        if counters_ready() {
            read_counter(K_ACTIVE)
        } else {
            self.stakes.iter().filter(|(_, s)| !s.claimed).count() as u64
        }
    }

    /// Active stakes of one account, same fallback.
    fn active_for(&self, account_id: &AccountId) -> u32 {
        if counters_ready() {
            read_counter(&ctr_key_for(account_id)) as u32
        } else {
            self.active_stake_count(account_id)
        }
    }

    /// The three hooks below are the whole maintenance burden the counters
    /// bring, and the rollback one is the dangerous member of the set: it runs
    /// in a callback after a failed transfer, and forgetting it there is how a
    /// counter silently starts to lie. audit_counters exists to catch that.
    fn count_stake_created(&self, owner: &AccountId) {
        if counters_ready() {
            bump_counter(K_ACTIVE, 1);
            bump_counter(&ctr_key_for(owner), 1);
        }
    }

    fn count_stake_claimed(&self, owner: &AccountId) {
        if counters_ready() {
            bump_counter(K_ACTIVE, -1);
            bump_counter(K_CLAIMED, 1);
            bump_counter(&ctr_key_for(owner), -1);
        }
    }

    fn count_claim_rolled_back(&self, owner: &AccountId) {
        if counters_ready() {
            bump_counter(K_ACTIVE, 1);
            bump_counter(K_CLAIMED, -1);
            bump_counter(&ctr_key_for(owner), 1);
        }
    }

    fn active_stake_count(&self, account_id: &AccountId) -> u32 {
        self.stakes_by_account
            .get(account_id)
            .map(|ids| {
                ids.iter()
                    .filter(|id| self.stakes.get(*id).map_or(false, |s| !s.claimed))
                    .count() as u32
            })
            .unwrap_or(0)
    }

    /// Returns the APR tier (bps) for a staker's NFT position (1st, 2nd, ... Nth).
    fn apr_bps_for_position(position: u32) -> u64 {
        match position {
            1..=10  => TIER_APR_BPS[0],
            11..=20 => TIER_APR_BPS[1],
            21..=30 => TIER_APR_BPS[2],
            31..=40 => TIER_APR_BPS[3],
            41..=50 => TIER_APR_BPS[4],
            _       => TIER_APR_BPS[5], // 51+
        }
    }

    /// Reward for ONE specific NFT at the given position of a staker.
    /// BUG FIX: this used to multiply by nft_count (the position), so the reward for a
    /// single NFT grew in proportion to its ordinal number instead of merely selecting
    /// the rate tier. The multiplier is now always 1 (it is one single NFT), and the tier
    /// only picks the APR.
    fn calc_reward(&self, period_days: u8, position: u32) -> u128 {
        let apr  = Self::apr_bps_for_position(position) as u128;
        let days = period_days as u128;
        NFT_VALUE_YOCTO
            .checked_mul(apr)
            .expect("calc_reward: overflow in apr multiply")
            .checked_mul(days)
            .expect("calc_reward: overflow in days multiply")
            / 10_000
            / 365
    }

    fn to_view(&self, stake_id: &str, s: &StakeInfo) -> StakeView {
        let now_sec    = env::block_timestamp() / 1_000_000_000;
        let period_sec = Self::period_sec_lenient(s.period_days);
        StakeView {
            stake_id:    stake_id.to_string(),
            token_id:    s.token_id.clone(),
            staked_at:   s.staked_at,
            period_days: s.period_days,
            unlocks_at:  s.staked_at + period_sec,
            reward_near: s.reward_near,
            is_mature:   now_sec >= s.staked_at + period_sec,
            claimed:     s.claimed,
        }
    }
}
