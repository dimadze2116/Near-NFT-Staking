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

/// Тиры APR (в basis points, 1% = 100 bps) по ПОЗИЦИИ NFT у стейкера
/// (его 1-й, 2-й, ... N-й застейканный NFT — не количество NFT в этой транзакции).
/// Подобраны так, чтобы бюджет 100 NEAR/месяц гарантированно не превышался
/// даже в худшем случае: один стейкер занимает весь потолок в MAX_ACTIVE_STAKES
/// NFT на максимальный срок (30 дней). См. change-apr-and-batch.md.
///
/// ВАЖНО (фикс бага): раньше формула умножала итоговую награду на ПОЗИЦИЮ NFT
/// (nft_count), из-за чего 50-й NFT стейкера считался так, будто застейкано
/// сразу 50 штук. Теперь позиция используется только чтобы выбрать тир —
/// сама награда за КАЖДЫЙ отдельный NFT берётся из TIER_APR_BPS[tier], без
/// домножения на позицию.
const TIER_APR_BPS: [u64; 6] = [
    2420, // позиция 1-10
    2904, // позиция 11-20
    3319, // позиция 21-30
    3733, // позиция 31-40
    4148, // позиция 41-50
    4205, // позиция 51+ (максимум, дальше не растёт)
];

/// Жёсткий потолок одновременно активных (незаклеймленных) NFT по всему контракту.
/// Обеспечивает математическую гарантию бюджета — см. change-apr-and-batch.md.
const MAX_ACTIVE_STAKES: u64 = 300;

/// Отсрочка, после которой стейк считается брошенным и владелец контракта может
/// вернуть NFT его владельцу принудительно (admin_return_nft). 90 дней сверх срока
/// разблокировки: заведомо больше самого длинного периода (30 дней), чтобы метод
/// нельзя было применить к живой позиции.
const ABANDONED_GRACE_SEC: u64 = 90 * 24 * 60 * 60;

const NFT_VALUE_YOCTO: u128 = 10 * 1_000_000_000_000_000_000_000_000u128;

const GAS_NFT_TRANSFER: Gas  = Gas::from_tgas(50);
const GAS_CALLBACK:     Gas  = Gas::from_tgas(10);
const GAS_REWARD_PAY:   Gas  = Gas::from_tgas(10);

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

/// Callback интерфейс нашего контракта
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

/// Отдельная view-структура для глобального списка заклеймленных стейков (админка):
/// в отличие от StakeView, содержит owner, т.к. выборка идёт не по одному аккаунту,
/// а по всему контракту сразу.
/// owner: String (не AccountId) — сохраняем этот выбор из прошлой версии, он точно
/// проверен и работает, менять нет смысла без явной необходимости.
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

/// Запись в топе лидерборда — ранжирование по количеству активных (незаклеймленных) NFT.
#[near(serializers = [json])]
pub struct LeaderboardEntry {
    pub owner:        String,
    pub nft_count:     u32,
    pub total_reward: U128,
}

/// Позиция конкретного аккаунта в общем рейтинге (для карточки "ваша позиция").
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

    // ── Init ──────────────────────────────────────────────────────────────────

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

    // ── Owner ─────────────────────────────────────────────────────────────────

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

    /// Аварийный возврат NFT по брошенному стейку.
    ///
    /// Две сознательные меры против злоупотребления правами владельца контракта:
    ///
    /// 1. Получатель НЕ является параметром. NFT всегда уходит на `stake.owner`,
    ///    записанный в состоянии. Владелец контракта может инициировать возврат,
    ///    но не может выбрать, кому. Это убирает вектор кражи, который есть у
    ///    привычной формы `admin_return_asset(id, to)`.
    /// 2. Метод работает только по стейкам, у которых с момента разблокировки
    ///    прошло больше ABANDONED_GRACE_SEC. По активным и недавно созревшим
    ///    позициям он недоступен — забрать чужой рабочий стейк нельзя.
    ///
    /// Зарезервированная награда возвращается в пул (закрывает F-6: раньше резерв
    /// по брошенному стейку зависал вне пула навсегда).
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

        // Помечаем ДО промиса — иначе повторный вызов в окне до колбэка пройдёт
        // проверки заново и отправит второй nft_transfer.
        let mut updated = stake.clone();
        updated.claimed = true;
        self.stakes.insert(stake_id.clone(), updated);

        // Резерв возвращаем в пул: владелец за наградой не пришёл.
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

    /// Вывод с callback — если Promise упадёт, reward_pool восстанавливается
    #[payable]
    pub fn withdraw_rewards(&mut self, amount: U128) -> Promise {
        self.assert_owner_one_yocto();
        let yocto = amount.0;
        require!(yocto > 0, "Amount must be greater than zero");
        require!(
            self.reward_pool >= yocto,
            "Requested amount exceeds reward pool balance"
        );
        // FIX 3: вычитаем ПОСЛЕ callback подтверждения
        // Временно резервируем — вычтем в on_withdraw_complete
        env::log_str(&format!(
            "WITHDRAW_INIT | amount={} yN | pool_before={} yN",
            yocto, self.reward_pool
        ));
        self.reward_pool -= yocto; // вычитаем сразу (защита от double-spend)

        Promise::new(self.owner_id.clone())
            .transfer(NearToken::from_yoctonear(yocto))
            .then(
                ext_self::ext(env::current_account_id())
                    .with_static_gas(GAS_CALLBACK)
                    .on_withdraw_complete(amount)
            )
    }

    /// F-1 (было clear_all_stakes): прежний метод очищал ВСЕ стейки разом и обнулял
    /// reward_pool. Поскольку NFT физически лежат на балансе контракта, а единственный
    /// путь их вернуть — unstake, который требует наличия записи, один вызов запирал
    /// все застейканные NFT пользователей навсегда. Аварийного вывода в контракте нет.
    /// Метод удалён и заменён на безопасный аналог ниже.
    ///
    /// Удаляет ТОЛЬКО заклеймленные записи — по ним NFT уже возвращены владельцам,
    /// поэтому потерять нечего. Активные стейки и reward_pool не трогаются.
    /// F-3: заклеймленные записи раньше копились в self.stakes без предела и удорожали
    /// каждый новый стейк (полный обход карты в nft_on_transfer). Этот метод даёт
    /// способ их подчищать без миграции состояния.
    /// limit ограничивает объём за вызов — метод рассчитан на многократный запуск.
    #[payable]
    pub fn prune_claimed_stakes(&mut self, limit: u32) -> u32 {
        self.assert_owner_one_yocto();
        require!(limit > 0 && limit <= 100, "prune: limit must be between 1 and 100");

        // Сначала собираем, потом удаляем — нельзя менять карту во время обхода.
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
            // Индекс аккаунта тоже чистим, иначе набор растёт вечно и удорожает
            // active_stake_count для этого пользователя.
            if let Some(ids) = self.stakes_by_account.get_mut(&owner) {
                ids.remove(&stake_id);
            }
        }

        env::log_str(&format!("PRUNE | removed {} claimed stakes", count));
        count
    }

    // ── NFT Receiver ──────────────────────────────────────────────────────────

    pub fn nft_on_transfer(
        &mut self,
        #[allow(unused_variables)]
        sender_id: AccountId,
        previous_owner_id: AccountId,
        token_id: String,
        msg: String,
    ) -> PromiseOrValue<bool> {
        // F-7: владельцем стейка обязан быть ПРЕЖНИЙ ВЛАДЕЛЕЦ NFT, а не тот, кто
        // вызвал nft_transfer_call. По NEP-171 это разные аккаунты: при использовании
        // апрувов (NEP-178) одобренный аккаунт может перенести чужой NFT, и раньше
        // он же становился владельцем стейка — то есть забирал при анстейке чужой
        // NFT вместе с наградой. В nft-marketplace этот выбор изначально сделан верно.
        let owner_id = previous_owner_id;
        let nft_contract: AccountId = NFT_CONTRACT_ID
            .parse()
            .expect("Invalid NFT contract ID in code");
        require!(
            env::predecessor_account_id() == nft_contract,
            "nft_on_transfer: caller is not the authorized NFT contract"
        );

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

        // Потолок активных NFT по всему контракту — гарантирует бюджет пула
        // математически, независимо от того, как распределятся сроки/стейкеры.
        let total_active = self.stakes.iter().filter(|(_, s)| !s.claimed).count() as u64;
        if total_active >= MAX_ACTIVE_STAKES {
            env::log_str(&format!(
                "STAKE_REJECTED | reason=pool_full | active={} | max={} | nft={} | owner={}",
                total_active, MAX_ACTIVE_STAKES, token_id, owner_id
            ));
            return PromiseOrValue::Value(true);
        }

        let active_count = self.active_stake_count(&owner_id);
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

        env::log_str(&format!(
            "STAKE_CREATED | nft={} | owner={} | period_days={} | reward={} yN | unlocks_at={}",
            token_id, owner_id, period_days, reward,
            now_sec + Self::period_sec(period_days)
        ));

        PromiseOrValue::Value(false)
    }

    // ── Unstake с callback ────────────────────────────────────────────────────

    /// FIX 1: unstake теперь использует callback on_unstake_complete
    /// Если nft_transfer упадёт — стейк восстанавливается, пользователь не теряет NFT/награду
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

        // Помечаем как claimed ПЕРЕД промисом (защита от reentrancy)
        let mut updated = stake.clone();
        updated.claimed = true;
        self.stakes.insert(stake_id.clone(), updated);

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
                    // 30 TGas вместо прежних 20: внутри этого колбэка теперь создаётся
                    // ещё один промис (перевод награды) со своим колбэком на GAS_REWARD_PAY.
                    .with_static_gas(Gas::from_tgas(30))
                    .on_unstake_complete(caller, token_id, stake_id, reward)
            )
    }

    // ── Callbacks (вызываются только самим контрактом) ────────────────────────

    /// Callback после nft_transfer в unstake.
    /// Если transfer успешен — выплачиваем награду.
    /// Если transfer упал — откатываем claimed = false, возвращаем награду в пул.
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
            // NFT успешно возвращён — выплачиваем NEAR награду.
            // F-4: перевод идёт под колбэком. Раньше он уходил «в никуда»: если
            // перевод не проходил (например, аккаунт получателя удалён), награда
            // терялась молча, а reward_pool был уменьшен ещё на этапе стейка.
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
            // NFT transfer упал — откатываем стейк
            env::log_str(&format!(
                "UNSTAKE_FAILED | nft={} | owner={} | reason=nft_transfer_failed — rolling back",
                token_id, caller
            ));
            // Восстанавливаем стейк
            if let Some(mut stake) = self.stakes.get(&stake_id).cloned() {
                stake.claimed = false;
                self.stakes.insert(stake_id, stake);
            }
            // Возвращаем награду в пул
            self.reward_pool += reward.0;
            None
        }
    }

    /// Callback аварийного возврата. При неудаче откатываем всё, что списали
    /// на пути туда: снимаем пометку claimed и убираем награду обратно из пула.
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
            env::log_str(&format!(
                "ADMIN_RETURN_FAILED | nft={} | owner={} — stake restored",
                stake.token_id, stake.owner
            ));
        }
    }

    /// F-4: Callback после перевода награды в on_unstake_complete.
    /// NFT к этому моменту уже у пользователя, поэтому откатывать стейк НЕЛЬЗЯ:
    /// сброс claimed = false создал бы запись, выглядящую активной, при том что
    /// самого NFT на контракте больше нет — и её нельзя было бы ни заклеймить,
    /// ни удалить (тот же класс проблемы, что фантомные объявления в маркете).
    /// Поэтому при неудаче возвращаем только сами средства в пул и явно логируем
    /// событие, чтобы владелец мог разобрать случай вручную.
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

    /// Callback после withdraw_rewards.
    /// Если transfer упал — восстанавливаем reward_pool.
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
            // Возвращаем средства в пул
            self.reward_pool += amount.0;
            env::log_str(&format!(
                "WITHDRAW_FAILED | amount={} yN | pool_restored={} yN",
                amount.0, self.reward_pool
            ));
        }
    }

    // ── View Methods ──────────────────────────────────────────────────────────

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
        U128(self.stakes.iter().filter(|(_, s)| !s.claimed).count() as u128)
    }

    pub fn get_matured_stakes_count(&self) -> U128 {
        let now = env::block_timestamp() / 1_000_000_000;
        U128(self.stakes.iter().filter(|(_, s)| {
            !s.claimed && now >= s.staked_at + Self::period_sec_lenient(s.period_days)
        }).count() as u128)
    }

    pub fn get_total_staked(&self) -> U128 {
        U128(self.stakes.iter().filter(|(_, s)| !s.claimed).count() as u128)
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

    /// Кол-во заклеймленных (уже выплаченных) стейков по всему контракту —
    /// в отличие от get_stakes_for_account, не требует знать account_id заранее.
    pub fn get_claimed_stakes_count(&self) -> U128 {
        U128(self.stakes.iter().filter(|(_, s)| s.claimed).count() as u128)
    }

    /// Список ключей заклеймленных стейков в формате "owner:token_id" (это тот же формат,
    /// что и внутренний stake_id — см. как формируется stake_id в fn stake()).
    /// Лёгкий вариант без деталей — оставлен как самостоятельный простой метод,
    /// полную информацию по каждому стейку см. в get_claimed_stakes() ниже.
    pub fn get_claimed_stake_ids(&self) -> Vec<String> {
        let mut result: Vec<String> = Vec::new();
        for (id, s) in self.stakes.iter() {
            if s.claimed {
                result.push(id.clone());
            }
        }
        result
    }

    /// Уникальные аккаунты, которые уже что-то заклеймили.
    pub fn get_unique_claimers_count(&self) -> U128 {
        let unique = self.stakes.iter()
            .filter(|(_, s)| s.claimed)
            .map(|(_, s)| s.owner.clone())
            .collect::<std::collections::HashSet<_>>()
            .len() as u128;
        U128(unique)
    }

    /// Постраничный список заклеймленных стейков с полными деталями (сумма, даты, период).
    /// from_index/limit — пагинация. Ранее считалось, что этот вариант (структура +
    /// пагинация) стабильно падает в WasmTrap на этой связке near-sdk/toolchain — но
    /// впоследствии выяснилось, что причиной был баг самого near-cli-rs при передаче
    /// непустых аргументов через as-read-only (не контракта). Проверено напрямую через
    /// RPC (в обход near-cli-rs) — работает корректно.
    ///
    /// Не паникуем на нестандартном period_days (легаси/битые записи) — для них
    /// unlocks_at просто равен staked_at, вместо падения всей выборки.
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

    /// Общая приватная агрегация для лидерборда: (owner, кол-во активных NFT, сумма наград).
    /// stakes_by_account — LookupMap, не поддерживает перечисление всех ключей, поэтому
    /// считаем через self.stakes.iter() (IterableMap, уже используется для аналогичных
    /// агрегаций в get_unique_claimers_count/get_claimed_stakes_count).
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
        // Вторичный критерий (owner) — обязателен для детерминизма: без него при
        // равном nft_count порядок зависел бы от обхода HashMap, который меняется
        // между отдельными вызовами (get_leaderboard vs get_my_rank могли бы
        // расходиться в позиции для аккаунтов с одинаковым количеством NFT).
        list.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        list
    }

    /// Топ стейкеров по количеству активных (незаклеймленных) NFT, по убыванию.
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

    /// Позиция конкретного аккаунта в общем рейтинге (None, если у него нет активных NFT).
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

    /// Предпросмотр награды для СТЕЙКА НЕСКОЛЬКИХ NFT ЗА РАЗ (батч).
    /// current_active_count — сколько NFT уже активно застейкано у этого аккаунта
    /// (позиции считаются от current_active_count+1 до current_active_count+add_count).
    /// add_count — сколько NFT сейчас добавляется.
    /// Возвращает СУММУ наград по всем добавляемым NFT, корректно учитывая переход
    /// через границы тиров (например, если у стейкера уже 8 NFT и он добавляет 5 —
    /// 2 из них попадут в тир 1-10, а 3 — в тир 11-20).
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

    /// APR (bps) для конкретной позиции NFT у стейкера (1-й, 2-й, ... N-й).
    /// Одинаков для всех периодов (7/14/30 дней) — награда масштабируется по
    /// сроку внутри calc_reward, а не через отдельный APR на период.
    pub fn get_apr_bps(&self, position: u32) -> u64 {
        require!(position > 0, "get_apr_bps: position must be > 0");
        Self::apr_bps_for_position(position)
    }

    // ── Internal ──────────────────────────────────────────────────────────────

    fn assert_owner(&self) {
        require!(
            env::predecessor_account_id() == self.owner_id,
            "Access denied: caller is not the contract owner"
        );
    }

    /// F-2: усиленная проверка для методов, распоряжающихся средствами и состоянием.
    /// Требование ровно 1 yoctoNEAR заставляет подписывать вызов full-access ключом:
    /// function-call ключи (те, что выдаются фронтендам и ботам) прикрепить депозит
    /// не могут. На момент правки на контрактном аккаунте только full-access ключи,
    /// то есть активного вектора нет — это защита на будущее, если такой ключ появится.
    /// Намеренно НЕ применяется к deposit_rewards: он принимает произвольный депозит.
    fn assert_owner_one_yocto(&self) {
        require!(
            env::attached_deposit().as_yoctonear() == 1,
            "attach exactly 1 yoctoNEAR"
        );
        self.assert_owner();
    }

    /// Строгая версия — только для write-путей, где period_days уже провалидирован
    /// (принимаются исключительно 7/14/30). Паника здесь означает баг в коде, а не
    /// плохие данные.
    fn period_sec(days: u8) -> u64 {
        match days {
            7  => PERIOD_7_DAYS,
            14 => PERIOD_14_DAYS,
            30 => PERIOD_30_DAYS,
            _  => env::panic_str("period_sec: invalid period — must be 7, 14, or 30"),
        }
    }

    /// F-5: мягкая версия для чтения и для unstake.
    /// Прежде period_sec паниковал везде, и одна запись с нестандартным period_days
    /// (легаси из ранних версий) роняла get_stake, get_stakes_for_account,
    /// get_matured_stakes_count и get_contract_stats сразу для ВСЕХ пользователей.
    /// Хуже того, тот же вызов стоял в unstake — то есть владелец такой записи
    /// вообще не мог забрать свой NFT. Возврат 0 означает «срок уже истёк»:
    /// для битой записи это самый безопасный исход — пользователь получает NFT назад.
    fn period_sec_lenient(days: u8) -> u64 {
        match days {
            7  => PERIOD_7_DAYS,
            14 => PERIOD_14_DAYS,
            30 => PERIOD_30_DAYS,
            _  => 0,
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

    /// Возвращает тир APR (bps) по позиции NFT у стейкера (1-й, 2-й, ... N-й).
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

    /// Награда за ОДИН конкретный NFT на данной позиции у стейкера.
    /// ФИКС БАГА: раньше здесь домножали на nft_count (позицию), из-за чего
    /// награда за один NFT росла пропорционально порядковому номеру вместо
    /// того, чтобы просто определять тир ставки. Теперь множитель — всегда 1
    /// (это же один-единственный NFT), тир только выбирает APR.
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
