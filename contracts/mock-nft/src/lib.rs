//! Minimal NEP-171 collection for integration tests.
//!
//! It implements only what the staking and marketplace contracts actually touch:
//! ownership, `nft_transfer`, and the `nft_transfer_call` → `nft_on_transfer` →
//! `nft_resolve_transfer` round trip, including the part that matters most — a
//! receiver returning `true` gets the token sent back to its previous owner.
//!
//! Deliberately not a full NEP-171: no approvals of its own, no royalties, no
//! metadata. Anything the contracts under test do not call would only add
//! surface that the tests never exercise.

use near_sdk::json_types::U128;
use near_sdk::store::IterableMap;
use near_sdk::{
    env, ext_contract, near, require, AccountId, Gas, NearToken, PanicOnDefault, PromiseError,
    PromiseOrValue,
};

/// Budget for the receiver's nft_on_transfer. Generous on purpose: the staking
/// contract schedules a refund transfer of its own inside that call.
const GAS_ON_TRANSFER: Gas = Gas::from_tgas(80);
const GAS_RESOLVE: Gas = Gas::from_tgas(20);

#[near(serializers = [json])]
#[derive(Clone)]
pub struct TokenView {
    pub token_id: String,
    pub owner_id: AccountId,
}

#[ext_contract(ext_receiver)]
trait NftReceiver {
    fn nft_on_transfer(
        &mut self,
        sender_id: AccountId,
        previous_owner_id: AccountId,
        token_id: String,
        msg: String,
    ) -> PromiseOrValue<bool>;
}

#[ext_contract(ext_self)]
trait SelfExt {
    fn nft_resolve_transfer(&mut self, previous_owner_id: AccountId, token_id: String) -> bool;
}

#[near(contract_state)]
#[derive(PanicOnDefault)]
pub struct MockNft {
    owners: IterableMap<String, AccountId>,
    /// When set, `nft_transfer` panics. A real collection can fail a transfer
    /// for reasons the caller cannot produce on demand — a deleted recipient,
    /// for one — and the contracts under test are supposed to refund and
    /// restore state when that happens. Without a way to make the failure
    /// happen, that path is untestable.
    fail_transfers: bool,
}

#[near]
impl MockNft {
    #[init]
    pub fn new() -> Self {
        require!(!env::state_exists(), "Already initialized");
        Self {
            owners: IterableMap::new(b"o".to_vec()),
            fail_transfers: false,
        }
    }

    // ── Test helpers (not part of NEP-171) ──────────────────────────────────

    /// Makes every subsequent `nft_transfer` fail. Test-only.
    pub fn set_fail_transfers(&mut self, fail: bool) {
        self.fail_transfers = fail;
    }

    pub fn nft_mint(&mut self, token_id: String, receiver_id: AccountId) {
        require!(
            self.owners.insert(token_id.clone(), receiver_id).is_none(),
            "nft_mint: token_id already exists"
        );
    }

    // ── NEP-171 ─────────────────────────────────────────────────────────────

    pub fn nft_token(&self, token_id: String) -> Option<TokenView> {
        self.owners.get(&token_id).map(|owner| TokenView {
            token_id,
            owner_id: owner.clone(),
        })
    }

    pub fn nft_tokens_for_owner(
        &self,
        account_id: AccountId,
        from_index: Option<U128>,
        limit: Option<u64>,
    ) -> Vec<TokenView> {
        let start = from_index.map(|v| v.0 as usize).unwrap_or(0);
        let take = limit.unwrap_or(50) as usize;
        self.owners
            .iter()
            .filter(|(_, owner)| **owner == account_id)
            .skip(start)
            .take(take)
            .map(|(id, owner)| TokenView {
                token_id: id.clone(),
                owner_id: owner.clone(),
            })
            .collect()
    }

    #[payable]
    pub fn nft_transfer(
        &mut self,
        receiver_id: AccountId,
        token_id: String,
        #[allow(unused_variables)] approval_id: Option<u64>,
        #[allow(unused_variables)] memo: Option<String>,
    ) {
        require!(
            env::attached_deposit() == NearToken::from_yoctonear(1),
            "nft_transfer: requires attached deposit of exactly 1 yoctoNEAR"
        );
        require!(!self.fail_transfers, "nft_transfer: simulated failure");
        let owner = self
            .owners
            .get(&token_id)
            .cloned()
            .unwrap_or_else(|| env::panic_str("nft_transfer: token not found"));
        require!(
            owner == env::predecessor_account_id(),
            "nft_transfer: sender is not the owner"
        );
        self.owners.insert(token_id, receiver_id);
    }

    #[payable]
    pub fn nft_transfer_call(
        &mut self,
        receiver_id: AccountId,
        token_id: String,
        #[allow(unused_variables)] approval_id: Option<u64>,
        #[allow(unused_variables)] memo: Option<String>,
        msg: String,
    ) -> PromiseOrValue<bool> {
        require!(
            env::attached_deposit() == NearToken::from_yoctonear(1),
            "nft_transfer_call: requires attached deposit of exactly 1 yoctoNEAR"
        );
        let sender = env::predecessor_account_id();
        let previous_owner = self
            .owners
            .get(&token_id)
            .cloned()
            .unwrap_or_else(|| env::panic_str("nft_transfer_call: token not found"));
        require!(
            previous_owner == sender,
            "nft_transfer_call: sender is not the owner"
        );

        // Ownership moves first, exactly as a real NEP-171 collection does: the
        // receiver must already own the token while nft_on_transfer runs.
        self.owners.insert(token_id.clone(), receiver_id.clone());

        PromiseOrValue::Promise(
            ext_receiver::ext(receiver_id)
                .with_static_gas(GAS_ON_TRANSFER)
                .nft_on_transfer(sender, previous_owner.clone(), token_id.clone(), msg)
                .then(
                    ext_self::ext(env::current_account_id())
                        .with_static_gas(GAS_RESOLVE)
                        .nft_resolve_transfer(previous_owner, token_id),
                ),
        )
    }

    /// `true` from the receiver, or a failed call, means the token comes back.
    #[private]
    pub fn nft_resolve_transfer(
        &mut self,
        previous_owner_id: AccountId,
        token_id: String,
        #[callback_result] result: Result<bool, PromiseError>,
    ) -> bool {
        let return_token = match result {
            Ok(needs_return) => needs_return,
            Err(_) => true,
        };
        if return_token {
            self.owners.insert(token_id, previous_owner_id);
        }
        return_token
    }
}
