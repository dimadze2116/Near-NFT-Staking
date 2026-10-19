import * as nearAPI from 'near-api-js';
import { Near, verifyNep413Signature } from 'near-kit';
import { hex } from '@scure/base';

const NEAR_NETWORK = process.env.NEAR_NETWORK || 'mainnet';
const NFT_CONTRACT = process.env.NFT_CONTRACT || 'nearlegion.nfts.tg';
const STAKING_CONTRACT = process.env.STAKING_CONTRACT || 'legion-staking.near';

const RPC_URL = NEAR_NETWORK === 'mainnet'
  ? 'https://free.rpc.fastnear.com'
  : 'https://rpc.testnet.near.org';

const provider = new nearAPI.providers.JsonRpcProvider({ url: RPC_URL });
const nearKit = new Near({ network: NEAR_NETWORK as 'mainnet' | 'testnet' });

// Хранилища ключей и подписывающего клиента здесь намеренно нет: бот работает
// с контрактами только на чтение. Всё, что меняет состояние, выполняется
// администратором вручную через near-cli.

export { provider, STAKING_CONTRACT };

export interface NftToken {
  token_id: string;
  owner_id: string;
  metadata?: { title?: string; media?: string; };
}

export interface VerifyPayload {
  signedMessage: { accountId: string; publicKey: string; signature: string; };
  message: string;
  recipient: string;
  nonce: string;
}

export interface StakeView {
  stake_id: string;
  token_id: string;
  staked_at: number;
  period_days: number;
  unlocks_at: number;
  reward_near: string;
  is_mature: boolean;
  claimed: boolean;
}

export interface ClaimedStakeView {
  stake_id: string;
  owner: string;
  token_id: string;
  staked_at: number;
  period_days: number;
  unlocks_at: number;
  reward_near: string;
}

export async function checkNftOwnership(accountId: string): Promise<{ hasNft: boolean; tokens: NftToken[] }> {
  try {
    const result = await provider.query({
      request_type: 'call_function',
      account_id: NFT_CONTRACT,
      method_name: 'nft_tokens_for_owner',
      args_base64: Buffer.from(JSON.stringify({ account_id: accountId, from_index: '0', limit: 50 })).toString('base64'),
      finality: 'final',
    }) as any;
    const tokens: NftToken[] = JSON.parse(Buffer.from(result.result).toString());
    return { hasNft: tokens.length > 0, tokens };
  } catch (err) {
    console.error(`[NEAR] checkNftOwnership error for ${accountId}:`, err);
    return { hasNft: false, tokens: [] };
  }
}

/** Текущий владелец конкретного NFT — нужен, чтобы перед отправкой уведомления
 * о продаже убедиться, что покупатель реально теперь владеет этим NFT
 * (защита от произвольных фейковых вызовов /api/notify-sale). */
export async function getNftOwner(tokenId: string): Promise<string | null> {
  try {
    const result = await provider.query({
      request_type: 'call_function',
      account_id: NFT_CONTRACT,
      method_name: 'nft_token',
      args_base64: Buffer.from(JSON.stringify({ token_id: tokenId })).toString('base64'),
      finality: 'final',
    }) as any;
    const token = JSON.parse(Buffer.from(result.result).toString());
    return token?.owner_id ?? null;
  } catch (err) {
    console.error(`[NEAR] getNftOwner error for ${tokenId}:`, err);
    return null;
  }
}

export async function verifyWalletSignature(payload: VerifyPayload): Promise<string | null> {
  try {
    const { signedMessage, message, recipient, nonce } = payload;
    const isValid = await verifyNep413Signature(signedMessage, { message, recipient, nonce: hex.decode(nonce) }, { near: nearKit });
    if (!isValid) return null;
    return signedMessage.accountId;
  } catch (err) {
    console.error('[NEAR] verifyWalletSignature error:', err);
    return null;
  }
}

export async function getStakesForAccount(accountId: string): Promise<StakeView[]> {
  try {
    const result = await provider.query({
      request_type: 'call_function',
      account_id: STAKING_CONTRACT,
      method_name: 'get_stakes_for_account',
      args_base64: Buffer.from(JSON.stringify({ account_id: accountId })).toString('base64'),
      finality: 'final',
    }) as any;
    return JSON.parse(Buffer.from(result.result).toString());
  } catch {
    return [];
  }
}

export async function getRewardPool(): Promise<string> {
  try {
    const result = await provider.query({
      request_type: 'call_function',
      account_id: STAKING_CONTRACT,
      method_name: 'get_reward_pool',
      args_base64: 'e30=',
      finality: 'final',
    }) as any;
    return JSON.parse(Buffer.from(result.result).toString());
  } catch {
    return '0';
  }
}

export async function getClaimedStakesCount(): Promise<number> {
  try {
    const result = await provider.query({
      request_type: 'call_function',
      account_id: STAKING_CONTRACT,
      method_name: 'get_claimed_stakes_count',
      args_base64: 'e30=',
      finality: 'final',
    }) as any;
    return Number(JSON.parse(Buffer.from(result.result).toString()));
  } catch {
    return 0;
  }
}

export async function getUniqueClaimersCount(): Promise<number> {
  try {
    const result = await provider.query({
      request_type: 'call_function',
      account_id: STAKING_CONTRACT,
      method_name: 'get_unique_claimers_count',
      args_base64: 'e30=',
      finality: 'final',
    }) as any;
    return Number(JSON.parse(Buffer.from(result.result).toString()));
  } catch {
    return 0;
  }
}

/**
 * Постраничный список заклеймленных стейков с деталями (сумма, даты, период) —
 * напрямую из контракта, одним вызовом.
 */
export async function getClaimedStakes(fromIndex = 0, limit = 50): Promise<ClaimedStakeView[]> {
  try {
    const result = await provider.query({
      request_type: 'call_function',
      account_id: STAKING_CONTRACT,
      method_name: 'get_claimed_stakes',
      args_base64: Buffer.from(JSON.stringify({ from_index: fromIndex, limit })).toString('base64'),
      finality: 'final',
    }) as any;
    const claims: ClaimedStakeView[] = JSON.parse(Buffer.from(result.result).toString());
    // Новые клеймы — сверху
    claims.sort((a, b) => b.unlocks_at - a.unlocks_at);
    return claims;
  } catch {
    return [];
  }
}

export async function getAccountBalance(accountId: string = STAKING_CONTRACT): Promise<string> {
  try {
    const account = await provider.query({
      request_type: 'view_account',
      account_id: accountId,
      finality: 'final',
    }) as any;
    return account.amount || '0';
  } catch {
    return '0';
  }
}

/**
 * Перевод суммы из NEAR в yoctoNEAR. Оставлен, потому что бот теперь не
 * подписывает транзакции сам, а формирует готовую CLI-команду — и пересчёт
 * в yocto вручную слишком легко сделать с ошибкой на порядок.
 */
export function nearToYocto(amountNear: string): string {
  const [whole, frac = ''] = amountNear.trim().split('.');
  const fracPadded = frac.padEnd(24, '0').substring(0, 24);
  const yocto = BigInt(whole) * 1_000_000_000_000_000_000_000_000n + BigInt(fracPadded);
  return yocto.toString();
}

/*
 * УДАЛЕНО СОЗНАТЕЛЬНО: callContract / depositRewards / withdrawRewards.
 *
 * Эти функции подписывали транзакции ключом из ADMIN_NEAR_PRIVATE_KEY. Владелец
 * обоих контрактов — сами контрактные аккаунты, и на legion-staking.near
 * существует единственный ключ, полнодоступный. То есть переменная окружения на
 * сервере давала не «пополнить пул», а полный контроль над контрактом, вплоть
 * до передеплоя. Проверка assert_owner_one_yocto (фикс F-2) от этого не защищает
 * вовсе: она отсекает function-call ключи, а full-access депозит прикладывает
 * свободно.
 *
 * Теперь пополнение и вывод выполняются вручную через near-cli с машины
 * администратора, а бот только формирует готовую команду. ADMIN_NEAR_PRIVATE_KEY
 * и ADMIN_NEAR_ACCOUNT должны быть удалены из окружения на Render.
 */
