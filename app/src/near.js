// near.js — вся работа с блокчейном NEAR: подключение кошелька, чтение данных
// с контракта, отправка стейкинга. Портировано из рабочего app.html (та же
// логика, что уже проверена на mainnet), адаптировано под React.

export const NFT_CONTRACT = "nearlegion.nfts.tg";
export const STAKING_CONTRACT = "legion-staking.near";
export const MARKETPLACE_CONTRACT = "legion-marketplace.near";
// Бэкенд бота — другой домен (Render), это приложение хостится отдельно (Netlify).
export const BOT_API = "https://nft-staking-bot.onrender.com";
const RPC_URL = "https://free.rpc.fastnear.com";

// Максимум NFT за одну транзакцию (протокольный лимит NEAR — 300 Tgas на транзакцию).
// См. change-apr-and-batch.md — значение консервативное, стоит уточнить вживую.
export const MAX_BATCH_SIZE = 4;
const GAS_PER_STAKE_ACTION = "70000000000000"; // 70 Tgas

export async function rpcView(contract, method, args) {
  const res = await fetch(RPC_URL, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({
      jsonrpc: "2.0",
      id: "dontcare",
      method: "query",
      params: {
        request_type: "call_function",
        account_id: contract,
        method_name: method,
        args_base64: btoa(JSON.stringify(args || {})),
        finality: "final",
      },
    }),
  });
  const json = await res.json();
  if (json.error) throw new Error(json.error.message || "RPC error");
  const out = new TextDecoder().decode(new Uint8Array(json.result.result));
  // Большие числа (yoctoNEAR) оборачиваем в строки, чтобы не терять точность в JSON.parse
  const safe = out.replace(/:(\s*)(\d{15,})/g, ':"$2"');
  try {
    return JSON.parse(safe);
  } catch {
    return out;
  }
}

export function yoctoToNear(v) {
  try {
    const n = BigInt(String(v || "0"));
    const d = 1000000000000000000000000n;
    const whole = n / d;
    const frac = (n % d).toString().padStart(24, "0").substring(0, 4);
    return `${whole}.${frac}`;
  } catch {
    return "0";
  }
}

/**
 * Конвертация введённой пользователем цены ("1.5", "0.001", "3") в строку
 * yoctoNEAR для передачи в контракт. Возвращает null при невалидном вводе
 * (не число, отрицательное/нулевое значение, больше 24 знаков после запятой).
 */
export function nearToYocto(input) {
  const s = String(input ?? "").trim();
  if (!/^\d+(\.\d+)?$/.test(s)) return null;
  const [wholeStr, fracStr = ""] = s.split(".");
  if (fracStr.length > 24) return null;
  const fracPadded = fracStr.padEnd(24, "0");
  const yocto = BigInt(wholeStr || "0") * 1000000000000000000000000n + BigInt(fracPadded || "0");
  if (yocto <= 0n) return null;
  return yocto.toString();
}

/**
 * Подключение кошелька через @hot-labs/near-connect (UI выбора кошелька) +
 * near-kit (обёртка для подписи транзакций). Возвращает Promise, который
 * резолвится объектом { near, connector, accountId } при успешном подключении.
 */
export function connectWallet() {
  return new Promise((resolve, reject) => {
    (async () => {
      try {
        // Versions are pinned deliberately. These imports resolve at runtime in
        // the browser, so an unpinned specifier meant every user loaded whatever
        // had been published by then - a breaking release in either library
        // would take wallet connection down, and with it every flow in the app.
        //
        // These are the versions the app was last verified on. Raising them is
        // a deliberate act: bump, open the bot, connect a wallet, sign one
        // transaction.
        //
        // Note this pins the libraries, not the dependency on esm.sh itself.
        // Bundling them instead would not remove that dependency either -
        // near-connect fetches its wallet manifest from cdn.jsdelivr.net at
        // runtime regardless.
        const { NearConnector } = await import("https://esm.sh/@hot-labs/near-connect@0.11.4");
        const { Near, fromHotConnect } = await import("https://esm.sh/near-kit@0.20.2");
        const connector = new NearConnector({ network: "mainnet" });
        connector.on("wallet:signIn", async (data) => {
          const near = new Near({
            network: "mainnet",
            wallet: fromHotConnect(connector),
            rpc: { url: RPC_URL },
          });
          const accountId = data.accounts ? data.accounts[0].accountId : data.accountId;
          resolve({ near, connector, accountId });
        });
        connector.connect();
      } catch (e) {
        reject(e);
      }
    })();
  });
}

export async function getNftsForOwner(accountId) {
  const result = await rpcView(NFT_CONTRACT, "nft_tokens_for_owner", {
    account_id: accountId,
    from_index: "0",
    limit: 50,
  });
  const list = Array.isArray(result) ? result : [];
  return list.map((n) => ({
    id: n.token_id,
    name: n.metadata?.title || `NFT #${n.token_id}`,
    tokenId: `#${n.token_id}`,
    image: n.metadata?.media || null,
  }));
}

/** Метаданные одного NFT по token_id — нужно для застейканных NFT (они уже не
 * принадлежат кошельку пользователя, nft_tokens_for_owner их не вернёт). */
export async function getNftMetadata(tokenId) {
  try {
    const t = await rpcView(NFT_CONTRACT, "nft_token", { token_id: tokenId });
    return t?.metadata?.media || null;
  } catch {
    return null;
  }
}

/** Имя + картинка одного NFT по token_id — нужно для карточек в маркете
 * (в отличие от getNftMetadata, возвращает оба поля, не только картинку). */
export async function getNftInfo(tokenId) {
  try {
    const t = await rpcView(NFT_CONTRACT, "nft_token", { token_id: tokenId });
    return {
      name: t?.metadata?.title || `NFT #${tokenId}`,
      image: t?.metadata?.media || null,
    };
  } catch {
    return { name: `NFT #${tokenId}`, image: null };
  }
}

export async function getStakesForAccount(accountId) {
  const result = await rpcView(STAKING_CONTRACT, "get_stakes_for_account", { account_id: accountId });
  return Array.isArray(result) ? result : [];
}

export async function getRewardPool() {
  return rpcView(STAKING_CONTRACT, "get_reward_pool", {});
}

export async function previewReward(periodDays, currentActiveCount, addCount) {
  return rpcView(STAKING_CONTRACT, "preview_reward", {
    period_days: periodDays,
    current_active_count: currentActiveCount,
    add_count: addCount,
  });
}

export async function getLeaderboard(limit = 20) {
  const result = await rpcView(STAKING_CONTRACT, "get_leaderboard", { limit });
  return Array.isArray(result) ? result : [];
}

export async function getMyRank(accountId) {
  return rpcView(STAKING_CONTRACT, "get_my_rank", { account_id: accountId });
}

/** Анстейк/клейм одного NFT — требует ровно 1 yoctoNEAR вложения (проверка владельца в контракте). */
export async function unstakeNft(near, tokenId) {
  return near.call(STAKING_CONTRACT, "unstake", { token_id: tokenId }, { attachedDeposit: 1n, gas: "200000000000000" });
}

/**
 * Стейкинг нескольких NFT. Разбивает на чанки по MAX_BATCH_SIZE — одна
 * транзакция (одна подпись кошелька) на чанк, чтобы уложиться в лимит газа.
 * near — объект near-kit из connectWallet().
 */
export async function stakeNfts(near, accountId, tokenIds, periodDays) {
  const chunks = [];
  for (let i = 0; i < tokenIds.length; i += MAX_BATCH_SIZE) {
    chunks.push(tokenIds.slice(i, i + MAX_BATCH_SIZE));
  }
  for (const chunk of chunks) {
    let tx = near.transaction(accountId);
    for (const id of chunk) {
      tx = tx.functionCall(
        NFT_CONTRACT,
        "nft_transfer_call",
        { receiver_id: STAKING_CONTRACT, token_id: id, msg: String(periodDays) },
        { attachedDeposit: 1n, gas: GAS_PER_STAKE_ACTION }
      );
    }
    await tx.send();
  }
}

/** Проверка прав админа через Telegram initData (тот же /api/is-admin, что и в app.html). */
export async function checkIsAdmin() {
  const initData = window.Telegram?.WebApp?.initData;
  if (!initData) return false;
  try {
    const res = await fetch(`${BOT_API}/api/is-admin`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ initData }),
    });
    const data = await res.json();
    return !!data?.isAdmin;
  } catch {
    return false;
  }
}

export async function getClaimedStakesCount() {
  return rpcView(STAKING_CONTRACT, "get_claimed_stakes_count", {});
}

export async function getUniqueClaimersCount() {
  return rpcView(STAKING_CONTRACT, "get_unique_claimers_count", {});
}

export async function getClaimedStakes(fromIndex = 0, limit = 50) {
  const result = await rpcView(STAKING_CONTRACT, "get_claimed_stakes", { from_index: fromIndex, limit });
  return Array.isArray(result) ? result : [];
}

/**
 * Верификация владения NFT/стейком для доступа в приватный канал — тот же
 * вызов, что и в старом app.html. Дёргается один раз сразу после подключения
 * кошелька. Возвращает { ok: boolean, reason?: string }.
 */
export async function verifyWebapp(accountId) {
  const initData = window.Telegram?.WebApp?.initData;
  if (!initData) return { ok: false, reason: "no_telegram" };
  try {
    const res = await fetch(`${BOT_API}/api/verify-webapp`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ initData, nearAccount: accountId }),
    });
    const data = await res.json().catch(() => ({}));
    if (!res.ok) return { ok: false, reason: data?.reason || "verify_failed" };
    return { ok: true, data };
  } catch (e) {
    return { ok: false, reason: "network_error", message: e.message };
  }
}

// ─── Маркетплейс NFT ─────────────────────────────────────────────────────────

const GAS_PER_LISTING_ACTION = "70000000000000"; // 70 Tgas — тот же порядок, что и стейкинг
const GAS_PER_PURCHASE_ITEM = 100000000000000n; // 100 Tgas — подтверждено тестами на mainnet
const MAX_GAS_PER_TX = 300000000000000n; // жёсткий лимит газа NEAR на транзакцию

/**
 * Выставить один NFT на продажу — nft_transfer_call с ценой (в yoctoNEAR) в msg.
 * Контракт маркетплейса сам решает принять (создать объявление) или вернуть NFT
 * (невалидная цена/уже выставлен) — это отражается в его логах, не в возврате
 * этого вызова напрямую (стандартный паттерн nft_transfer_call).
 */
export async function listNftForSale(near, accountId, tokenId, priceYocto) {
  return near
    .transaction(accountId)
    .functionCall(
      NFT_CONTRACT,
      "nft_transfer_call",
      { receiver_id: MARKETPLACE_CONTRACT, token_id: tokenId, msg: priceYocto },
      { attachedDeposit: 1n, gas: GAS_PER_LISTING_ACTION }
    )
    .send();
}

/** Отменить своё объявление — контракт вернёт NFT обратно на этот же кошелёк.
 *  Требует ровно 1 yoctoNEAR (M-5): метод перемещает актив, поэтому по NEP-171
 *  должен подписываться full-access ключом. */
export async function cancelListing(near, tokenId) {
  return near.call(MARKETPLACE_CONTRACT, "cancel_listing", { token_id: tokenId }, { attachedDeposit: 1n, gas: "100000000000000" });
}

/** Постраничный список всех активных объявлений (вкладка "Маркет"). */
export async function getMarketListings(fromIndex = 0, limit = 50) {
  const result = await rpcView(MARKETPLACE_CONTRACT, "get_listings", { from_index: fromIndex, limit });
  return Array.isArray(result) ? result : [];
}

/**
 * Покупка одного или нескольких NFT одним вызовом ("корзина"). totalPriceYocto —
 * точная сумма цен всех token_ids (строка yoctoNEAR), прикладывается как депозит.
 */
export async function buyListings(near, accountId, tokenIds, totalPriceYocto) {
  let gas = GAS_PER_PURCHASE_ITEM * BigInt(tokenIds.length);
  if (gas > MAX_GAS_PER_TX) gas = MAX_GAS_PER_TX;
  return near
    .transaction(accountId)
    .functionCall(
      MARKETPLACE_CONTRACT,
      "buy_multiple",
      { token_ids: tokenIds },
      { attachedDeposit: BigInt(totalPriceYocto), gas: gas.toString() }
    )
    .send();
}

/**
 * Уведомляет бэкенд о состоявшейся покупке, чтобы бот отправил продавцам
 * сообщение о продаже. Best-effort — ошибка здесь не должна ломать сам факт
 * успешной покупки (она уже подтверждена в блокчейне к этому моменту).
 * items — те же объекты объявлений (token_id/seller/price), что были в корзине.
 */
export async function notifySale(accountId, items) {
  const initData = window.Telegram?.WebApp?.initData;
  if (!initData) return;
  try {
    await fetch(`${BOT_API}/api/notify-sale`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        initData,
        nearAccount: accountId,
        items: items.map((item) => ({
          tokenId: item.token_id,
          sellerAccount: item.seller,
          price: yoctoToNear(item.price),
        })),
      }),
    });
  } catch (e) {
    console.error("[market] notify-sale failed", e);
  }
}
