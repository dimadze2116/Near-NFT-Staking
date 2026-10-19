import path from 'path';
import dotenv from 'dotenv';
dotenv.config({ path: path.join(__dirname, '..', '.env') });

import express from 'express';
import crypto from 'crypto';
import { bot, grantAccess } from './bot';
import { verifyWalletSignature, checkNftOwnership, getStakesForAccount, getNftOwner } from './near';
import {
  initDb,
  saveNonce,
  getNonceOwner,
  deleteNonce,
  cleanOldNonces,
  setUserNear,
  setUserNft,
  getUser,
  getUserByNearAccount,
  setUserInChannel,
} from './db';
import { startCron } from './cron';

const PORT        = parseInt(process.env.PORT || '3000');
const WEBHOOK_URL = process.env.WEBHOOK_URL;
const BOT_TOKEN   = process.env.BOT_TOKEN!;
const CHANNEL_ID  = process.env.CHANNEL_ID!;
const STAKING_CONTRACT = process.env.STAKING_CONTRACT || 'legion-staking.near';
const RESTART_SECRET   = process.env.RESTART_SECRET || '';
const API_SECRET       = process.env.API_SECRET || '';  // новый ключ для внешних запросов
const ADMIN_TELEGRAM_ID = parseInt(process.env.ADMIN_TELEGRAM_ID || '0');

if (!WEBHOOK_URL) throw new Error('WEBHOOK_URL is missing');
if (!BOT_TOKEN)   throw new Error('BOT_TOKEN is missing');

function hasActiveStakes(stakes: any[]): boolean {
  return Array.isArray(stakes) && stakes.length > 0;
}

const app = express();
app.use((req, res, next) => {
  res.setHeader('Access-Control-Allow-Origin', '*');
  res.setHeader('Access-Control-Allow-Methods', 'GET, POST, OPTIONS');
  res.setHeader('Access-Control-Allow-Headers', 'Content-Type, x-telegram-init-data, x-api-key');
  if (req.method === 'OPTIONS') return res.sendStatus(204);
  next();
});
app.use(express.json());
app.use((req, res, next) => {
  res.setHeader('ngrok-skip-browser-warning', 'true');
  next();
});
// newapp.html не содержит хеша в имени (в отличие от /assets/*.js|css) — без
// явного no-cache браузер/вебвью Telegram может отдавать старую закэшированную
// версию даже после нового деплоя.
app.get('/newapp.html', (req, res) => {
  res.setHeader('Cache-Control', 'no-cache, no-store, must-revalidate');
  res.sendFile(path.join(__dirname, '..', 'public', 'newapp.html'));
});
app.use(express.static(path.join(__dirname, '..', 'public')));

// ── Rate limiting (простой in-memory) ────────────────────────────────────────
const rateLimitMap = new Map<string, { count: number; resetAt: number }>();

function rateLimit(ip: string, max = 30, windowMs = 60_000): boolean {
  const now = Date.now();
  const entry = rateLimitMap.get(ip);
  if (!entry || now > entry.resetAt) {
    rateLimitMap.set(ip, { count: 1, resetAt: now + windowMs });
    return true; // OK
  }
  entry.count++;
  if (entry.count > max) return false; // превышен лимит
  return true;
}

function getIp(req: express.Request): string {
  return (req.headers['x-forwarded-for'] as string)?.split(',')[0] || req.ip || 'unknown';
}

// ── Middleware: Rate limit ────────────────────────────────────────────────────
const rateLimitMiddleware = (req: express.Request, res: express.Response, next: express.NextFunction) => {
  const ip = getIp(req);
  if (!rateLimit(ip)) {
    return res.status(429).json({ error: 'Too many requests' });
  }
  next();
};

// ── Middleware: Проверка Telegram initData ────────────────────────────────────
function validateTelegramInitData(initData: string): { valid: boolean; userId?: number } {
  try {
    const params = new URLSearchParams(initData);
    const hash = params.get('hash');
    if (!hash) return { valid: false };
    params.delete('hash');

    const sorted = [...params.entries()]
      .sort(([a], [b]) => a.localeCompare(b))
      .map(([k, v]) => `${k}=${v}`)
      .join('\n');

    const secretKey = crypto.createHmac('sha256', 'WebAppData').update(BOT_TOKEN).digest();
    const expected  = crypto.createHmac('sha256', secretKey).update(sorted).digest('hex');

    if (expected !== hash) return { valid: false };

    const authDate = parseInt(params.get('auth_date') || '0');
    if (Date.now() / 1000 - authDate > 3600) return { valid: false };

    const userJson = params.get('user');
    const userId = userJson ? JSON.parse(userJson).id : undefined;
    return { valid: true, userId };
  } catch {
    return { valid: false };
  }
}

const requireTelegramAuth = (req: express.Request, res: express.Response, next: express.NextFunction) => {
  const initData = req.body?.initData || req.headers['x-telegram-init-data'] as string;
  if (!initData) return res.status(401).json({ error: 'Missing Telegram auth' });
  const { valid } = validateTelegramInitData(initData);
  if (!valid) return res.status(401).json({ error: 'Invalid Telegram auth' });
  next();
};

const requireApiKey = (req: express.Request, res: express.Response, next: express.NextFunction) => {
  if (!API_SECRET) return next();
  const key = req.headers['x-api-key'] || req.query.api_key;
  if (key !== API_SECRET) return res.status(401).json({ error: 'Invalid API key' });
  next();
};

app.post(`/webhook/${BOT_TOKEN}`, (req, res) => {
  bot.handleUpdate(req.body, res);
});

app.post('/admin/restart', (req, res) => {
  const headerSecret = req.header('x-restart-secret') || '';
  if (RESTART_SECRET && headerSecret !== RESTART_SECRET) {
    return res.status(403).json({ error: 'forbidden' });
  }
  res.json({ ok: true });
  setTimeout(() => process.exit(0), 300);
});

app.post('/api/nonce', rateLimitMiddleware, (req, res) => {
  const { telegram_id, nonce } = req.body;
  if (!telegram_id || !nonce) return res.status(400).json({ error: 'telegram_id and nonce required' });
  cleanOldNonces();
  saveNonce(nonce, parseInt(telegram_id));
  res.json({ ok: true });
});

app.post('/api/verify', rateLimitMiddleware, async (req, res) => {
  const { signedMessage, message, recipient, nonce } = req.body;
  if (!signedMessage || !message || !recipient || !nonce)
    return res.status(400).json({ error: 'Missing fields' });

  const telegramId = getNonceOwner(nonce);
  if (!telegramId) return res.status(400).json({ error: 'Unknown or expired nonce' });

  const accountId = await verifyWalletSignature({ signedMessage, message, recipient, nonce });
  if (!accountId) return res.status(401).json({ error: 'Invalid signature' });

  const [nft, stakes] = await Promise.all([
    checkNftOwnership(accountId),
    getStakesForAccount(accountId),
  ]);
  const hasNft = nft.hasNft;
  const hasActiveStake = hasActiveStakes(stakes);

  setUserNear(telegramId, accountId);
  setUserNft(telegramId, hasNft);
  deleteNonce(nonce);

  if (!hasNft && !hasActiveStake) {
    return res.json({ success: false, accountId, reason: 'no_nft' });
  }

  const user = getUser(telegramId);
  if (user?.in_channel) return res.json({ success: true, accountId, already_in_channel: true });

  try {
    const link = await bot.telegram.createChatInviteLink(CHANNEL_ID, {
      member_limit: 1,
      expire_date: Math.floor(Date.now() / 1000) + 3600,
    });
    setUserInChannel(telegramId, true);
    await bot.telegram.sendMessage(
      telegramId,
      `🎉 ${hasNft ? 'NFT подтверждён!' : 'Стейк подтверждён!'}\n\nСсылка для входа в канал (1 час):\n${(link as any).invite_link}`
    );
    return res.json({ success: true, accountId, via_stake: !hasNft && hasActiveStake });
  } catch (err) {
    console.error('[SERVER] invite error:', err);
    return res.status(500).json({ error: 'Failed to create invite link' });
  }
});

app.post('/api/verify-webapp', rateLimitMiddleware, async (req, res) => {
  const { initData, nearAccount } = req.body;
  if (!initData || !nearAccount) return res.status(400).json({ error: 'Missing fields' });

  const { valid, userId } = validateTelegramInitData(initData);
  if (!valid || !userId) return res.status(401).json({ error: 'Invalid Telegram auth' });

  const [nft, stakes] = await Promise.all([
    checkNftOwnership(nearAccount),
    getStakesForAccount(nearAccount),
  ]);
  const hasNft = nft.hasNft;
  const hasActiveStake = hasActiveStakes(stakes);

  if (!hasNft && !hasActiveStake) return res.status(403).json({ success: false, reason: 'no_nft' });

  setUserNear(userId, nearAccount);
  setUserNft(userId, hasNft);

  const existing = getUser(userId);
  if (existing?.in_channel) {
    return res.json({ success: true, userId, nearAccount, already_in_channel: true });
  }

  await grantAccess(userId).catch(console.error);
  res.json({ success: true, userId, nearAccount, via_stake: !hasNft && hasActiveStake });
});

/**
 * Уведомление продавца о продаже NFT. Вызывается фронтендом сразу после
 * успешной покупки (buy_multiple). Перед отправкой сообщения бэкенд сам
 * проверяет ончейн, что заявленный покупатель реально теперь владеет каждым
 * из NFT — это защита от произвольных фейковых вызовов с чужими данными.
 * items: [{ tokenId, sellerAccount, price (строка NEAR, для текста) }]
 */
app.post('/api/notify-sale', rateLimitMiddleware, requireTelegramAuth, async (req, res) => {
  const { nearAccount, items } = req.body;
  if (!nearAccount || !Array.isArray(items) || items.length === 0) {
    return res.status(400).json({ error: 'nearAccount and items required' });
  }

  const results: Array<{ tokenId: string; notified: boolean; reason?: string }> = [];

  for (const item of items) {
    const { tokenId, sellerAccount, price } = item || {};
    if (!tokenId || !sellerAccount || !price) {
      results.push({ tokenId: tokenId || '?', notified: false, reason: 'missing_fields' });
      continue;
    }

    try {
      const owner = await getNftOwner(tokenId);
      if (owner !== nearAccount) {
        results.push({ tokenId, notified: false, reason: 'ownership_mismatch' });
        continue;
      }

      const seller = getUserByNearAccount(sellerAccount);
      if (!seller) {
        results.push({ tokenId, notified: false, reason: 'seller_not_found' });
        continue;
      }

      const text = seller.lang === 'en'
        ? `💰 Your NFT #${tokenId} has been sold for ${price} NEAR!`
        : `💰 Ваш NFT #${tokenId} продан за ${price} NEAR!`;
      await bot.telegram.sendMessage(seller.telegram_id, text);
      results.push({ tokenId, notified: true });
    } catch (err) {
      console.error(`[SERVER] notify-sale failed for token ${tokenId}:`, err);
      results.push({ tokenId, notified: false, reason: 'error' });
    }
  }

  res.json({ success: true, results });
});

app.post('/api/rpc', rateLimitMiddleware, async (req, res) => {
  const initData = req.headers['x-telegram-init-data'] as string;
  const apiKey   = req.headers['x-api-key'] as string;

  if (API_SECRET && apiKey !== API_SECRET) {
    if (!initData) return res.status(401).json({ error: 'Auth required' });
    const { valid } = validateTelegramInitData(initData);
    if (!valid) return res.status(401).json({ error: 'Invalid auth' });
  }

  try {
    const r = await fetch('https://free.rpc.fastnear.com', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(req.body),
    });
    res.json(await r.json());
  } catch (e: any) {
    res.status(500).json({ error: e.message });
  }
});

app.get('/api/preview-reward', rateLimitMiddleware, async (req, res) => {
  // Обратная совместимость: старые вызовы (например, внешний keep-alive cron)
  // могут ещё присылать nft_count вместо add_count — трактуем как add_count.
  const addCountRaw = (req.query.add_count as string) ?? (req.query.nft_count as string);
  const args = {
    period_days:          parseInt(req.query.period_days as string),
    current_active_count: parseInt((req.query.current_active_count as string) || '0'),
    add_count:            parseInt(addCountRaw),
  };
  if (!args.period_days || !args.add_count)
    return res.status(400).json({ error: 'period_days and add_count required' });

  try {
    const r = await fetch('https://free.rpc.fastnear.com', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        jsonrpc: '2.0', id: '1', method: 'query',
        params: {
          request_type: 'call_function',
          finality: 'final',
          account_id: STAKING_CONTRACT,
          method_name: 'preview_reward',
          args_base64: Buffer.from(JSON.stringify(args)).toString('base64'),
        },
      }),
    });
    const d: any = await r.json();
    res.json({ reward: JSON.parse(Buffer.from(d.result.result).toString()) });
  } catch (e: any) {
    res.status(500).json({ error: e.message });
  }
});

app.get('/api/stakes/:nearAccount', rateLimitMiddleware, requireApiKey, async (req, res) => {
  try {
    const r = await fetch('https://free.rpc.fastnear.com', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        jsonrpc: '2.0', id: '1', method: 'query',
        params: {
          request_type: 'call_function',
          finality: 'final',
          account_id: STAKING_CONTRACT,
          method_name: 'get_stakes_for_account',
          args_base64: Buffer.from(JSON.stringify({ account_id: req.params.nearAccount })).toString('base64'),
        },
      }),
    });
    const d: any = await r.json();
    res.json(JSON.parse(Buffer.from(d.result.result).toString()));
  } catch (e: any) {
    res.status(500).json({ error: e.message });
  }
});

app.get('/api/status', rateLimitMiddleware, requireApiKey, (req, res) => {
  const telegramId = parseInt(req.query.telegram_id as string);
  if (!telegramId) return res.status(400).json({ error: 'missing telegram_id' });
  const user = getUser(telegramId);
  if (!user) return res.json({ exists: false });
  res.json({
    exists:       true,
    near_account: user.near_account,
    has_nft:      !!user.has_nft,
    in_channel:   !!user.in_channel,
  });
});

// Используется веб-интерфейсом (app.html), чтобы решить — показывать ли вкладку "Admin".
// Ничего секретного не отдаёт, только true/false.
app.post('/api/is-admin', rateLimitMiddleware, (req, res) => {
  const initData = req.body?.initData || req.headers['x-telegram-init-data'] as string;
  if (!initData) return res.json({ isAdmin: false });
  const { valid, userId } = validateTelegramInitData(initData);
  res.json({ isAdmin: !!valid && !!userId && userId === ADMIN_TELEGRAM_ID });
});

async function main() {
  await initDb();
  await bot.telegram.deleteWebhook();
  await bot.telegram.setWebhook(`${WEBHOOK_URL}/webhook/${BOT_TOKEN}`);
  // Токен НЕ логируем: раньше строка печаталась целиком и оседала в логах
  // хостинга при каждом деплое. Токен даёт полный контроль над ботом —
  // чтение всех сообщений, отправку от его имени и подмену вебхука.
  console.log(`[BOT] Webhook установлен: ${WEBHOOK_URL}/webhook/***`);
  startCron();
  app.listen(PORT, () => console.log(`[SERVER] :${PORT}`));
}

main().catch(console.error);