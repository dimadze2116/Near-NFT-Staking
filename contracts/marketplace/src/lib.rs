use near_sdk::store::IterableMap;
use near_sdk::json_types::U128;
use near_sdk::{
    env, near, require, AccountId, BorshStorageKey,
    Gas, NearToken, PanicOnDefault, Promise, PromiseOrValue, ext_contract,
};

const NFT_CONTRACT_ID: &str = "nearlegion.nfts.tg";

/// Комиссия маркетплейса, в basis points (1% = 100 bps). 50 bps = 0.5%.
const FEE_BPS: u128 = 50;

// Значения выставлены по фактическим замерам в mainnet, а не на глаз:
// nft_transfer у nearlegion.nfts.tg сжигает 3.53 TGas, колбэк отмены — 1.37 TGas.
// Прежние 50 TGas на перевод давали 14-кратный запас и не позволяли уместить
// четыре позиции в потолок транзакции (300 TGas).
const GAS_NFT_TRANSFER: Gas = Gas::from_tgas(25);
const GAS_CALLBACK:     Gas = Gas::from_tgas(10);

/// Колбэк покупки создаёт внутри себя ещё один промис (выплата продавцу со своим
/// колбэком, M-6), поэтому ему нужен бюджет больше обычного: своё исполнение
/// плюс GAS_SELLER_PAID для вложенного вызова.
const GAS_PURCHASE_CALLBACK: Gas = Gas::from_tgas(20);
const GAS_SELLER_PAID:       Gas = Gas::from_tgas(5);
/// Завершающий колбэк батча — нужен, чтобы замкнуть цепочку через .then().
const GAS_BATCH_DONE:        Gas = Gas::from_tgas(5);

/// M-4: потолок позиций в одной покупке. На каждую уходит GAS_NFT_TRANSFER (25)
/// + GAS_PURCHASE_CALLBACK (20) = 45 TGas. Четыре позиции — 180 TGas, плюс
/// исполнение самой функции (~6) и завершающий колбэк (5): около 191 из 300.
const MAX_CART_ITEMS: usize = 4;

/// M-8: потолок цены объявления — 1 млн NEAR. Раньше принималось любое значение
/// вплоть до u128::MAX. Такой лот не только засорял выдачу, но и был физически
/// неоплачиваем: при расчёте комиссии (price * FEE_BPS) происходило переполнение,
/// а при overflow-checks = true это паника, то есть покупка падала.
const MAX_LISTING_PRICE: u128 = 1_000_000 * 1_000_000_000_000_000_000_000_000u128;

// ─── External contract interfaces ─────────────────────────────────────────────

/// Интерфейс NFT контракта (NEP-171)
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

// ─── Data Structures ────────────────────────────────────────────────────────────

/// Объявление о продаже. NFT в момент выставления переводится в контракт
/// (escrow) — так же, как стейк-контракт держит застейканные NFT у себя.
#[near(serializers = [borsh, json])]
#[derive(Clone)]
pub struct Listing {
    pub seller:    AccountId,
    pub price:     U128, // цена в yoctoNEAR, без учёта комиссии
    pub listed_at: u64,  // unix-время (секунды)
}

/// View-версия объявления — включает token_id (в самом Listing его нет,
/// он и так ключ мапы, но во view-ответе он нужен клиенту).
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

    // ── Init ──────────────────────────────────────────────────────────────────

    #[init]
    pub fn new(owner_id: AccountId) -> Self {
        require!(!env::state_exists(), "Already initialized");
        Self {
            owner_id,
            listings:       IterableMap::new(StorageKey::Listings),
            fees_collected: 0,
        }
    }

    // ── Приём NFT — выставление на продажу ──────────────────────────────────────

    /// Вызывается NFT-контрактом через nft_transfer_call. `msg` должен
    /// содержать цену в yoctoNEAR строкой (например "1500000000000000000000000"
    /// = 1.5 NEAR). Возвращаем `true`, если нужно вернуть NFT отправителю
    /// (невалидная цена / уже выставлен), `false` — если приняли на продажу.
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

    // ── Отмена объявления ────────────────────────────────────────────────────────

    /// Продавец снимает свой NFT с продажи и забирает его обратно.
    #[payable]
    pub fn cancel_listing(&mut self, token_id: String) -> Promise {
        // M-5: метод перемещает актив, поэтому по NEP-171 требует подписи
        // full-access ключом — function-call ключ депозит приложить не может.
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

        // Удаляем СРАЗУ (защита от повторной отмены/покупки одновременно) —
        // если nft_transfer ниже упадёт, колбэк вернёт объявление обратно.
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

    // ── Покупка (одна или несколько NFT за раз — "корзина") ─────────────────────

    /// Покупатель прикладывает NEAR-депозит >= суммы цен всех выбранных
    /// объявлений. Каждый NFT переводится отдельным промисом с собственным
    /// колбэком — если конкретный трансфер не удался, именно это объявление
    /// возвращается на продажу, а остальные покупки в батче не страдают.
    /// Излишек депозита (если приложили больше суммы цен) возвращается
    /// покупателю отдельным переводом, независимо от исхода трансферов.
    #[payable]
    pub fn buy_multiple(&mut self, token_ids: Vec<String>) -> Promise {
        require!(!token_ids.is_empty(), "buy_multiple: token_ids must not be empty");

        // M-4: каждая позиция стоит GAS_NFT_TRANSFER (50) + GAS_CALLBACK (10) = 60 TGas.
        // При потолке в 300 TGas больше четырёх позиций собрать нельзя — раньше вызов
        // падал уже после всех проверок, с невнятной для UI ошибкой про газ.
        require!(
            token_ids.len() <= MAX_CART_ITEMS,
            "buy_multiple: maximum 4 items per purchase"
        );

        // M-2: дубликаты в корзине приводили к порче состояния. Одно объявление
        // удалялось дважды (второй remove — no-op), но создавалось ДВА nft_transfer
        // одного и того же NFT. Первый проходил, второй падал (контракт уже не владелец),
        // и его колбэк восстанавливал объявление — при том что NFT на контракте больше
        // нет. Такой лот нельзя ни купить, ни отменить, ни перевыставить.
        let mut seen = std::collections::HashSet::new();
        require!(
            token_ids.iter().all(|id| seen.insert(id)),
            "buy_multiple: duplicate token_ids are not allowed"
        );

        let buyer    = env::predecessor_account_id();
        let attached = env::attached_deposit().as_yoctonear();

        // Собираем и проверяем ВСЕ объявления до какого-либо списания —
        // если хотя бы одного нет в продаже, весь вызов падает целиком
        // (предсказуемее для UI: либо покупаем всё из корзины, либо ничего).
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

        // Удаляем ВСЕ объявления из продажи СРАЗУ (защита от повторной
        // покупки/отмены, пока идут промисы) — колбэк на каждый NFT вернёт
        // именно его объявление обратно, если конкретный трансфер не удался.
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

        // Промис, собранный через .and() (две и более позиции), возвращать нельзя:
        // near-sdk отвечает "Returning joint promise is currently prohibited", и вся
        // транзакция откатывается. Из-за этого покупка корзиной больше одной позиции
        // не работала вовсе — одиночная проходила, потому что .and() не вызывался.
        // Лечится замыканием цепочки через .then(): он превращает совместный промис
        // в обычный. Раньше это случайно срабатывало только при наличии сдачи.
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

    // ── Вывод накопленной комиссии (только владелец) ────────────────────────────

    #[payable]
    pub fn withdraw_fees(&mut self, amount: U128) -> Promise {
        self.assert_owner_one_yocto();
        let yocto = amount.0;
        require!(yocto > 0, "withdraw_fees: amount must be greater than zero");
        require!(
            self.fees_collected >= yocto,
            "withdraw_fees: requested amount exceeds collected fees balance"
        );

        // Вычитаем сразу (защита от double-spend), откатим в колбэке при неудаче.
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

    // ── Callbacks (вызываются только самим контрактом) ──────────────────────────

    /// Колбэк после nft_transfer в buy_multiple.
    /// Успех — засчитываем комиссию, выплачиваем продавцу его часть.
    /// Неудача — возвращаем объявление обратно на продажу (без выплат).
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
            // M-6: выплата продавцу теперь под колбэком. Раньше она уходила без
            // него — если перевод не проходил (например, аккаунт продавца удалён),
            // деньги молча оставались на контракте и достать их было нечем.
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
            // M-1: раньше здесь объявление просто возвращалось на продажу, а деньги
            // покупателя (listing.price) оставались на балансе контракта — ни продавцу,
            // ни в fees_collected они не попадали, и достать их не мог никто:
            // withdraw_fees ограничен fees_collected. Покупатель оставался без NFT
            // и без денег. Теперь возвращаем ему полную стоимость лота.
            let refund = listing.price.0;
            env::log_str(&format!(
                "PURCHASE_FAILED | nft={} | buyer={} | refund={} yN | reason=nft_transfer_failed — relisting",
                token_id, buyer, refund
            ));
            self.listings.insert(token_id, listing);
            Some(Promise::new(buyer).transfer(NearToken::from_yoctonear(refund)))
        }
    }

    /// Колбэк после nft_transfer в cancel_listing.
    /// Неудача — восстанавливаем объявление (отмена не состоялась).
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

    /// M-6: Callback после выплаты продавцу. Откатывать продажу здесь нельзя —
    /// NFT уже у покупателя, а деньги с него списаны. Поэтому при неудаче
    /// зачисляем сумму в fees_collected: так она становится доступна владельцу
    /// через withdraw_fees и может быть передана продавцу вручную, вместо того
    /// чтобы навсегда зависнуть на балансе контракта.
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

    /// Завершающий колбэк батча покупки. Полезной логики не несёт: его
    /// единственная задача — замкнуть цепочку через .then(), чтобы из функции
    /// возвращался обычный промис, а не совместный (см. buy_multiple).
    /// Заодно даёт в логах отметку, что батч дошёл до конца.
    #[private]
    pub fn on_purchase_batch_done(&mut self, buyer: AccountId, items: u32) {
        env::log_str(&format!(
            "PURCHASE_BATCH_DONE | buyer={} | items={}",
            buyer, items
        ));
    }

    /// Аварийный возврат выставленного NFT продавцу.
    ///
    /// Как и в контракте стейкинга, получатель НЕ является параметром: NFT всегда
    /// уходит на `listing.seller`, записанный в состоянии. Владелец контракта может
    /// инициировать возврат, но не выбрать получателя — вектор кражи закрыт.
    /// Нужен на случай рассинхронизации состояния: без него застрявший в эскроу
    /// NFT вернуть было бы нечем.
    #[payable]
    pub fn admin_return_listing(&mut self, token_id: String) -> Promise {
        self.assert_owner_one_yocto();

        let listing = self
            .listings
            .get(&token_id)
            .unwrap_or_else(|| env::panic_str("admin_return_listing: listing not found"))
            .clone();

        // Убираем объявление ДО промиса, иначе повторный вызов в окне до колбэка
        // отправит второй nft_transfer.
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

    /// Callback аварийного возврата: при неудаче восстанавливаем объявление.
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

    /// Колбэк после withdraw_fees. Неудача — возвращаем сумму в баланс комиссий.
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

    // ── View methods ─────────────────────────────────────────────────────────────

    /// Постраничный список всех активных объявлений (для вкладки "Маркет").
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

    /// Одно объявление по token_id (для страницы конкретного NFT / проверки перед покупкой).
    pub fn get_listing(&self, token_id: String) -> Option<ListingView> {
        self.listings.get(&token_id).map(|listing| ListingView {
            token_id:  token_id.clone(),
            seller:    listing.seller.clone(),
            price:     listing.price,
            listed_at: listing.listed_at,
        })
    }

    /// Активные объявления конкретного продавца (вкладка "Мои объявления").
    /// M-7: метод отдавал ВСЕ объявления продавца одним куском, без ограничения.
    /// При росте числа лотов ответ однажды перестал бы помещаться в лимит газа
    /// view-вызова. Теперь обязательная пагинация, как в get_listings.
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

    // ── Внутреннее ────────────────────────────────────────────────────────────────

    fn assert_owner(&self) {
        require!(
            env::predecessor_account_id() == self.owner_id,
            "Access denied: caller is not the contract owner"
        );
    }

    /// M-3: усиленная проверка для вывода комиссии. Требование ровно 1 yoctoNEAR
    /// заставляет подписывать вызов full-access ключом: function-call ключи
    /// (выдаваемые ботам и фронтендам) депозит прикрепить не могут.
    fn assert_owner_one_yocto(&self) {
        require!(
            env::attached_deposit().as_yoctonear() == 1,
            "attach exactly 1 yoctoNEAR"
        );
        self.assert_owner();
    }
}
