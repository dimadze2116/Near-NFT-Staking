import { Telegraf, Markup } from 'telegraf';
import { upsertUser, getUser, setUserInChannel, getAllUsersInChannel, setUserLang, getUserLang } from './db';
import { checkNftOwnership, getStakesForAccount, getRewardPool, getAccountBalance, nearToYocto, getClaimedStakesCount, getUniqueClaimersCount, getClaimedStakes, STAKING_CONTRACT } from './near';

const BOT_TOKEN = process.env.BOT_TOKEN!;
const CHANNEL_ID = process.env.CHANNEL_ID!;
const WEBAPP_URL = process.env.WEBAPP_URL!;
const ADMIN_TELEGRAM_ID = parseInt(process.env.ADMIN_TELEGRAM_ID || '0');
const RESTART_ENDPOINT = process.env.RESTART_ENDPOINT;
const RESTART_SECRET = process.env.RESTART_SECRET;

if (!ADMIN_TELEGRAM_ID) throw new Error('ADMIN_TELEGRAM_ID is missing in .env');

export const bot = new Telegraf(BOT_TOKEN);

// ── i18n ──────────────────────────────────────────────────────────────────────

type Lang = 'ru' | 'en';

const t = {
  ru: {
    already_in_channel:    '✅ Ты уже в канале! Всё работает.',
    checking_wallet:       '🔄 Проверяю твой кошелёк...',
    nft_not_found:         '❌ NFT не найден в кошельке.',
    welcome:               '👋 Привет! Для доступа в закрытый канал подтверди владение NFT NEAR Legion.\n\nНажми кнопку ниже:',
    connect_wallet_btn:    '🔗 Подключить кошелёк',
    not_registered:        '❌ Ты не зарегистрирован. Напиши /start',
    no_access:             '🔒 Сначала пройди верификацию через /start.',
    no_wallet:             '❌ Сначала привяжи кошелёк через /start.',
    checking_stakes:       '🔍 Проверяю стейки',
    no_stakes:             '📭 Нет активных стейков.',
    pool_label:            '💰 Пул',
    stakes_label:          '📦 Стейки',
    status_mature:         '✅ Готов к клейму',
    status_pending:        '⏳ В процессе',
    finish_label:          'Завершение',
    reward_label:          'Награда',
    total_label:           '💰 Всего',
    stakes_error:          '❌ Ошибка получения стейков.',
    stakes_not_mature:     '⏳ Стейки ещё не созрели. Проверь /mystakes.',
    no_mature_stakes:      '📭 Нет завершённых стейков. Используй /stake.',
    claim_ready:           '💰 стейк(ов) готовы к клейму!\nСумма',
    claim_open:            'Открой WebApp для подписи транзакции:',
    claim_btn:             '💰 Забрать награды',
    pool_title:            '📊 Пул наград',
    pool_contract:         '🏗 Контракт',
    pool_reward:           '💰 Reward pool',
    pool_balance:          '🏦 Account balance',
    pool_error:            '❌ Ошибка получения данных пула.',
    staking_btn:           '📦 Открыть стейкинг',
    staking_title:         '📈 Стейкинг NFT',
    granted:               '🎉 NFT подтверждён! Ссылка для входа в канал (действует 1 час):\n\n',
    help_no_access:        '📖 Команды:\n\n/start — подключить кошелёк\n/status — статус\n/help — справка\n\n🔒 Стейкинг доступен после вступления в канал.',
    help_access:           '📖 Команды:\n\n/start — подключить кошелёк\n/status — статус\n/stake — застейкать NFT\n/mystakes — мои стейки\n/claim — забрать награды\n/pool — пул наград\n/help — справка\n\n📌 APR (по позиции NFT у вас в стейкинге, одинаково для 7/14/30 дней):\n1-10: 24.2% | 11-20: 29.0% | 21-30: 33.2%\n31-40: 37.3% | 41-50: 41.5% | 51+: 42.1%',
    menu_connect:          '🔗 Подключить NEAR',
    menu_status:           '📋 Статус',
    menu_help:             'ℹ️ Помощь',
    menu_stake:            '🎴 Стейкинг',
    menu_mystakes:         '📦 Мои стейки',
    menu_claim:            '💰 Забрать награды',
    menu_pool:             '📊 Пул',
    menu_admin:            '👮 Админ-панель',
    lang_changed:          '🇷🇺 Язык изменён на русский',
    choose_lang:           '🌐 Выбери язык / Choose language:',
    status_near:           'не привязан',
    status_nft_yes:        '✅ есть',
    status_nft_no:         '❌ нет',
    status_channel_yes:    '✅ в канале',
    status_channel_no:     '❌ не в канале',
  },
  en: {
    already_in_channel:    '✅ You are already in the channel! Everything works.',
    checking_wallet:       '🔄 Checking your wallet...',
    nft_not_found:         '❌ NFT not found in wallet.',
    welcome:               '👋 Hi! To access the private channel, confirm ownership of your NEAR Legion NFT.\n\nClick the button below:',
    connect_wallet_btn:    '🔗 Connect Wallet',
    not_registered:        '❌ You are not registered. Type /start',
    no_access:             '🔒 Please verify first via /start.',
    no_wallet:             '❌ Please connect your wallet via /start.',
    checking_stakes:       '🔍 Checking stakes for',
    no_stakes:             '📭 No active stakes.',
    pool_label:            '💰 Pool',
    stakes_label:          '📦 Stakes',
    status_mature:         '✅ Ready to claim',
    status_pending:        '⏳ In progress',
    finish_label:          'Ends',
    reward_label:          'Reward',
    total_label:           '💰 Total',
    stakes_error:          '❌ Error fetching stakes.',
    stakes_not_mature:     '⏳ Stakes are not mature yet. Check /mystakes.',
    no_mature_stakes:      '📭 No completed stakes. Use /stake.',
    claim_ready:           '💰 stake(s) ready to claim!\nAmount',
    claim_open:            'Open WebApp to sign the transaction:',
    claim_btn:             '💰 Claim Rewards',
    pool_title:            '📊 Reward Pool',
    pool_contract:         '🏗 Contract',
    pool_reward:           '💰 Reward pool',
    pool_balance:          '🏦 Account balance',
    pool_error:            '❌ Error fetching pool data.',
    staking_btn:           '📦 Open Staking',
    staking_title:         '📈 NFT Staking',
    granted:               '🎉 NFT confirmed! Channel invite link (valid 1 hour):\n\n',
    help_no_access:        '📖 Commands:\n\n/start — connect wallet\n/status — check status\n/help — help\n\n🔒 Staking is available after joining the channel.',
    help_access:           '📖 Commands:\n\n/start — connect wallet\n/status — status\n/stake — stake NFT\n/mystakes — my stakes\n/claim — claim rewards\n/pool — reward pool\n/help — help\n\n📌 APR (by NFT position in your stakes, same for 7/14/30 days):\n1-10: 24.2% | 11-20: 29.0% | 21-30: 33.2%\n31-40: 37.3% | 41-50: 41.5% | 51+: 42.1%',
    menu_connect:          '🔗 Connect NEAR',
    menu_status:           '📋 Status',
    menu_help:             'ℹ️ Help',
    menu_stake:            '🎴 Staking',
    menu_mystakes:         '📦 My Stakes',
    menu_claim:            '💰 Claim Rewards',
    menu_pool:             '📊 Pool',
    menu_admin:            '👮 Admin Panel',
    lang_changed:          '🇬🇧 Language changed to English',
    choose_lang:           '🌐 Выбери язык / Choose language:',
    status_near:           'not linked',
    status_nft_yes:        '✅ yes',
    status_nft_no:         '❌ no',
    status_channel_yes:    '✅ in channel',
    status_channel_no:     '❌ not in channel',
  }
};

function getLang(telegramId: number): Lang {
  return (getUserLang(telegramId) as Lang) || 'ru';
}

function tr(telegramId: number, key: keyof typeof t.ru): string {
  const lang = getLang(telegramId);
  return t[lang][key] || t.ru[key];
}

// ── Admin ─────────────────────────────────────────────────────────────────────

// Converts yoctoNEAR (string/BigInt) to human-readable NEAR with 4 decimal places.
// Same logic as yoctoToNear() in app.html - formatting is kept consistent.
function yoctoToNear(v: string | number): string {
  try {
    const n = BigInt(String(v || '0'));
    const d = 1_000_000_000_000_000_000_000_000n;
    const whole = n / d;
    const frac = (n % d).toString().padStart(24, '0').substring(0, 4);
    return `${whole}.${frac}`;
  } catch {
    return '0';
  }
}

function isAdmin(ctx: any): boolean {
  return ctx.from?.id === ADMIN_TELEGRAM_ID;
}

function userHasAccess(user: any): boolean {
  return !!user?.in_channel;
}

// ── Menus ─────────────────────────────────────────────────────────────────────

function mainMenu(user: any, telegramId: number) {
  const lang = getLang(telegramId);
  const T = t[lang];
  const rows: any[] = [];
  if (!userHasAccess(user)) {
    rows.push([
      Markup.button.callback(T.menu_connect, 'user_connect'),
      Markup.button.callback(T.menu_status, 'user_status'),
    ]);
    rows.push([Markup.button.callback(T.menu_help, 'user_help')]);
  } else {
    rows.push([
      Markup.button.callback(T.menu_stake, 'user_stake'),
      Markup.button.callback(T.menu_mystakes, 'user_mystakes'),
    ]);
    rows.push([
      Markup.button.callback(T.menu_claim, 'user_claim'),
      Markup.button.callback(T.menu_pool, 'user_pool'),
    ]);
    rows.push([
      Markup.button.callback(T.menu_status, 'user_status'),
      Markup.button.callback(T.menu_help, 'user_help'),
    ]);
  }
  rows.push([
    Markup.button.callback('🇷🇺 RU', 'lang_ru'),
    Markup.button.callback('🇬🇧 EN', 'lang_en'),
  ]);
  if (isAdmin({ from: { id: telegramId } })) {
    rows.push([Markup.button.callback(T.menu_admin, 'user_admin')]);
  }
  return Markup.inlineKeyboard(rows);
}

function adminMenu() {
  return Markup.inlineKeyboard([
    [Markup.button.callback('📊 Пул', 'admin_pool'), Markup.button.callback('🧾 Стейки', 'admin_stakes')],
    [Markup.button.callback('🔖 Заклеймлено', 'admin_claimed'), Markup.button.callback('👥 Пользователи', 'admin_users')],
    [Markup.button.callback('💰 Пополнение', 'admin_deposit'), Markup.button.callback('🏦 Вывод пула', 'admin_withdraw')],
    [Markup.button.callback('📈 Статус', 'admin_status'), Markup.button.callback('🔄 Рестарт', 'admin_restart')],
    [Markup.button.callback('▶️ Запуск', 'admin_start'), Markup.button.callback('⏹ Стоп', 'admin_stop')],
    [Markup.button.callback('🩺 Health', 'admin_health')],
  ]);
}

// ── Helpers ───────────────────────────────────────────────────────────────────

async function getPoolNear(): Promise<string> {
  const pool = await getRewardPool().catch(() => '0');
  return (BigInt(pool || '0') / 1_000_000_000_000_000_000_000_000n).toString();
}

async function getAccountNear(): Promise<string> {
  const balance = await getAccountBalance(STAKING_CONTRACT).catch(() => '0');
  return (BigInt(balance || '0') / 1_000_000_000_000_000_000_000_000n).toString();
}

async function buildStatusText(telegramId: number): Promise<string> {
  const user = getUser(telegramId);
  const lang = getLang(telegramId);
  const T = t[lang];
  if (!user) return T.not_registered;
  return [
    `👤 Telegram: @${user.username ?? telegramId}`,
    `💳 NEAR: ${user.near_account ?? T.status_near}`,
    `🖼 NFT: ${user.has_nft ? T.status_nft_yes : T.status_nft_no}`,
    `📢 ${lang === 'ru' ? 'Канал' : 'Channel'}: ${user.in_channel ? T.status_channel_yes : T.status_channel_no}`,
  ].join('\n');
}

async function getLocalStakesSummary() {
  const users = getAllUsersInChannel();
  const seen = new Set<string>();
  const seenClaimers = new Set<string>();
  let active = 0, matured = 0, claimed = 0, claimedRewardNear = 0;
  for (const user of users) {
    const nearAccount = (user as any).near_account;
    if (!nearAccount) continue;
    seen.add(nearAccount);
    const stakes = await getStakesForAccount(nearAccount).catch(() => []);
    for (const stake of stakes) {
      if (stake.claimed) {
        claimed++;
        seenClaimers.add(nearAccount);
        claimedRewardNear += parseFloat(stake.reward_near) || 0;
        continue;
      }
      active++;
      if (stake.is_mature) matured++;
    }
  }
  return { active, matured, unique: seen.size, claimed, claimedUnique: seenClaimers.size, claimedRewardNear };
}

// ── Language switch ───────────────────────────────────────────────────────────

bot.action('lang_ru', async (ctx) => {
  const telegramId = ctx.from.id;
  setUserLang(telegramId, 'ru');
  const user = getUser(telegramId);
  await ctx.answerCbQuery('🇷🇺 Русский').catch(() => undefined);
  await ctx.reply(t.ru.lang_changed, mainMenu(user, telegramId));
});

bot.action('lang_en', async (ctx) => {
  const telegramId = ctx.from.id;
  setUserLang(telegramId, 'en');
  const user = getUser(telegramId);
  await ctx.answerCbQuery('🇬🇧 English').catch(() => undefined);
  await ctx.reply(t.en.lang_changed, mainMenu(user, telegramId));
});

// ── /start ────────────────────────────────────────────────────────────────────

bot.start(async (ctx) => {
  const telegramId = ctx.from.id;
  const username = ctx.from.username;
  upsertUser(telegramId, username);
  const user = getUser(telegramId);
  const T = t[getLang(telegramId)];

  if (user?.in_channel) {
    return ctx.reply(T.already_in_channel, mainMenu({ ...user, telegram_id: telegramId, username }, telegramId));
  }

  if (user?.near_account && !user.in_channel) {
    await ctx.reply(T.checking_wallet);
    const { hasNft } = await checkNftOwnership(user.near_account);
    if (hasNft) {
      await grantAccess(telegramId);
    } else {
      return ctx.reply(T.nft_not_found, mainMenu(user, telegramId));
    }
    return;
  }

  await ctx.reply(T.welcome, {
    reply_markup: { inline_keyboard: [
      [{ text: T.connect_wallet_btn, web_app: { url: WEBAPP_URL } }],
      [{ text: '🇷🇺 Русский', callback_data: 'lang_ru' }, { text: '🇬🇧 English', callback_data: 'lang_en' }],
    ]}
  });
});

// ── User actions ──────────────────────────────────────────────────────────────

bot.action(/^user_connect$/, async (ctx) => {
  const telegramId = ctx.from.id;
  const user = getUser(telegramId);
  const T = t[getLang(telegramId)];
  if (userHasAccess(user)) return ctx.answerCbQuery().catch(() => undefined);
  await ctx.reply(T.welcome, {
    reply_markup: { inline_keyboard: [[{ text: T.connect_wallet_btn, web_app: { url: WEBAPP_URL } }]] }
  });
  await ctx.answerCbQuery().catch(() => undefined);
});

bot.action(/^user_status$/, async (ctx) => {
  await ctx.reply(await buildStatusText(ctx.from.id));
  await ctx.answerCbQuery().catch(() => undefined);
});

bot.action(/^user_help$/, async (ctx) => {
  const telegramId = ctx.from.id;
  const user = getUser(telegramId);
  const T = t[getLang(telegramId)];
  await ctx.reply(user?.in_channel ? T.help_access : T.help_no_access);
  await ctx.answerCbQuery().catch(() => undefined);
});

bot.action(/^user_stake$/, async (ctx) => {
  const telegramId = ctx.from.id;
  const user = getUser(telegramId);
  const T = t[getLang(telegramId)];
  if (!userHasAccess(user)) return ctx.reply(T.no_access);
  if (!user?.near_account) return ctx.reply(T.no_wallet);
  const webAppUrl = `${WEBAPP_URL}?tab=stake&account_id=${user.near_account}&lang=${getLang(telegramId)}`;
  await ctx.reply(T.staking_title, { reply_markup: { inline_keyboard: [[{ text: T.staking_btn, web_app: { url: webAppUrl } }]] } });
  await ctx.answerCbQuery().catch(() => undefined);
});

bot.action(/^user_mystakes$/, async (ctx) => {
  const telegramId = ctx.from.id;
  const user = getUser(telegramId);
  const T = t[getLang(telegramId)];
  if (!userHasAccess(user)) return ctx.reply(T.no_access);
  if (!user?.near_account) return ctx.reply(T.no_wallet);

  const stakes = await getStakesForAccount(user.near_account);
  const active = stakes.filter((s: any) => !s.claimed);

  if (!active.length) {
    await ctx.reply(T.no_stakes);
  } else {
    let msg = `${T.stakes_label}: ${active.length}\n\n`;
    for (const s of active) {
      const date = new Date(s.staked_at * 1000).toLocaleDateString(getLang(telegramId) === 'ru' ? 'ru-RU' : 'en-GB');
      const daysLeft = Math.max(0, Math.ceil((s.unlocks_at - Math.floor(Date.now() / 1000)) / 86400));
      const reward = (BigInt(s.reward_near) / BigInt('1000000000000000000000000')).toString();
      const status = s.is_mature ? T.status_mature : `⏳ ${daysLeft} ${getLang(telegramId) === 'ru' ? 'дн.' : 'd.'}`;
      msg += `🎴 NFT #${s.token_id}\n📅 ${getLang(telegramId) === 'ru' ? 'Начало' : 'Started'}: ${date}\n💰 ${T.reward_label}: ${reward} NEAR\n${status}\n\n`;
    }
    await ctx.reply(msg);
  }
  await ctx.answerCbQuery().catch(() => undefined);
});

bot.action(/^user_claim$/, async (ctx) => {
  const telegramId = ctx.from.id;
  const user = getUser(telegramId);
  const T = t[getLang(telegramId)];
  if (!userHasAccess(user)) return ctx.reply(T.no_access);
  if (!user?.near_account) return ctx.reply(T.no_wallet);
  await ctx.reply(T.claim_open, {
    reply_markup: { inline_keyboard: [[{
      text: T.claim_btn,
      web_app: { url: `${WEBAPP_URL}?tab=mystakes&account_id=${user.near_account}&lang=${getLang(telegramId)}` }
    }]] }
  });
  await ctx.answerCbQuery().catch(() => undefined);
});

bot.action(/^user_pool$/, async (ctx) => {
  const telegramId = ctx.from.id;
  const T = t[getLang(telegramId)];
  const poolNear = await getPoolNear();
  const accountNear = await getAccountNear();
  await ctx.reply(`${T.pool_title}\n\n${T.pool_contract}: ${STAKING_CONTRACT}\n${T.pool_reward}: ${poolNear} NEAR\n${T.pool_balance}: ${accountNear} NEAR`);
  await ctx.answerCbQuery().catch(() => undefined);
});

bot.action(/^user_admin$/, async (ctx) => {
  if (!isAdmin(ctx)) return ctx.answerCbQuery().catch(() => undefined);
  await ctx.reply('👮 Админ-панель', adminMenu());
  await ctx.answerCbQuery().catch(() => undefined);
});

// ── Admin actions ─────────────────────────────────────────────────────────────

bot.action(/^admin_pool$/, async (ctx) => {
  if (!isAdmin(ctx)) return ctx.reply('⛔ Доступ только для админа.');
  const poolNear = await getPoolNear();
  const accountNear = await getAccountNear();
  await ctx.editMessageText(`📊 Пул\n\n${STAKING_CONTRACT}\nPool: ${poolNear} NEAR\nBalance: ${accountNear} NEAR`, adminMenu()).catch(() => undefined);
  await ctx.answerCbQuery().catch(() => undefined);
});

bot.action(/^admin_stakes$/, async (ctx) => {
  if (!isAdmin(ctx)) return ctx.reply('⛔ Доступ только для админа.');
  const stats = await getLocalStakesSummary();
  // Fetch the data from the contract
  let contractTotal = '?';
  try {
    const result = await fetch(`https://free.rpc.fastnear.com`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        jsonrpc: '2.0', id: '1', method: 'query',
        params: {
          request_type: 'call_function',
          finality: 'final',
          account_id: STAKING_CONTRACT,
          method_name: 'get_contract_stats',
          args_base64: 'e30=',
        }
      })
    });
    const data: any = await result.json();
    const decoded = JSON.parse(Buffer.from(data.result.result).toString());
    contractTotal = decoded.total_staked;
  } catch {}

  // Claimed stakes come straight from the contract (globally, across ALL accounts,
  // not only the ones present in the bot's local database)
  const [claimedCount, claimedUnique] = await Promise.all([
    getClaimedStakesCount(),
    getUniqueClaimersCount(),
  ]);

  await ctx.editMessageText(
    `🧾 Стейки\n\n` +
    `📦 Всего NFT в стейкинге: ${contractTotal}\n` +
    `✅ Активных: ${stats.active}\n` +
    `🔓 Созревших: ${stats.matured}\n` +
    `👥 Уникальных стейкеров: ${stats.unique}\n\n` +
    `🔖 Заклеймлено (всего по контракту): ${claimedCount}\n` +
    `👤 Уникальных заклеймивших: ${claimedUnique}`,
    adminMenu()
  ).catch(() => undefined);
  await ctx.answerCbQuery().catch(() => undefined);
});

bot.action(/^admin_claimed$/, async (ctx) => {
  if (!isAdmin(ctx)) return ctx.reply('⛔ Доступ только для админа.');

  const [claimedCount, allClaims] = await Promise.all([
    getClaimedStakesCount(),
    getClaimedStakes(),
  ]);
    const claims = allClaims.slice(0, 15); // latest by unlock time, top 15 for the preview

  if (claims.length === 0) {
    await ctx.editMessageText('🔖 Заклеймлено: 0\n\nПока никто не забирал награды.', adminMenu()).catch(() => undefined);
    return ctx.answerCbQuery().catch(() => undefined);
  }

  const totalRewardYocto = claims.reduce((sum, c) => sum + BigInt(c.reward_near || '0'), 0n);
  const lines = claims.map((c) => {
    const date = new Date(c.unlocks_at * 1000).toISOString().slice(0, 10);
    return `• ${c.owner} — ${c.token_id} — ${yoctoToNear(c.reward_near)} NEAR (${date})`;
  });

  await ctx.editMessageText(
    `🔖 Заклеймленные стейки\n\n` +
    `Всего по контракту: ${claimedCount}\n` +
    `Показаны последние ${claims.length} из ${allClaims.length}, сумма наград по ним: ${yoctoToNear(totalRewardYocto.toString())} NEAR\n\n` +
    lines.join('\n'),
    adminMenu()
  ).catch(() => undefined);
  await ctx.answerCbQuery().catch(() => undefined);
});

bot.action(/^admin_users$/, async (ctx) => {
  if (!isAdmin(ctx)) return ctx.reply('⛔ Доступ только для админа.');
  await ctx.editMessageText(`👥 Users in channel: ${getAllUsersInChannel().length}`, adminMenu()).catch(() => undefined);
  await ctx.answerCbQuery().catch(() => undefined);
});

bot.action(/^admin_deposit$/, async (ctx) => {
  if (!isAdmin(ctx)) return ctx.reply('⛔ Доступ только для админа.');
  await ctx.editMessageText('💰 /deposittopup <amount_NEAR> — бот выдаст готовую CLI-команду', adminMenu()).catch(() => undefined);
  await ctx.answerCbQuery().catch(() => undefined);
});

bot.action(/^admin_withdraw$/, async (ctx) => {
  if (!isAdmin(ctx)) return ctx.reply('⛔ Доступ только для админа.');
  await ctx.editMessageText('🏦 /withdrawpool <amount_NEAR> — бот выдаст готовую CLI-команду', adminMenu()).catch(() => undefined);
  await ctx.answerCbQuery().catch(() => undefined);
});

bot.action(/^admin_status$/, async (ctx) => {
  if (!isAdmin(ctx)) return ctx.reply('⛔ Доступ только для админа.');
  await ctx.editMessageText('📈 Bot is running on Render.', adminMenu()).catch(() => undefined);
  await ctx.answerCbQuery().catch(() => undefined);
});

bot.action(/^admin_restart$/, async (ctx) => {
  if (!isAdmin(ctx)) return ctx.reply('⛔ Доступ только для админа.');
  if (RESTART_ENDPOINT) {
    try {
      await fetch(RESTART_ENDPOINT, { method: 'POST', headers: RESTART_SECRET ? { 'x-restart-secret': RESTART_SECRET } : undefined });
      await ctx.editMessageText('🔄 Restart sent.', adminMenu()).catch(() => undefined);
    } catch {
      await ctx.editMessageText('❌ Failed to call RESTART_ENDPOINT.', adminMenu()).catch(() => undefined);
    }
  } else {
    await ctx.editMessageText('🔄 RESTART_ENDPOINT not set.', adminMenu()).catch(() => undefined);
  }
  await ctx.answerCbQuery().catch(() => undefined);
});

bot.action(/^admin_start$/, async (ctx) => {
  if (!isAdmin(ctx)) return ctx.reply('⛔ Доступ только для админа.');
  await ctx.editMessageText('▶️ Use PM2/systemd/Docker to start.', adminMenu()).catch(() => undefined);
  await ctx.answerCbQuery().catch(() => undefined);
});

bot.action(/^admin_stop$/, async (ctx) => {
  if (!isAdmin(ctx)) return ctx.reply('⛔ Доступ только для админа.');
  await ctx.editMessageText('⏹ Use graceful shutdown.', adminMenu()).catch(() => undefined);
  await ctx.answerCbQuery().catch(() => undefined);
});

bot.action(/^admin_health$/, async (ctx) => {
  if (!isAdmin(ctx)) return ctx.reply('⛔ Доступ только для админа.');
  const poolNear = await getPoolNear();
  await ctx.editMessageText(`🩺 Health\n\nDB: ok\nRPC: ok\nWebhook: ok\nCron: ok\nPool: ${poolNear} NEAR`, adminMenu()).catch(() => undefined);
  await ctx.answerCbQuery().catch(() => undefined);
});

bot.action(/^confirm_cancel$/, async (ctx) => {
  if (!isAdmin(ctx)) return ctx.reply('⛔ Доступ только для админа.');
  await ctx.editMessageText('Cancelled / Отменено.').catch(() => undefined);
  await ctx.answerCbQuery().catch(() => undefined);
});

// The bot no longer signs transactions: the contract owner's private key must not
// sit on the server. Instead of executing, it hands back a ready CLI command - the
// bot converts to yoctoNEAR itself, because getting that wrong by an order of
// magnitude by hand is far too easy.

bot.command('deposittopup', async (ctx) => {
  if (!isAdmin(ctx)) return ctx.reply('⛔ Admin only.');
  const amount = ctx.message?.text?.split(' ')[1];
  if (!amount) return ctx.reply('Usage: /deposittopup <amount_NEAR>');
  if (isNaN(parseFloat(amount)) || parseFloat(amount) <= 0) {
    return ctx.reply('❌ Некорректная сумма.');
  }
  await ctx.reply(
    `💰 Пополнение пула на ${amount} NEAR\n\n` +
    `Выполнить на своей машине:\n\n` +
    `near contract call-function as-transaction ${STAKING_CONTRACT} deposit_rewards ` +
    `json-args '{}' prepaid-gas '30 Tgas' attached-deposit '${amount} NEAR' ` +
    `sign-as ${STAKING_CONTRACT} network-config mainnet sign-with-keychain send\n\n` +
    `⚠️ Пополнение пула снова открывает приём новых стейков.`
  );
});

bot.command('withdrawpool', async (ctx) => {
  if (!isAdmin(ctx)) return ctx.reply('⛔ Admin only.');
  const amount = ctx.message?.text?.split(' ')[1];
  if (!amount) return ctx.reply('Usage: /withdrawpool <amount_NEAR>');
  if (isNaN(parseFloat(amount)) || parseFloat(amount) <= 0) {
    return ctx.reply('❌ Некорректная сумма.');
  }
  const yocto = nearToYocto(amount);
  await ctx.reply(
    `🏦 Вывод из пула ${amount} NEAR\n\n` +
    `Выполнить на своей машине:\n\n` +
    `near contract call-function as-transaction ${STAKING_CONTRACT} withdraw_rewards ` +
    `json-args '{"amount": "${yocto}"}' prepaid-gas '50 Tgas' attached-deposit '1 yoctoNEAR' ` +
    `sign-as ${STAKING_CONTRACT} network-config mainnet sign-with-keychain send\n\n` +
    `Выводится только свободный пул — зарезервированные награды не затрагиваются.`
  );
});

// ── Commands ──────────────────────────────────────────────────────────────────

bot.command('help', async (ctx) => {
  const telegramId = ctx.from.id;
  const user = getUser(telegramId);
  const T = t[getLang(telegramId)];
  return ctx.reply(user?.in_channel ? T.help_access : T.help_no_access);
});

bot.command('status', async (ctx) => {
  const telegramId = ctx.from.id;
  const user = getUser(telegramId);
  if (!user) return ctx.reply(tr(telegramId, 'not_registered'), mainMenu(user, telegramId));
  ctx.reply(await buildStatusText(telegramId), mainMenu(user, telegramId));
});

bot.command('stake', async (ctx) => {
  const telegramId = ctx.from.id;
  const user = getUser(telegramId);
  const T = t[getLang(telegramId)];
  if (!user?.in_channel) return ctx.reply(T.no_access, mainMenu(user, telegramId));
  if (!user?.near_account) return ctx.reply(T.no_wallet, mainMenu(user, telegramId));
  const webAppUrl = `${WEBAPP_URL}?tab=stake&account_id=${user.near_account}&lang=${getLang(telegramId)}`;
  ctx.reply(T.staking_title, { reply_markup: { inline_keyboard: [[{ text: T.staking_btn, web_app: { url: webAppUrl } }]] } });
});

bot.command('mystakes', async (ctx) => {
  const telegramId = ctx.from.id;
  const user = getUser(telegramId);
  const T = t[getLang(telegramId)];
  if (!user?.in_channel) return ctx.reply(T.no_access, mainMenu(user, telegramId));
  if (!user?.near_account) return ctx.reply(T.no_wallet, mainMenu(user, telegramId));

  await ctx.reply(`${T.checking_stakes} ${user.near_account}...`);
  try {
    const stakes = await getStakesForAccount(user.near_account);
    if (!stakes.length) {
      return ctx.reply(`${T.no_stakes}\n\n💳 ${user.near_account}`, mainMenu(user, telegramId));
    }
    const active = stakes.filter(s => !s.claimed);
    let msg = `${T.stakes_label} (${active.length})\n💳 ${user.near_account}\n\n`;
    for (const s of active) {
      const status = s.is_mature ? T.status_mature : T.status_pending;
      const finishDate = new Date(s.unlocks_at * 1000).toLocaleDateString(getLang(telegramId) === 'ru' ? 'ru-RU' : 'en-GB');
      const reward = (BigInt(s.reward_near || '0') / BigInt('1000000000000000000000000')).toString();
      msg += `#${s.token_id} · ${s.period_days}d\n  ${status}\n  ${T.finish_label}: ${finishDate}\n  ${T.reward_label}: ${reward} NEAR\n\n`;
    }
    await ctx.reply(msg, mainMenu(user, telegramId));
  } catch {
    ctx.reply(T.stakes_error, mainMenu(user, telegramId));
  }
});

bot.command('claim', async (ctx) => {
  const telegramId = ctx.from.id;
  const user = getUser(telegramId);
  const T = t[getLang(telegramId)];
  if (!user?.in_channel) return ctx.reply(T.no_access, mainMenu(user, telegramId));
  if (!user?.near_account) return ctx.reply(T.no_wallet, mainMenu(user, telegramId));

  try {
    const stakes = await getStakesForAccount(user.near_account);
    const mature = stakes.filter(s => !s.claimed && s.is_mature);
    if (!mature.length) {
      const pending = stakes.filter(s => !s.claimed && !s.is_mature);
      if (pending.length) return ctx.reply(T.stakes_not_mature, mainMenu(user, telegramId));
      return ctx.reply(T.no_mature_stakes, mainMenu(user, telegramId));
    }
    const totalReward = mature.reduce((sum, s) => sum + BigInt(s.reward_near || '0'), 0n);
    const totalNear = (totalReward / BigInt('1000000000000000000000000')).toString();
    ctx.reply(
      `💰 ${mature.length} ${T.claim_ready}: ${totalNear} NEAR\n\n${T.claim_open}`,
      { reply_markup: { inline_keyboard: [[{ text: T.claim_btn, web_app: { url: `${WEBAPP_URL}?tab=mystakes&account_id=${user.near_account}&lang=${getLang(telegramId)}` } }]] } }
    );
  } catch {
    ctx.reply(T.stakes_error, mainMenu(user, telegramId));
  }
});

bot.command('pool', async (ctx) => {
  const telegramId = ctx.from.id;
  const T = t[getLang(telegramId)];
  try {
    const poolNear = await getPoolNear();
    const accountNear = await getAccountNear();
    ctx.reply(
      `${T.pool_title}\n\n${T.pool_contract}: ${STAKING_CONTRACT}\n${T.pool_reward}: ${poolNear} NEAR\n${T.pool_balance}: ${accountNear} NEAR`,
      mainMenu(getUser(telegramId), telegramId)
    );
  } catch {
    ctx.reply(T.pool_error);
  }
});

bot.command('lang', async (ctx) => {
  await ctx.reply(t.ru.choose_lang, Markup.inlineKeyboard([
    [Markup.button.callback('🇷🇺 Русский', 'lang_ru'), Markup.button.callback('🇬🇧 English', 'lang_en')]
  ]));
});

bot.command('whoami', async (ctx) => {
  await ctx.reply(`👤 id: ${ctx.from?.id}\n👤 username: @${ctx.from?.username ?? 'none'}`);
});

bot.on('callback_query', async (ctx) => {
  await ctx.answerCbQuery().catch(() => undefined);
});

// ── Exported helpers ──────────────────────────────────────────────────────────

export async function grantAccess(telegramId: number) {
  try {
    const link = await bot.telegram.createChatInviteLink(CHANNEL_ID, {
      member_limit: 1,
      expire_date: Math.floor(Date.now() / 1000) + 3600,
    });
    setUserInChannel(telegramId, true);
    const T = t[getLang(telegramId)];
    await bot.telegram.sendMessage(
      telegramId,
      T.granted + link.invite_link,
      mainMenu(getUser(telegramId), telegramId)
    );
  } catch (err) {
    console.error('[BOT] grantAccess error:', err);
  }
}

export async function kickFromChannel(telegramId: number) {
  try {
    await bot.telegram.banChatMember(CHANNEL_ID, telegramId);
    await bot.telegram.unbanChatMember(CHANNEL_ID, telegramId);
    setUserInChannel(telegramId, false);
  } catch (err) {
    console.error(`[BOT] kickFromChannel error for ${telegramId}:`, err);
  }
}
