import initSqlJs, { Database as SqlJsDatabase } from 'sql.js';
import path from 'path';
import fs from 'fs';

const DATA_DIR = path.join(__dirname, '..', 'data');
if (!fs.existsSync(DATA_DIR)) fs.mkdirSync(DATA_DIR, { recursive: true });

const DB_PATH = path.join(DATA_DIR, 'bot.db');

let db: SqlJsDatabase;

export async function initDb(): Promise<void> {
  const SQL = await initSqlJs();
  if (fs.existsSync(DB_PATH)) {
    const fileBuffer = fs.readFileSync(DB_PATH);
    db = new SQL.Database(fileBuffer);
  } else {
    db = new SQL.Database();
  }

  db.run(`
    CREATE TABLE IF NOT EXISTS users (
      telegram_id   INTEGER PRIMARY KEY,
      username      TEXT,
      near_account  TEXT,
      has_nft       INTEGER DEFAULT 0,
      in_channel    INTEGER DEFAULT 0,
      verified_at   TEXT,
      created_at    TEXT DEFAULT (datetime('now')),
      lang          TEXT DEFAULT 'ru'
    );

    CREATE TABLE IF NOT EXISTS nonces (
      nonce         TEXT PRIMARY KEY,
      telegram_id   INTEGER NOT NULL,
      created_at    TEXT DEFAULT (datetime('now')),
      lang          TEXT DEFAULT 'ru'
    );

    -- Дедупликация уведомлений о созревших стейках (реальные данные о стейках
    -- берутся из контракта через get_stakes_for_account, не из локальной БД —
    -- see history: раньше здесь была отдельная таблица stakes для локального
    -- трекинга, но она никогда не заполнялась и не читалась, убрана).
    -- stake_id — тот же идентификатор, что возвращает контракт.
    CREATE TABLE IF NOT EXISTS notified_matures (
      stake_id      TEXT PRIMARY KEY,
      telegram_id   INTEGER NOT NULL,
      notified_at   TEXT DEFAULT (datetime('now'))
    );
  `);

  // Миграция: добавить колонку lang если не существует
  try {
    db.run(`ALTER TABLE users ADD COLUMN lang TEXT DEFAULT 'ru'`);
    save();
  } catch {
    // колонка уже существует — игнорируем
  }

  save();
  console.log('[DB] Инициализирована');
}

function save() {
  const data = db.export();
  fs.writeFileSync(DB_PATH, Buffer.from(data));
}

function run(sql: string, params: any[] = []) {
  db.run(sql, params);
  save();
}

function get<T>(sql: string, params: any[] = []): T | undefined {
  const stmt = db.prepare(sql);
  stmt.bind(params);
  if (stmt.step()) {
    const row = stmt.getAsObject() as any;
    stmt.free();
    return row as T;
  }
  stmt.free();
  return undefined;
}

function all<T>(sql: string, params: any[] = []): T[] {
  const stmt = db.prepare(sql);
  stmt.bind(params);
  const rows: T[] = [];
  while (stmt.step()) {
    rows.push(stmt.getAsObject() as any);
  }
  stmt.free();
  return rows;
}

// --- Users ---

export function upsertUser(telegramId: number, username?: string) {
  run(`
    INSERT INTO users (telegram_id, username)
    VALUES (?, ?)
    ON CONFLICT(telegram_id) DO UPDATE SET username = excluded.username
  `, [telegramId, username ?? null]);
}

export function setUserNear(telegramId: number, nearAccount: string) {
  run(`UPDATE users SET near_account = ?, verified_at = datetime('now') WHERE telegram_id = ?`,
    [nearAccount, telegramId]);
}

export function setUserNft(telegramId: number, hasNft: boolean) {
  run(`UPDATE users SET has_nft = ? WHERE telegram_id = ?`, [hasNft ? 1 : 0, telegramId]);
}

export function setUserInChannel(telegramId: number, inChannel: boolean) {
  run(`UPDATE users SET in_channel = ? WHERE telegram_id = ?`, [inChannel ? 1 : 0, telegramId]);
}

export function setUserLang(telegramId: number, lang: string) {
  run(`UPDATE users SET lang = ? WHERE telegram_id = ?`, [lang, telegramId]);
}

export function getUserLang(telegramId: number): string {
  const row = get<{ lang: string }>(`SELECT lang FROM users WHERE telegram_id = ?`, [telegramId]);
  return row?.lang || 'ru';
}

export function getUser(telegramId: number): UserRow | undefined {
  return get<UserRow>(`SELECT * FROM users WHERE telegram_id = ?`, [telegramId]);
}

/** Поиск пользователя по NEAR-аккаунту — нужен, чтобы уведомить продавца о
 * продаже NFT, зная только его near_account (из события покупки на фронтенде). */
export function getUserByNearAccount(nearAccount: string): UserRow | undefined {
  return get<UserRow>(`SELECT * FROM users WHERE near_account = ?`, [nearAccount]);
}

export function getAllUsersInChannel(): UserRow[] {
  return all<UserRow>(`SELECT * FROM users WHERE in_channel = 1`);
}

// --- Nonces ---

export function saveNonce(nonce: string, telegramId: number) {
  run(`INSERT OR REPLACE INTO nonces (nonce, telegram_id) VALUES (?, ?)`, [nonce, telegramId]);
}

export function getNonceOwner(nonce: string): number | null {
  const row = get<{ telegram_id: number }>(`SELECT telegram_id FROM nonces WHERE nonce = ?`, [nonce]);
  return row ? row.telegram_id : null;
}

export function deleteNonce(nonce: string) {
  run(`DELETE FROM nonces WHERE nonce = ?`, [nonce]);
}

export function cleanOldNonces() {
  run(`DELETE FROM nonces WHERE created_at < datetime('now', '-10 minutes')`);
}

// --- Уведомления о созревших стейках (дедупликация) ---

export function isMatureStakeNotified(stakeId: string): boolean {
  const row = get<{ stake_id: string }>(`SELECT stake_id FROM notified_matures WHERE stake_id = ?`, [stakeId]);
  return !!row;
}

export function markMatureStakeNotified(stakeId: string, telegramId: number) {
  run(`INSERT OR IGNORE INTO notified_matures (stake_id, telegram_id) VALUES (?, ?)`, [stakeId, telegramId]);
}

// --- Types ---

export interface UserRow {
  telegram_id: number;
  username: string | null;
  near_account: string | null;
  has_nft: number;
  in_channel: number;
  verified_at: string | null;
  created_at: string;
  lang: string;
}