import { createContext, useContext, useState, useCallback, useEffect, useRef } from "react";
import { connectWallet, checkIsAdmin } from "./near";

const WalletContext = createContext(null);

// Между запусками сохраняем ТОЛЬКО идентификатор аккаунта (строка вида
// "alice.near"). Это публичные данные — они и так видны в блокчейне, красть
// по ним нечего. Сама сессия кошелька (объект-подписант) НЕ сохраняется:
// при каждом заходе её нет, и она поднимается только в момент реального
// действия с деньгами (стейк/анстейк/покупка/продажа/отмена).
//
// Зачем так: всё чтение (список NFT, активные стейки, награды, ранг,
// лидерборд, объявления маркета) идёт через RPC view-вызовы, которым нужна
// только строка accountId. Поэтому человек, открыв приложение, сразу видит
// свои данные без единого окна подтверждения — а подтверждение кошелька
// появляется ровно тогда, когда оно осмысленно.
const LS_ACCOUNT_KEY = "nft-staking:account-id";

function readStoredAccount() {
  try {
    return localStorage.getItem(LS_ACCOUNT_KEY) || null;
  } catch {
    /* localStorage недоступен (приватный режим и т.п.) — работаем без восстановления */
    return null;
  }
}

function writeStoredAccount(accountId) {
  try {
    if (accountId) localStorage.setItem(LS_ACCOUNT_KEY, accountId);
    else localStorage.removeItem(LS_ACCOUNT_KEY);
  } catch {
    /* не критично: не сохранили — просто в следующий раз попросим подключиться */
  }
}

/**
 * Ошибка, которую бросает ensureSigner(), если при подключении пользователь
 * выбрал в кошельке НЕ тот аккаунт, для которого начиналось действие.
 * Действие в этом случае обязано прерваться: список NFT/стейков на экране
 * относится к прежнему аккаунту, и выполнять его от имени нового нельзя.
 */
export class AccountChangedError extends Error {
  constructor(from, to) {
    super(`account changed: ${from} -> ${to}`);
    this.name = "AccountChangedError";
    this.code = "account_changed";
    this.from = from;
    this.to = to;
  }
}

export function WalletProvider({ children }) {
  // accountId восстанавливаем из хранилища — этого достаточно для чтения.
  const [accountId, setAccountId] = useState(readStoredAccount);
  // near — подписант. null до тех пор, пока пользователь не совершит действие,
  // требующее подписи. Наличие accountId при near === null это нормальное
  // состояние "режим чтения", а не ошибка.
  const [near, setNear] = useState(null);
  // accountRestored — accountId поднят из хранилища, а не получен подключением
  // в этой сессии. Нужен интерфейсу, чтобы не показывать при каждом открытии
  // приложения тост об успешной верификации (пользователь ничего не делал).
  const [accountRestored, setAccountRestored] = useState(() => !!readStoredAccount());
  const [connecting, setConnecting] = useState(false);
  const [isAdmin, setIsAdmin] = useState(false);
  // refreshKey — увеличиваем после стейка/клейма, чтобы компоненты,
  // читающие данные с контракта (Header, StakeScreen, MyStakesBody),
  // перезапросили актуальные данные.
  const [refreshKey, setRefreshKey] = useState(0);

  // Держим ссылку на коннектор, чтобы корректно завершить сессию на стороне
  // библиотеки при явном отключении.
  const connectorRef = useRef(null);
  // Промис незавершённого подключения: если два действия одновременно
  // запросят подписанта, окно выбора кошелька должно открыться один раз.
  const pendingConnectRef = useRef(null);
  // Актуальный accountId для чтения внутри колбэков без пересоздания функций.
  const accountIdRef = useRef(accountId);
  useEffect(() => {
    accountIdRef.current = accountId;
  }, [accountId]);

  // Права админа определяются по Telegram, не по кошельку — проверяем один раз.
  useEffect(() => {
    checkIsAdmin().then(setIsAdmin);
  }, []);

  // Единая точка подключения: открывает окно выбора кошелька и запоминает
  // результат. Возвращает { near, accountId }.
  const runConnect = useCallback(async () => {
    if (pendingConnectRef.current) return pendingConnectRef.current;
    setConnecting(true);
    const promise = (async () => {
      try {
        const result = await connectWallet();
        connectorRef.current = result.connector || null;
        setNear(result.near);
        setAccountId(result.accountId);
        accountIdRef.current = result.accountId;
        setAccountRestored(false);
        writeStoredAccount(result.accountId);
        return result;
      } finally {
        setConnecting(false);
        pendingConnectRef.current = null;
      }
    })();
    pendingConnectRef.current = promise;
    return promise;
  }, []);

  // Явное подключение по кнопке.
  const connect = useCallback(async () => {
    await runConnect();
  }, [runConnect]);

  /**
   * Вернуть подписанта для действия, требующего подписи. Если сессии нет
   * (обычный случай при новом заходе) — поднимает окно кошелька.
   * Бросает AccountChangedError, если пользователь подключил другой аккаунт.
   */
  const ensureSigner = useCallback(async () => {
    if (near) return near;
    const previous = accountIdRef.current;
    const result = await runConnect();
    if (previous && result.accountId !== previous) {
      throw new AccountChangedError(previous, result.accountId);
    }
    return result.near;
  }, [near, runConnect]);

  // Полное отключение — только по явному действию пользователя.
  const disconnect = useCallback(() => {
    try {
      connectorRef.current?.signOut?.();
    } catch {
      /* библиотека могла не поддерживать signOut — на нашей стороне всё равно чистим */
    }
    connectorRef.current = null;
    pendingConnectRef.current = null;
    setNear(null);
    setAccountId(null);
    accountIdRef.current = null;
    setAccountRestored(false);
    writeStoredAccount(null);
    setRefreshKey((k) => k + 1);
  }, []);

  const refresh = useCallback(() => setRefreshKey((k) => k + 1), []);

  return (
    <WalletContext.Provider
      value={{
        accountId,
        near,
        accountRestored,
        connecting,
        connect,
        ensureSigner,
        disconnect,
        refreshKey,
        refresh,
        isAdmin,
      }}
    >
      {children}
    </WalletContext.Provider>
  );
}

export function useWallet() {
  const ctx = useContext(WalletContext);
  if (!ctx) throw new Error("useWallet must be used within WalletProvider");
  return ctx;
}
