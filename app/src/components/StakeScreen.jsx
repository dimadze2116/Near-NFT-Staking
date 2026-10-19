import { useEffect, useLayoutEffect, useRef, useState } from "react";
import Header from "./Header";
import ScrollBody from "./ScrollBody";
import MyStakesBody from "./MyStakesBody";
import LeaderboardBody from "./LeaderboardBody";
import DetailsBody from "./DetailsBody";
import AdminBody from "./AdminBody";
import MarketBody from "./MarketBody";
import CartModal from "./CartModal";
import StakingPanel from "./StakingPanel";
import ButtonNftStake from "./ButtonNftStake";
import MyRankBar from "./MyRankBar";
import SidebarNav from "./SidebarNav";
import Toast from "./Toast";
import SellModal from "./SellModal";
import { useLanguage } from "../i18n";
import { useWallet, AccountChangedError } from "../WalletContext";
import { getNftsForOwner, getStakesForAccount, previewReward, stakeNfts, unstakeNft, getMyRank, verifyWebapp, yoctoToNear, listNftForSale, nearToYocto, buyListings, notifySale } from "../near";
import "./StakeScreen.css";

function buildTabOrder(isAdmin) {
  const base = ["stake", "mystakes", "market", "details", "leaderboard"];
  return isAdmin ? [...base, "admin"] : base;
}

function fmtCountdown(unlocksAt) {
  const s = Math.max(0, Math.floor(unlocksAt - Date.now() / 1000));
  const d = Math.floor(s / 86400);
  const h = Math.floor((s % 86400) / 3600);
  const m = Math.floor((s % 3600) / 60);
  return `${d}d ${h}h ${m}m`;
}
const SWAP_MS = 320;
// Порог свайпа между вкладками и допуск, после которого жест считается
// горизонтальным (и вертикальный скролл блокируется до конца касания)
const SWIPE_THRESHOLD = 60;
const SWIPE_LOCK_DISTANCE = 10;
// Длительность ухода флоу: анимация 0.3s + макс. задержка стаггера 0.16s + запас
const FLOW_EXIT_MS = 480;
// Потолок позиций в корзине — жёстко ограничен газом контракта (MAX_CART_ITEMS)
const MAX_CART_ITEMS = 4;
// stake-flow-disappear 0.3s — столько модалка проигрывает уход перед размонтированием
const MODAL_EXIT_MS = 300;
// Длительность анимации ухода кнопки Select All
const SELECT_ALL_EXIT_MS = 300;
// Длительность анимации ухода всплывающей плашки (см. toast-disappear)
const TOAST_EXIT_MS = 240;


export default function StakeScreen() {
  const { t } = useLanguage();
  const { accountId, accountRestored, connect, ensureSigner, refresh, refreshKey, isAdmin } = useWallet();
  const TAB_ORDER = buildTabOrder(isAdmin);
  const [activeTab, setActiveTab] = useState("stake");
  const [displayedTab, setDisplayedTab] = useState("stake");
  const [nfts, setNfts] = useState([]);
  const [selectedPeriod, setSelectedPeriod] = useState("7");
  const [currentActiveCount, setCurrentActiveCount] = useState(0);
  const [myStakes, setMyStakes] = useState([]);
  const [claiming, setClaiming] = useState(false);
  const [previewAmount, setPreviewAmount] = useState(null);
  const [staking, setStaking] = useState(false);
  const [sellingNft, setSellingNft] = useState(null);
  const [listingSubmitting, setListingSubmitting] = useState(false);
  const [cart, setCart] = useState(new Map());
  const [cartOpen, setCartOpen] = useState(false);
  const [buyingCart, setBuyingCart] = useState(false);
  // Выбор NFT поднят сюда: сетка (ScrollBody) вверху, а блок периода + кнопка
  // стейка внизу зависят от него — появляются, когда выбрана хотя бы одна NFT
  const [selectedNfts, setSelectedNfts] = useState(() => new Set());
  // Снимок выбора до нажатия "Select All" — повторное нажатие возвращает как было
  const [preSelectAllNfts, setPreSelectAllNfts] = useState(null);

  // Всплывающая плашка результата. exiting — фаза проигрывания анимации ухода
  // перед размонтированием. stakeAttemptRef чередует результат стейка:
  // 1-е нажатие — успех, 2-е — неудача, и т.д. (временно, для примера)
  const [toast, setToast] = useState(null);
  const [toastExiting, setToastExiting] = useState(false);
  const stakeAttemptRef = useRef(0);

  const showToast = (next) => {
    setToastExiting(false);
    setToast(next);
  };

  const dismissToast = () => {
    setToastExiting(true);
    setTimeout(() => {
      setToast(null);
      setToastExiting(false);
    }, TOAST_EXIT_MS);
  };

  // Верификация с ботом (доступ в канал, статус) — один раз на новый accountId,
  // не повторяется при обычных refresh() после стейка/анстейка.
  const verifiedAccountRef = useRef(null);
  useEffect(() => {
    if (!accountId || verifiedAccountRef.current === accountId) return;
    verifiedAccountRef.current = accountId;
    verifyWebapp(accountId).then((result) => {
      if (result.ok) {
        // При восстановленном аккаунте верификация проходит фоном на каждом
        // открытии приложения — пользователь ничего не нажимал, тост об успехе
        // был бы шумом. Сообщения об ОТКАЗЕ показываем всегда: потеря доступа
        // к каналу — это то, о чём человек должен узнать.
        if (accountRestored) return;
        showToast({ variant: "success", title: t.toastVerifySuccessTitle ?? "Подтверждено", text: t.toastVerifySuccessText ?? "Доступ к каналу выдан" });
      } else if (result.reason === "no_nft") {
        showToast({ variant: "error", title: t.toastVerifyFailTitle ?? "Нет доступа", text: t.toastVerifyNoNftText ?? "На этом кошельке нет NFT/стейка" });
      } else if (result.reason !== "no_telegram") {
        showToast({ variant: "error", title: t.toastVerifyFailTitle ?? "Ошибка", text: t.toastVerifyFailText ?? "Верификация не прошла" });
      }
      // reason === "no_telegram" — открыто не из Telegram, не показываем ошибку
    });
  }, [accountId]);

  // Загрузка реальных NFT и количества уже активных стейков при подключении
  // кошелька (и после каждого refresh() — стейк/анстейк).
  useEffect(() => {
    if (!accountId) {
      setNfts([]);
      setCurrentActiveCount(0);
      setMyStakes([]);
      return;
    }
    let cancelled = false;
    (async () => {
      try {
        const [ownedNfts, stakes] = await Promise.all([
          getNftsForOwner(accountId),
          getStakesForAccount(accountId),
        ]);
        if (cancelled) return;
        setNfts(ownedNfts);
        const active = stakes.filter((s) => !s.claimed);
        setCurrentActiveCount(active.length);
        setMyStakes(active);
      } catch (e) {
        console.error("[stake] load nfts/stakes failed", e);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [accountId, refreshKey]);

  // Предпросмотр награды — пересчитывается при смене выбора NFT или периода
  useEffect(() => {
    if (selectedNfts.size === 0) {
      setPreviewAmount(null);
      return;
    }
    let cancelled = false;
    (async () => {
      try {
        const reward = await previewReward(Number(selectedPeriod), currentActiveCount, selectedNfts.size);
        if (!cancelled) setPreviewAmount(yoctoToNear(reward));
      } catch (e) {
        console.error("[stake] preview_reward failed", e);
        if (!cancelled) setPreviewAmount(null);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [selectedNfts.size, selectedPeriod, currentActiveCount]);

  function handleOpenSell(nft) {
    setSellingNft(nft);
  }

  async function handleConfirmSell(priceStr) {
    if (!sellingNft || listingSubmitting) return;
    const priceYocto = nearToYocto(priceStr);
    if (!priceYocto) {
      showToast({ variant: "error", title: t.toastListFailTitle, text: t.sellInvalidPrice });
      return;
    }
    setListingSubmitting(true);
    try {
      const signer = await ensureSigner();
      await listNftForSale(signer, accountId, sellingNft.id, priceYocto);
      showToast({ variant: "success", title: t.toastListSuccessTitle, text: t.toastListSuccessText });
      setSellingNft(null);
      refresh();
    } catch (e) {
      if (e instanceof AccountChangedError) {
        setSellingNft(null);
        showToast({ variant: "error", title: t.toastAccountChangedTitle, text: t.toastAccountChangedText });
        refresh();
        return;
      }
      console.error("[sell] listing failed", e);
      showToast({ variant: "error", title: t.toastListFailTitle, text: e.message || t.toastListFailText });
    } finally {
      setListingSubmitting(false);
    }
  }

  function handleToggleCart(item) {
    // Контракт принимает максимум MAX_CART_ITEMS позиций за покупку: на каждую
    // уходит 60 TGas, при потолке 300 больше четырёх не помещается. Раньше лимита
    // не было, и пользователь узнавал об этом невнятной ошибкой про газ уже
    // после подписания транзакции.
    if (!cart.has(item.token_id) && cart.size >= MAX_CART_ITEMS) {
      showToast({
        variant: "error",
        title: t.toastCartFullTitle,
        text: t.toastCartFullText,
      });
      return;
    }
    setCart((prev) => {
      const next = new Map(prev);
      if (next.has(item.token_id)) next.delete(item.token_id);
      else next.set(item.token_id, item);
      return next;
    });
  }

  function handleRemoveFromCart(tokenId) {
    setCart((prev) => {
      const next = new Map(prev);
      next.delete(tokenId);
      return next;
    });
  }

  async function handleBuyAll(items, totalYocto) {
    if (buyingCart || items.length === 0) return;
    setBuyingCart(true);
    try {
      const tokenIds = items.map((item) => item.token_id);
      const signer = await ensureSigner();
      await buyListings(signer, accountId, tokenIds, totalYocto);
      showToast({ variant: "success", title: t.toastBuySuccessTitle, text: t.toastBuySuccessText });
      setCart(new Map());
      setCartOpen(false);
      refresh();
      notifySale(accountId, items).catch(() => {});
    } catch (e) {
      if (e instanceof AccountChangedError) {
        setCartOpen(false);
        showToast({ variant: "error", title: t.toastAccountChangedTitle, text: t.toastAccountChangedText });
        refresh();
        return;
      }
      console.error("[market] purchase failed", e);
      showToast({ variant: "error", title: t.toastBuyFailTitle, text: e.message || t.toastBuyFailText });
    } finally {
      setBuyingCart(false);
    }
  }

  const handleStakeClick = async () => {
    if (!accountId) {
      connect().catch(() => {});
      return;
    }
    if (staking || selectedNfts.size === 0) return;
    setStaking(true);
    try {
      const tokenIds = [...selectedNfts].map(String);
      const signer = await ensureSigner();
      // Контракт при пустом пуле НЕ выбрасывает ошибку — он просто молча
      // возвращает NFT владельцу (транзакция технически успешна). Поэтому
      // единственный надёжный способ узнать реальный результат — сравнить,
      // что было активно ДО и стало ПОСЛЕ отправки транзакции.
      const before = await getStakesForAccount(accountId);
      const beforeIds = new Set(before.filter((s) => !s.claimed).map((s) => s.token_id));

      await stakeNfts(signer, accountId, tokenIds, Number(selectedPeriod));

      await new Promise((r) => setTimeout(r, 3000));
      const after = await getStakesForAccount(accountId);
      const afterIds = new Set(after.filter((s) => !s.claimed).map((s) => s.token_id));
      const accepted = tokenIds.filter((id) => !beforeIds.has(id) && afterIds.has(id));

      if (accepted.length === tokenIds.length) {
        showToast({ variant: "success", title: t.toastStakeSuccessTitle, text: t.toastStakeSuccessText });
        setSelectedNfts(new Set());
      } else if (accepted.length > 0) {
        showToast({ variant: "error", title: t.toastStakePartialTitle, text: t.toastStakePartialText });
        setSelectedNfts(new Set(tokenIds.filter((id) => !accepted.includes(id))));
      } else {
        showToast({ variant: "error", title: t.toastStakePoolEmptyTitle, text: t.toastStakePoolEmptyText });
      }
      refresh();
    } catch (e) {
      if (e instanceof AccountChangedError) {
        setSelectedNfts(new Set());
        showToast({ variant: "error", title: t.toastAccountChangedTitle, text: t.toastAccountChangedText });
        refresh();
        return;
      }
      console.error("[stake] stake failed", e);
      showToast({ variant: "error", title: t.toastStakeFailTitle, text: e.message || t.toastStakeFailText });
    } finally {
      setStaking(false);
    }
  };

  const claimableStakes = myStakes.filter((s) => s.is_mature);
  const claimableTotal = claimableStakes.reduce((sum, s) => sum + BigInt(s.reward_near || "0"), 0n);
  const nextUnlock = myStakes
    .filter((s) => !s.is_mature)
    .reduce((min, s) => (min == null || s.unlocks_at < min ? s.unlocks_at : min), null);

  const handleClaimClick = async () => {
    if (claiming || claimableStakes.length === 0) return;
    setClaiming(true);
    try {
      const signer = await ensureSigner();
      for (const s of claimableStakes) {
        await unstakeNft(signer, s.token_id);
      }
      showToast({
        variant: "success",
        title: t.toastClaimTitle,
        text: t.toastClaimText(yoctoToNear(claimableTotal.toString())),
      });
      refresh();
    } catch (e) {
      if (e instanceof AccountChangedError) {
        showToast({ variant: "error", title: t.toastAccountChangedTitle, text: t.toastAccountChangedText });
        refresh();
        return;
      }
      console.error("[claim] failed", e);
      showToast({ variant: "error", title: t.toastStakeFailTitle, text: e.message || t.toastStakeFailText });
    } finally {
      setClaiming(false);
    }
  };

  const toggleNft = (id) => {
    setPreSelectAllNfts(null);
    setSelectedNfts((prev) => {
      const next = new Set(prev);
      next.has(id) ? next.delete(id) : next.add(id);
      return next;
    });
  };

  const handleSelectAllClick = () => {
    if (preSelectAllNfts) {
      setSelectedNfts(preSelectAllNfts);
      setPreSelectAllNfts(null);
    } else {
      setPreSelectAllNfts(selectedNfts);
      setSelectedNfts(new Set(nfts.map((nft) => nft.id)));
    }
  };

  const renderTab = (tab) => {
    if (tab === "stake") return <ScrollBody nfts={nfts} selected={selectedNfts} onToggle={toggleNft} onSell={handleOpenSell} />;
    if (tab === "mystakes") return <MyStakesBody />;
    if (tab === "market")
      return <MarketBody cart={cart} onToggleCart={handleToggleCart} showToast={showToast} />;
    if (tab === "leaderboard") return <LeaderboardBody youRowRef={youRowRef} flashYou={flashYou} />;
    if (tab === "details") return <DetailsBody />;
    if (tab === "admin") return <AdminBody />;
    return null;
  };
  // Кроссфейд: старый и новый экран анимируются одновременно, без пустого кадра.
  // dir = куда уезжает старый контент; scroll — чтобы старый экран не прыгал
  // к верху при сбросе скролла
  const [swap, setSwap] = useState(null);
  const scrollRef = useRef(null);
  // Нижнюю группу нельзя якорить через `bottom`: в Chromium это ломает
  // backdrop-filter у её потомков над скролл-слоем. Поэтому держим её на
  // `top`, вычисляя его как (высота экрана − высота группы − 30px).
  const bottomRef = useRef(null);
  const [bottomTop, setBottomTop] = useState(null);

  // Свайп влево/вправо между вкладками (мобильные тач-жесты). Слушатели
  // нативные (не React-синтетика): touchmove нужен non-passive, чтобы
  // preventDefault останавливал вертикальный скролл на время горизонтального
  // свайпа — с passive-обработчиком браузер это игнорирует.
  useEffect(() => {
    const el = scrollRef.current;
    if (!el) return;

    const touch = { x: 0, y: 0, locked: null };

    const onTouchStart = (e) => {
      const t = e.touches[0];
      touch.x = t.clientX;
      touch.y = t.clientY;
      touch.locked = null;
    };

    const onTouchMove = (e) => {
      const t = e.touches[0];
      const dx = t.clientX - touch.x;
      const dy = t.clientY - touch.y;
      if (touch.locked === null && (Math.abs(dx) > SWIPE_LOCK_DISTANCE || Math.abs(dy) > SWIPE_LOCK_DISTANCE)) {
        touch.locked = Math.abs(dx) > Math.abs(dy) ? "x" : "y";
      }
      if (touch.locked === "x") e.preventDefault();
    };

    const onTouchEnd = (e) => {
      if (touch.locked === "x") {
        const t = e.changedTouches[0];
        const dx = t.clientX - touch.x;
        if (Math.abs(dx) >= SWIPE_THRESHOLD) {
          const idx = TAB_ORDER.indexOf(activeTab);
          if (dx < 0 && idx < TAB_ORDER.length - 1) setActiveTab(TAB_ORDER[idx + 1]);
          else if (dx > 0 && idx > 0) setActiveTab(TAB_ORDER[idx - 1]);
        }
      }
      touch.locked = null;
    };

    el.addEventListener("touchstart", onTouchStart, { passive: true });
    el.addEventListener("touchmove", onTouchMove, { passive: false });
    el.addEventListener("touchend", onTouchEnd);
    return () => {
      el.removeEventListener("touchstart", onTouchStart);
      el.removeEventListener("touchmove", onTouchMove);
      el.removeEventListener("touchend", onTouchEnd);
    };
  }, [activeTab]);

  // Клик по плашке "мой ранг": если своя строка уже видна — вспышка сразу;
  // если нет — сперва скролл к ней, вспышка запускается уже после прокрутки
  const youRowRef = useRef(null);
  const [flashYou, setFlashYou] = useState(false);
  const [myRank, setMyRank] = useState(null);

  useEffect(() => {
    if (!accountId) {
      setMyRank(null);
      return;
    }
    let cancelled = false;
    getMyRank(accountId)
      .then((r) => !cancelled && setMyRank(r))
      .catch(() => !cancelled && setMyRank(null));
    return () => {
      cancelled = true;
    };
  }, [accountId, refreshKey]);
  const handleShowMyRank = () => {
    const rowEl = youRowRef.current;
    const scrollEl = scrollRef.current;
    if (!rowEl) return;

    const flash = () => {
      setFlashYou(true);
      setTimeout(() => setFlashYou(false), 800);
    };

    const rowRect = rowEl.getBoundingClientRect();
    const scrollRect = scrollEl?.getBoundingClientRect();
    const inView =
      !!scrollRect && rowRect.top >= scrollRect.top && rowRect.bottom <= scrollRect.bottom;

    if (inView) {
      flash();
      return;
    }

    let done = false;
    const finish = () => {
      if (done) return;
      done = true;
      scrollEl?.removeEventListener("scrollend", finish);
      flash();
    };
    scrollEl?.addEventListener("scrollend", finish);
    // подстраховка на случай, если scrollend не поддерживается браузером
    setTimeout(finish, 500);
    rowEl.scrollIntoView({ behavior: "smooth", block: "center" });
  };

  const onStakeTab = activeTab === "stake";
  const showStakeFlow = onStakeTab && selectedNfts.size > 0;
  const showSelectAll =
    onStakeTab && (preSelectAllNfts != null || (selectedNfts.size >= 2 && selectedNfts.size < nfts.length));

  // Фазы жизни нижнего флоу, чтобы проиграть анимацию ухода перед размонтированием
  const [flowPhase, setFlowPhase] = useState(showStakeFlow ? "shown" : "hidden");

  useEffect(() => {
    if (showStakeFlow) {
      setFlowPhase("shown");
    } else if (!onStakeTab) {
      // Уход с вкладки Stake целиком — кросс-фейд вкладок уже скрывает переход,
      // поэтому без анимации ухода флоу: иначе он ещё 480ms остаётся в DOM
      // рядом с контролами новой вкладки (например Claim Rewards), раздувая
      // общую высоту нижней группы и сбивая её позицию (визуальный "прыжок")
      setFlowPhase("hidden");
    } else {
      setFlowPhase((p) => (p === "hidden" ? "hidden" : "exiting"));
    }
  }, [showStakeFlow, onStakeTab]);

  useEffect(() => {
    if (flowPhase !== "exiting") return;
    const t = setTimeout(() => setFlowPhase("hidden"), FLOW_EXIT_MS);
    return () => clearTimeout(t);
  }, [flowPhase]);

  const renderFlow = flowPhase !== "hidden";
  const flowExiting = flowPhase === "exiting";

  // Модалки живут над навбаром и уходят с анимацией (stake-flow-disappear, 0.3s),
  // поэтому размонтируем их не сразу, а после проигрывания — как флоу выше.
  const [cartPhase, setCartPhase] = useState("hidden");

  useEffect(() => {
    if (cartOpen) setCartPhase("shown");
    else setCartPhase((prev) => (prev === "hidden" ? "hidden" : "exiting"));
  }, [cartOpen]);

  useEffect(() => {
    if (cartPhase !== "exiting") return;
    const t = setTimeout(() => setCartPhase("hidden"), MODAL_EXIT_MS);
    return () => clearTimeout(t);
  }, [cartPhase]);

  const renderCart = cartPhase !== "hidden";
  const cartExiting = cartPhase === "exiting";

  // Для модалки продажи нужны данные NFT и во время ухода, поэтому держим
  // отдельный снимок: sellingNft уже null, а показывать ещё есть что.
  const [sellPhase, setSellPhase] = useState("hidden");
  const [sellNftShown, setSellNftShown] = useState(null);

  useEffect(() => {
    if (sellingNft) {
      setSellNftShown(sellingNft);
      setSellPhase("shown");
    } else {
      setSellPhase((prev) => (prev === "hidden" ? "hidden" : "exiting"));
    }
  }, [sellingNft]);

  useEffect(() => {
    if (sellPhase !== "exiting") return;
    const t = setTimeout(() => {
      setSellPhase("hidden");
      setSellNftShown(null);
    }, MODAL_EXIT_MS);
    return () => clearTimeout(t);
  }, [sellPhase]);

  const renderSell = sellPhase !== "hidden";
  const sellExiting = sellPhase === "exiting";

  // Та же схема фаз — для анимации ухода кнопки Select All перед размонтированием
  const [selectAllPhase, setSelectAllPhase] = useState(showSelectAll ? "shown" : "hidden");

  useEffect(() => {
    if (showSelectAll) {
      setSelectAllPhase("shown");
    } else if (!onStakeTab) {
      // Та же причина, что и у флоу выше — не тянуть анимацию ухода на чужую вкладку
      setSelectAllPhase("hidden");
    } else {
      setSelectAllPhase((p) => (p === "hidden" ? "hidden" : "exiting"));
    }
  }, [showSelectAll, onStakeTab]);

  useEffect(() => {
    if (selectAllPhase !== "exiting") return;
    const t = setTimeout(() => setSelectAllPhase("hidden"), SELECT_ALL_EXIT_MS);
    return () => clearTimeout(t);
  }, [selectAllPhase]);

  const renderSelectAll = selectAllPhase !== "hidden";
  const selectAllExiting = selectAllPhase === "exiting";

  useEffect(() => {
    if (activeTab === displayedTab) return;

    const dir =
      TAB_ORDER.indexOf(activeTab) > TAB_ORDER.indexOf(displayedTab) ? "left" : "right";
    const scroll = scrollRef.current ? scrollRef.current.scrollTop : 0;
    setSwap({ from: displayedTab, dir, scroll });
    setDisplayedTab(activeTab);
    if (scrollRef.current) scrollRef.current.scrollTop = 0;
  }, [activeTab, displayedTab]);

  useEffect(() => {
    if (!swap) return;
    const t = setTimeout(() => setSwap(null), SWAP_MS);
    return () => clearTimeout(t);
  }, [swap]);

  useLayoutEffect(() => {
    const el = bottomRef.current;
    const screen = el?.parentElement;
    if (!el || !screen) return;
    const compute = () => setBottomTop(screen.clientHeight - el.offsetHeight - 30);
    // Пересчёт синхронно, до отрисовки кадра: при переключении вкладки высота
    // группы меняется (например появляется/исчезает Claim Rewards) в тот же
    // рендер, что и activeTab — если ждать только ResizeObserver (он всегда
    // асинхронный, на кадр позже), браузer успевает отрисовать один кадр со
    // старым top и новой высотой контента, и навбар визуально "прыгает"
    compute();
    const ro = new ResizeObserver(compute);
    ro.observe(el);
    ro.observe(screen);
    return () => ro.disconnect();
  }, [renderFlow, activeTab, renderCart, renderSell]);

  let scrollMod = "";
  if (renderCart || renderSell) scrollMod = " stake-screen__scroll--cart";
  else if (activeTab === "leaderboard") scrollMod = " stake-screen__scroll--rank";
  else if (!onStakeTab) scrollMod = " stake-screen__scroll--compact";
  else if (renderFlow) scrollMod = " stake-screen__scroll--flow";

  return (
    <div className="stake-screen">
      <span className="stake-screen__scrim" aria-hidden="true" />
      <div ref={scrollRef} className={`stake-screen__scroll${scrollMod}`}>
        <div className="stake-screen__stack">
          {swap && (
            <div
              key={`out-${swap.from}`}
              className={`stake-screen__content stake-screen__content--out-${swap.dir}`}
              style={{ marginTop: -swap.scroll }}
              aria-hidden="true"
            >
              {renderTab(swap.from)}
            </div>
          )}
          <div
            key={displayedTab}
            className={`stake-screen__content${swap ? ` stake-screen__content--in-${swap.dir}` : ""}`}
          >
            {renderTab(displayedTab)}
          </div>
        </div>
      </div>
      <div className="stake-screen__header">
        <Header />
      </div>
      <div
        className="stake-screen__bottom"
        ref={bottomRef}
        style={bottomTop != null ? { top: `${bottomTop}px`, bottom: "auto" } : undefined}
      >
        {renderFlow && (
          <div className="stake-screen__periods-wrap">
            {renderSelectAll && (
              <div className="stake-screen__select-all">
                <ButtonNftStake
                  compact
                  slideIn
                  exiting={selectAllExiting}
                  label={preSelectAllNfts != null ? t.deselectAll : t.selectAll}
                  onClick={handleSelectAllClick}
                />
              </div>
            )}
            <StakingPanel exiting={flowExiting} period={selectedPeriod} onPeriodChange={setSelectedPeriod} />
          </div>
        )}
        {renderFlow && (
          <ButtonNftStake
            exiting={flowExiting}
            amount={previewAmount}
            label={staking ? t.staking ?? "..." : undefined}
            disabled={staking}
            onClick={handleStakeClick}
          />
        )}
        {activeTab === "mystakes" &&
          (claimableStakes.length > 0 ? (
            <ButtonNftStake
              label={claiming ? t.staking ?? "..." : t.claimFor(yoctoToNear(claimableTotal.toString()))}
              withNearIcon={!claiming}
              disabled={claiming}
              onClick={handleClaimClick}
            />
          ) : (
            <ButtonNftStake
              label={t.claimRewards}
              timer={nextUnlock != null ? t.claimTimer(fmtCountdown(nextUnlock)) : undefined}
              disabled
            />
          ))}
        {activeTab === "market" && cart.size > 0 && !renderCart && (
          <ButtonNftStake
            label={`${t.marketViewCart} (${cart.size}) · ${yoctoToNear(
              Array.from(cart.values()).reduce((sum, item) => sum + BigInt(item.price), 0n).toString()
            )}`}
            withNearIcon
            onClick={() => setCartOpen(true)}
          />
        )}
        {activeTab === "leaderboard" && myRank && (
          <MyRankBar
            position={myRank.position}
            name={myRank.owner}
            nftCount={`${myRank.nft_count} NFT`}
            reward={`${yoctoToNear(myRank.total_reward)} Near`}
            onClick={handleShowMyRank}
          />
        )}
        {renderSell && (
          <SellModal
            nft={sellNftShown}
            submitting={listingSubmitting}
            exiting={sellExiting}
            onConfirm={handleConfirmSell}
            onClose={() => setSellingNft(null)}
          />
        )}
        {renderCart && (
          <CartModal
            cart={cart}
            submitting={buyingCart}
            exiting={cartExiting}
            onRemove={handleRemoveFromCart}
            onBuyAll={handleBuyAll}
            onClose={() => setCartOpen(false)}
          />
        )}
        <SidebarNav active={activeTab} onChange={setActiveTab} isAdmin={isAdmin} />
      </div>
      {toast && (
        <Toast
          variant={toast.variant}
          title={toast.title}
          text={toast.text}
          exiting={toastExiting}
          onClose={dismissToast}
        />
      )}
    </div>
  );
}
