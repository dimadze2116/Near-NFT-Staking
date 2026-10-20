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

// There is deliberately no key store and no signing client here: the bot only
// reads from the contracts. Everything that changes state is performed manually
// by an administrator through near-cli.

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

/** Current owner of a specific NFT - needed so that, before sending a sale
 * notification, the backend can confirm the buyer really does own that NFT now
 * (a guard against arbitrary forged calls to /api/notify-sale). */
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
 * Paginated list of claimed stakes with details (amount, dates, period) -
 * straight from the contract, in a single call.
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
    // Newest claims first
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
 * Converts an amount from NEAR to yoctoNEAR. Kept because the bot no longer
 * signs transactions itself but assembles a ready CLI command instead - and
 * converting to yocto by hand is far too easy to get wrong by an order of magnitude.
 */
export function nearToYocto(amountNear: string): string {
  const [whole, frac = ''] = amountNear.trim().split('.');
  const fracPadded = frac.padEnd(24, '0').substring(0, 24);
  const yocto = BigInt(whole) * 1_000_000_000_000_000_000_000_000n + BigInt(fracPadded);
  return yocto.toString();
}

/*
 * DELIBERATELY ABSENT: callContract / depositRewards / withdrawRewards.
 *
 * This module signs nothing, and that is a design decision rather than an omission.
 * The owner of both contracts is the contract account itself, and that account holds
 * a single full-access key. Such a key in a server environment variable would grant
 * not "top up the pool" but full control of the contract, up to and including
 * redeployment. assert_owner_one_yocto (the F-2 fix) is no defence here: it rejects
 * function-call keys, while a full-access key attaches a deposit freely.
 *
 * Top-ups and withdrawals are therefore performed manually through near-cli from the
 * administrator's machine, and the bot only assembles the command, converting the
 * amount to yoctoNEAR. Do not add signing to this module.
 */
