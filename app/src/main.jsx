import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import './index.css'
import App from './App.jsx'

// Telegram Mini App: синхронизируем реальную высоту листа (viewportStableHeight)
// в CSS-переменную — но ТОЛЬКО на iOS. Изначальная проблема (свёрнутый
// наполовину лист, уезжающий навбар) была специфична именно для iPhone;
// применение той же логики на Android вызвало новые глюки верстки. Поэтому
// сузили фикс строго под платформу через tg.platform, вместо общего для всех
// решения. На Android (и остальных платформах) ничего не меняем — работает
// прежнее, месяцами проверенное поведение через 100svh (см. App.css/index.css).
function syncTelegramViewport() {
  const tg = window.Telegram?.WebApp;
  if (!tg) return;
  const h = tg.viewportStableHeight || tg.viewportHeight;
  if (h) document.documentElement.style.setProperty('--tg-vh', `${h}px`);
}

const tg = window.Telegram?.WebApp;
if (tg) {
  tg.ready();
  if (tg.platform === 'ios') {
    tg.disableVerticalSwipes?.();
    syncTelegramViewport();
    tg.onEvent('viewportChanged', syncTelegramViewport);
    window.addEventListener('resize', syncTelegramViewport);
  }
}

createRoot(document.getElementById('root')).render(
  <StrictMode>
    <App />
  </StrictMode>,
)
