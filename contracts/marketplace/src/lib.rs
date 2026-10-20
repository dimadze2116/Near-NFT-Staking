use near_sdk::store::IterableMap;
use near_sdk::json_types::U128;
use near_sdk::{
    env, near, require, AccountId, BorshStorageKey,
    Gas, NearToken, PanicOnDefault, Promise, PromiseOrValue, ext_contract,
};

const NFT_CONTRACT_ID: &str = "nearlegion.nfts.tg";

/// Marketplace fee in basis points (1% = 100 bps). 50 bps = 0.5%.
const FEE_BPS: u128 = 50;

// These values come from actual mainnet measurements, not guesswork:
// nft_transfer on nearlegion.nfts.tg burns 3.53 TGas, the cancel callback 1.37 TGas.
// The previous 50 TGas per transfer left a 14x margin and made it impossible to
// fit four items under the transaction ceiling (300 TGas).
const GAS_NFT_TRANSFER: Gas = Gas::from_tgas(25);
const GAS_CALLBACK:     Gas = Gas::from_tgas(10);

/// The purchase callback spawns another promise of its own (the seller payout with
/// its own callback, M-6), so it needs a larger budget than usual: its own
/// execution plus GAS_SELLER_PAID for the nested call.
const GAS_PURCHASE_CALLBACK: Gas = Gas::from_tgas(20);
const GAS_SELLER_PAID:       Gas = Gas::from_tgas(5);
/// Terminating callback of the batch - needed to close the chain with .then().
const GAS_BATCH_DONE:        Gas = Gas::from_tgas(5);

/// M-4: ceiling on items in a single purchase. Each one costs GAS_NFT_TRANSFER (25)
/// + GAS_PURCHASE_CALLBACK (20) = 45 TGas. Four items is 180 TGas, plus the
/// function's own execution (~6) and the terminating callback (5): about 191 of 300.
const MAX_CART_ITEMS: usize = 4;

/// M-8: ceiling on a listing price - 1M NEAR. Any value up to u128::MAX used to be
/// accepted. Such a listing not only cluttered the feed, it was physically
/// unpayable: computing the fee (price * FEE_BPS) overflowed, and with
/// overflow-checks = true that is a panic, so the purchase failed.
const MAX_LISTING_PRICE: u128 = 1_000_000 * 1_000_000_000_000_000_000_000_000u128;

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

#[ext_contract(ext_self)]
trait SelfCallback {
    fn on_purchase_complete(
        &mut self,
        buyer: AccountId,
        token_id: String,
        listing: Listing,
        fee: U128,
        seller_amount: U128,
    );
    fn on_cancel_complete(
        &mut self,
        token_id: String,
        listing: Listing,
    );
    fn on_withdraw_fees_complete(&mut self, amount: U128);
    fn on_seller_paid(
        &mut self,
        seller: AccountId,
        token_id: String,
        amount: U128,
    );
    fn on_admin_return_complete(
        &mut self,
        token_id: String,
        listing: Listing,
    );
    fn on_purchase_batch_done(
        &mut self,
        buyer: AccountId,
        items: u32,
    );
}

// ─── Storage keys ──────────────────────────────────────────────────────────────

#[derive(BorshStorageKey)]
#[near]
enum StorageKey {
    Listings,
}

/// Pause flag, kept under its own storage key rather than as a field of the
/// contract struct. Adding a field changes the borsh layout of the state, which
/// means the new code cannot read the old bytes until a migration has run — on
/// a contract holding other people's NFTs in escrow that is not a step worth
/// taking for a boolean. A separate key needs no migration: its absence simply
/// means "not paused", which is how the contract behaved before this existed.
///
/// The prefix is text so it cannot collide with the collection prefix produced
/// by StorageKey's discriminant (0x00) or with "STATE".
const K_PAUSED: &[u8] = b"flag:paused";

// ─── Data Structures ────────────────────────────────────────────────────────────

/// A listing. On listing, the NFT is transferred into the contract (escrow) -
/// the same way the staking contract holds staked NFTs itself.
#[near(serializers = [borsh, json])]
#[derive(Clone)]
pub struct Listing {
    pub seller:    AccountId,
    pub price:     U128, // price in yoctoNEAR, before the fee
    pub listed_at: u64,  // unix time (seconds)
}

/// View form of a listing - includes token_id (Listing itself does not carry it,
/// it is the map key, but the client needs it in the view response).
#[near(serializers = [json])]
pub struct ListingView {
    pub token_id:  String,
    pub seller:    AccountId,
    pub price:     U128,
    pub listed_at: u64,
}

#[near(serializers = [json])]
pub struct MarketplaceStats {
    pub owner_id:        AccountId,
    pub active_listings:  U128,
    pub fees_collected:   U128,
}

// ─── Contract ───────────────────────────────────────────────────────────────────

#[near(contract_state)]
#[derive(PanicOnDefault)]
pub struct MarketplaceContract {
    pub owner_id:           AccountId,
    pub listings:           IterableMap<String, Listing>,
    pub fees_collected:     u128,
}

#[near]
impl MarketplaceContract {

    // ── Init ───────────────────────────────────────────────────────────────

    #[init]
    pub fn new(owner_id: AccountId) -> Self {
        require!(!env::state_exists(), "Already initialized");
        Self {
            owner_id,
            listings:       IterableMap::new(StorageKey::Listings),
            fees_collected: 0,
        }
    }

    // ── Receiving an NFT - listing it for sale ─────────────────────────────

    /// Called by the NFT contract through nft_transfer_call. `msg` must contain
    /// the price in yoctoNEAR as a string (e.g. "1500000000000000000000000"
    /// = 1.5 NEAR). Returns `true` if the NFT must be returned to the sender
    /// (invalid price / already listed), `false` if it was accepted for sale.
    pub fn nft_on_transfer(
        &mut self,
        #[allow(unused_variables)]
        sender_id: AccountId,
        previous_owner_id: AccountId,
        token_id: String,
        msg: String,
    ) -> PromiseOrValue<bool> {
        let nft_contract: AccountId = NFT_CONTRACT_ID
            .parse()
            .expect("Invalid NFT contract ID in code");
        require!(
            env::predecessor_account_id() == nft_contract,
            "nft_on_transfer: caller is not the authorized NFT contract"
        );

        // Paused means no new listings. Returning true sends the NFT back;
        // panicking here would strand it on the contract with no listing.
        if env::storage_has_key(K_PAUSED) {
            env::log_str(&format!(
                "LISTING_REJECTED | reason=paused | nft={} | seller={}",
                token_id, previous_owner_id
            ));
            return PromiseOrValue::Value(true);
        }

        let price: u128 = match msg.trim().parse::<u128>() {
            Ok(p) if p > 0 && p <= MAX_LISTING_PRICE => p,
            Ok(_) => {
                env::log_str(&format!(
                    "LISTING_REJECTED | reason=price_out_of_range | nft={} | seller={}",
                    token_id, previous_owner_id
                ));
                return PromiseOrValue::Value(true);
            }
            Err(_) => {
                env::log_str(&format!(
                    "LISTING_REJECTED | reason=price_not_a_number | msg={} | nft={} | seller={}",
                    msg, token_id, previous_owner_id
                ));
                return PromiseOrValue::Value(true);
            }
        };

        if self.listings.contains_key(&token_id) {
            env::log_str(&format!(
                "LISTING_REJECTED | reason=already_listed | nft={}",
                token_id
            ));
            return PromiseOrValue::Value(true);
        }

        let listing = Listing {
            seller:    previous_owner_id.clone(),
            price:     U128(price),
            listed_at: env::block_timestamp() / 1_000_000_000,
        };
        self.listings.insert(token_id.clone(), listing);

        env::log_str(&format!(
            "LISTING_CREATED | nft={} | seller={} | price={} yN",
            token_id, previous_owner_id, price
        ));

        PromiseOrValue::Value(false)
    }

    // ── Cancelling a listing ───────────────────────────────────────────────

    /// The seller takes their NFT off sale and gets it back.
    #[payable]
    pub fn cancel_listing(&mut self, token_id: String) -> Promise {
        // M-5: this method moves an asset, so per NEP-171 it must be signed with a
        // full-access key - a function-call key cannot attach a deposit.
        require!(
            env::attached_deposit().as_yoctonear() == 1,
            "cancel_listing: attach exactly 1 yoctoNEAR"
        );

        let caller = env::predecessor_account_id();

        let listing = self
            .listings
            .get(&token_id)
            .cloned()
            .unwrap_or_else(|| env::panic_str("cancel_listing: listing not found"));

        require!(
            listing.seller == caller,
            "cancel_listing: caller is not the seller of this listing"
        );

        // Remove it IMMEDIATELY (guards against a concurrent cancel/buy) -
        // if nft_transfer below fails, the callback puts the listing back.
        self.listings.remove(&token_id);

        let nft_contract: AccountId = NFT_CONTRACT_ID.parse().unwrap();

        env::log_str(&format!(
            "LISTING_CANCEL_INIT | nft={} | seller={}",
            token_id, caller
        ));

        ext_nft::ext(nft_contract)
            .with_attached_deposit(NearToken::from_yoctonear(1))
            .with_static_gas(GAS_NFT_TRANSFER)
            .nft_transfer(
                caller.clone(),
                token_id.clone(),
                Some("Marketplace listing cancelled".to_string()),
            )
            .then(
                ext_self::ext(env::current_account_id())
                    .with_static_gas(GAS_CALLBACK)
                    .on_cancel_complete(token_id, listing)
            )
    }

    // ── Purchase (one or several NFTs at once - the "cart") ────────────────

    /// The buyer attaches a NEAR deposit >= the sum of the prices of all selected
    /// listings. Each NFT is transferred by its own promise with its own callback -
    /// if one particular transfer fails, that one listing goes back on sale while
    /// the rest of the batch is unaffected.
    /// Any excess deposit (more than the sum of prices) is returned to the buyer
    /// as a separate transfer, independent of how the transfers turn out.
    #[payable]
    pub fn buy_multiple(&mut self, token_ids: Vec<String>) -> Promise {
        // Panicking is right here, unlike in nft_on_transfer: nothing has been
        // charged or moved yet, so the buyer simply keeps their deposit.
        require!(
            !env::storage_has_key(K_PAUSED),
            "buy_multiple: the marketplace is paused"
        );
        require!(!token_ids.is_empty(), "buy_multiple: token_ids must not be empty");

        // M-4: each item costs GAS_NFT_TRANSFER (25) + GAS_PURCHASE_CALLBACK (20) =
        // 45 TGas. Under the 300 TGas ceiling no more than four items fit - the call
        // used to fail after all the checks, with a gas error the UI could not explain.
        require!(
            token_ids.len() <= MAX_CART_ITEMS,
            "buy_multiple: maximum 4 items per purchase"
        );

        // M-2: duplicates in the cart corrupted state. One listing was removed twice
        // (the second remove a no-op), but TWO nft_transfer calls were created for the
        // same NFT. The first succeeded, the second failed (the contract no longer owned
        // it), and its callback restored the listing - even though the NFT was no longer
        // held by the contract. Such a listing can be neither bought, cancelled nor relisted.
        let mut seen = std::collections::HashSet::new();
        require!(
            token_ids.iter().all(|id| seen.insert(id)),
            "buy_multiple: duplicate token_ids are not allowed"
        );

        let buyer    = env::predecessor_account_id();
        let attached = env::attached_deposit().as_yoctonear();

        // Collect and validate ALL listings before charging anything -
        // if even one of them is not on sale, the whole call fails
        // (more predictable for the UI: either the whole cart or nothing).
        let mut total_price: u128 = 0;
        let mut items: Vec<(String, Listing)> = Vec::with_capacity(token_ids.len());
        for token_id in &token_ids {
            let listing = self
                .listings
                .get(token_id)
                .unwrap_or_else(|| {
                    env::panic_str(&format!("buy_multiple: listing not found for {}", token_id))
                })
                .clone();
            total_price += listing.price.0;
            items.push((token_id.clone(), listing));
        }

        require!(
            attached >= total_price,
            &format!(
                "buy_multiple: attached deposit {} yN is less than total price {} yN",
                attached, total_price
            )
        );

        // Remove ALL listings from sale IMMEDIATELY (guards against a repeat
        // buy/cancel while the promises are in flight) - the per-NFT callback puts
        // that specific listing back if its transfer failed.
        for (token_id, _) in &items {
            self.listings.remove(token_id);
        }

        let nft_contract: AccountId = NFT_CONTRACT_ID.parse().unwrap();
        let refund = attached - total_price;

        env::log_str(&format!(
            "PURCHASE_INIT | buyer={} | items={} | total={} yN",
            buyer, items.len(), total_price
        ));

        let items_count = items.len();
        let mut combined: Option<Promise> = None;
        for (token_id, listing) in items {
            let fee           = listing.price.0 * FEE_BPS / 10_000;
            let seller_amount = listing.price.0 - fee;

            let p = ext_nft::ext(nft_contract.clone())
                .with_attached_deposit(NearToken::from_yoctonear(1))
                .with_static_gas(GAS_NFT_TRANSFER)
                .nft_transfer(
                    buyer.clone(),
                    token_id.clone(),
                    Some("Purchased on Near NFT Staking marketplace".to_string()),
                )
                .then(
                    ext_self::ext(env::current_account_id())
                        .with_static_gas(GAS_PURCHASE_CALLBACK)
                        .on_purchase_complete(buyer.clone(), token_id, listing, U128(fee), U128(seller_amount))
                );

            combined = Some(match combined {
                Some(existing) => existing.and(p),
                None => p,
            });
        }

        let final_promise = combined.unwrap();

        // A promise assembled with .and() (two or more items) must not be returned:
        // near-sdk answers "Returning joint promise is currently prohibited" and the
        // whole transaction reverts. Because of this, buying a cart of more than one
        // item never worked at all - a single item went through because .and() was
        // never called. The cure is to close the chain with .then(), which turns a
        // joint promise into an ordinary one. Before this fix it only worked by
        // accident, in the one case where there was excess deposit to refund.
        if refund > 0 {
            final_promise.then(Promise::new(buyer).transfer(NearToken::from_yoctonear(refund)))
        } else {
            final_promise.then(
                ext_self::ext(env::current_account_id())
                    .with_static_gas(GAS_BATCH_DONE)
                    .on_purchase_batch_done(buyer, items_count as u32)
            )
        }
    }

    // ── Withdrawing accumulated fees (owner only) ──────────────────────────

    #[payable]
    pub fn withdraw_fees(&mut self, amount: U128) -> Promise {
        self.assert_owner_one_yocto();
        let yocto = amount.0;
        require!(yocto > 0, "withdraw_fees: amount must be greater than zero");
        require!(
            self.fees_collected >= yocto,
            "withdraw_fees: requested amount exceeds collected fees balance"
        );

        // Deduct right away (guards against double-spend); the callback restores it on failure.
        self.fees_collected -= yocto;

        env::log_str(&format!(
            "WITHDRAW_FEES_INIT | amount={} yN | remaining={} yN",
            yocto, self.fees_collected
        ));

        Promise::new(self.owner_id.clone())
            .transfer(NearToken::from_yoctonear(yocto))
            .then(
                ext_self::ext(env::current_account_id())
                    .with_static_gas(GAS_CALLBACK)
                    .on_withdraw_fees_complete(amount)
            )
    }

    // ── Pause ──────────────────────────────────────────────────────────────────

    /// Closes the way in: no new listings, no purchases. Deliberately does not
    /// touch cancel_listing — a seller must be able to take their NFT out of
    /// escrow at any time. A pause that could trap other people's assets would
    /// not be a safety measure.
    #[payable]
    pub fn pause(&mut self) {
        self.assert_owner_one_yocto();
        env::storage_write(K_PAUSED, &[1u8]);
        env::log_str("PAUSED | listings and purchases refused, cancel unaffected");
    }

    #[payable]
    pub fn unpause(&mut self) {
        self.assert_owner_one_yocto();
        env::storage_remove(K_PAUSED);
        env::log_str("UNPAUSED");
    }

    pub fn is_paused(&self) -> bool {
        env::storage_has_key(K_PAUSED)
    }

    // ── Callbacks (invoked only by the contract itself) ────────────────────

    /// Callback after nft_transfer in buy_multiple.
    /// Success - record the fee, pay the seller their share.
    /// Failure - put the listing back on sale (no payouts).
    #[private]
    #[allow(deprecated)]
    pub fn on_purchase_complete(
        &mut self,
        buyer: AccountId,
        token_id: String,
        listing: Listing,
        fee: U128,
        seller_amount: U128,
    ) -> Option<Promise> {
        let transfer_succeeded = matches!(
            env::promise_result(0),
            near_sdk::PromiseResult::Successful(_)
        );

        if transfer_succeeded {
            self.fees_collected += fee.0;
            env::log_str(&format!(
                "PURCHASE_SUCCESS | nft={} | seller={} | seller_amount={} yN | fee={} yN",
                token_id, listing.seller, seller_amount.0, fee.0
            ));
            // M-6: the seller payout is now under a callback. It used to go out without
            // one - if the transfer failed (a deleted seller account, say), the money
            // silently stayed on the contract with no way to get it out.
            Some(
                Promise::new(listing.seller.clone())
                    .transfer(NearToken::from_yoctonear(seller_amount.0))
                    .then(
                        ext_self::ext(env::current_account_id())
                            .with_static_gas(GAS_SELLER_PAID)
                            .on_seller_paid(listing.seller, token_id, seller_amount)
                    )
            )
        } else {
            // M-1: this used to simply put the listing back on sale while the buyer's
            // money (listing.price) stayed on the contract balance - it reached neither
            // the seller nor fees_collected, and nobody could retrieve it:
            // withdraw_fees is bounded by fees_collected. The buyer was left without the
            // NFT and without the money. Now the full price of the item is refunded.
            let refund = listing.price.0;
            env::log_str(&format!(
                "PURCHASE_FAILED | nft={} | buyer={} | refund={} yN | reason=nft_transfer_failed — relisting",
                token_id, buyer, refund
            ));
            self.listings.insert(token_id, listing);
            Some(Promise::new(buyer).transfer(NearToken::from_yoctonear(refund)))
        }
    }

    /// Callback after nft_transfer in cancel_listing.
    /// Failure - restore the listing (the cancellation did not happen).
    #[private]
    #[allow(deprecated)]
    pub fn on_cancel_complete(&mut self, token_id: String, listing: Listing) {
        let transfer_succeeded = matches!(
            env::promise_result(0),
            near_sdk::PromiseResult::Successful(_)
        );

        if transfer_succeeded {
            env::log_str(&format!("LISTING_CANCEL_SUCCESS | nft={}", token_id));
        } else {
            env::log_str(&format!(
                "LISTING_CANCEL_FAILED | nft={} | reason=nft_transfer_failed — relisting",
                token_id
            ));
            self.listings.insert(token_id, listing);
        }
    }

    /// M-6: callback after the seller payout. The sale cannot be rolled back here -
    /// the NFT is already with the buyer and the buyer has been charged. So on failure
    /// the amount is credited to fees_collected: that makes it reachable by the owner
    /// through withdraw_fees and transferable to the seller by hand, instead of being
    /// stuck on the contract balance forever.
    #[private]
    #[allow(deprecated)]
    pub fn on_seller_paid(&mut self, seller: AccountId, token_id: String, amount: U128) {
        let paid = matches!(
            env::promise_result(0),
            near_sdk::PromiseResult::Successful(_)
        );

        if paid {
            env::log_str(&format!(
                "SELLER_PAID | nft={} | seller={} | amount={} yN",
                token_id, seller, amount.0
            ));
        } else {
            self.fees_collected += amount.0;
            env::log_str(&format!(
                "SELLER_PAY_FAILED | nft={} | seller={} | amount={} yN moved to fees for manual settlement",
                token_id, seller, amount.0
            ));
        }
    }

    /// Terminating callback of the purchase batch. It carries no useful logic: its
    /// only job is to close the chain with .then(), so the function returns an
    /// ordinary promise rather than a joint one (see buy_multiple).
    /// It also leaves a log entry marking that the batch reached the end.
    #[private]
    pub fn on_purchase_batch_done(&mut self, buyer: AccountId, items: u32) {
        env::log_str(&format!(
            "PURCHASE_BATCH_DONE | buyer={} | items={}",
            buyer, items
        ));
    }

    /// Emergency return of a listed NFT to its seller.
    ///
    /// As in the staking contract, the recipient is NOT a parameter: the NFT always
    /// goes to the `listing.seller` recorded in state. The contract owner can trigger
    /// a return but cannot choose who receives it - the theft vector is closed.
    /// Needed in case state desynchronises: without it, an NFT stuck in escrow
    /// could not be returned at all.
    #[payable]
    pub fn admin_return_listing(&mut self, token_id: String) -> Promise {
        self.assert_owner_one_yocto();

        let listing = self
            .listings
            .get(&token_id)
            .unwrap_or_else(|| env::panic_str("admin_return_listing: listing not found"))
            .clone();

        // Remove the listing BEFORE the promise, otherwise a repeat call in the window
        // before the callback would send a second nft_transfer.
        self.listings.remove(&token_id);

        let nft_contract: AccountId = NFT_CONTRACT_ID.parse().unwrap();

        env::log_str(&format!(
            "ADMIN_RETURN_INIT | nft={} | seller={}",
            token_id, listing.seller
        ));

        ext_nft::ext(nft_contract)
            .with_attached_deposit(NearToken::from_yoctonear(1))
            .with_static_gas(GAS_NFT_TRANSFER)
            .nft_transfer(
                listing.seller.clone(),
                token_id.clone(),
                Some("Listing returned by contract owner".to_string()),
            )
            .then(
                ext_self::ext(env::current_account_id())
                    .with_static_gas(GAS_CALLBACK)
                    .on_admin_return_complete(token_id, listing)
            )
    }

    /// Callback of the emergency return: on failure, restore the listing.
    #[private]
    #[allow(deprecated)]
    pub fn on_admin_return_complete(&mut self, token_id: String, listing: Listing) {
        let returned = matches!(
            env::promise_result(0),
            near_sdk::PromiseResult::Successful(_)
        );

        if returned {
            env::log_str(&format!(
                "ADMIN_RETURN_SUCCESS | nft={} | seller={}",
                token_id, listing.seller
            ));
        } else {
            env::log_str(&format!(
                "ADMIN_RETURN_FAILED | nft={} | seller={} — listing restored",
                token_id, listing.seller
            ));
            self.listings.insert(token_id, listing);
        }
    }

    /// Callback after withdraw_fees. On failure, return the amount to the fee balance.
    #[private]
    #[allow(deprecated)]
    pub fn on_withdraw_fees_complete(&mut self, amount: U128) {
        let transfer_succeeded = matches!(
            env::promise_result(0),
            near_sdk::PromiseResult::Successful(_)
        );

        if transfer_succeeded {
            env::log_str(&format!("WITHDRAW_FEES_SUCCESS | amount={} yN", amount.0));
        } else {
            env::log_str(&format!(
                "WITHDRAW_FEES_FAILED | amount={} yN — restoring balance",
                amount.0
            ));
            self.fees_collected += amount.0;
        }
    }

    // ── View methods ───────────────────────────────────────────────────────

    /// Paginated list of all active listings (for the "Market" tab).
    pub fn get_listings(&self, from_index: u64, limit: u64) -> Vec<ListingView> {
        let mut result: Vec<ListingView> = Vec::new();
        let mut idx: u64 = 0;
        for (token_id, listing) in self.listings.iter() {
            if idx < from_index {
                idx += 1;
                continue;
            }
            if (result.len() as u64) >= limit {
                break;
            }
            result.push(ListingView {
                token_id:  token_id.clone(),
                seller:    listing.seller.clone(),
                price:     listing.price,
                listed_at: listing.listed_at,
            });
            idx += 1;
        }
        result
    }

    /// A single listing by token_id (for an NFT's page / a pre-purchase check).
    pub fn get_listing(&self, token_id: String) -> Option<ListingView> {
        self.listings.get(&token_id).map(|listing| ListingView {
            token_id:  token_id.clone(),
            seller:    listing.seller.clone(),
            price:     listing.price,
            listed_at: listing.listed_at,
        })
    }

    /// Active listings of a specific seller (the "My listings" tab).
    /// M-7: this method used to return ALL of a seller's listings in one piece, with
    /// no limit. As the number of items grew, the response would one day stop fitting
    /// into the gas limit of a view call. Pagination is now mandatory, as in get_listings.
    pub fn get_listings_by_seller(
        &self,
        account_id: AccountId,
        from_index: u64,
        limit: u64,
    ) -> Vec<ListingView> {
        require!(limit > 0 && limit <= 100, "get_listings_by_seller: limit must be between 1 and 100");

        self.listings
            .iter()
            .filter(|(_, listing)| listing.seller == account_id)
            .skip(from_index as usize)
            .take(limit as usize)
            .map(|(token_id, listing)| ListingView {
                token_id:  token_id.clone(),
                seller:    listing.seller.clone(),
                price:     listing.price,
                listed_at: listing.listed_at,
            })
            .collect()
    }

    pub fn get_active_listings_count(&self) -> U128 {
        U128(self.listings.len() as u128)
    }

    pub fn get_fees_collected(&self) -> U128 {
        U128(self.fees_collected)
    }

    pub fn get_fee_bps(&self) -> U128 {
        U128(FEE_BPS)
    }

    pub fn get_marketplace_stats(&self) -> MarketplaceStats {
        MarketplaceStats {
            owner_id:        self.owner_id.clone(),
            active_listings: U128(self.listings.len() as u128),
            fees_collected:  U128(self.fees_collected),
        }
    }

    // ── Internal ───────────────────────────────────────────────────────────

    fn assert_owner(&self) {
        require!(
            env::predecessor_account_id() == self.owner_id,
            "Access denied: caller is not the contract owner"
        );
    }

    /// M-3: a stronger check for withdrawing fees. Requiring exactly 1 yoctoNEAR
    /// forces the call to be signed with a full-access key: function-call keys
    /// (the kind handed to bots and frontends) cannot attach a deposit.
    fn assert_owner_one_yocto(&self) {
        require!(
            env::attached_deposit().as_yoctonear() == 1,
            "attach exactly 1 yoctoNEAR"
        );
        self.assert_owner();
    }
}
