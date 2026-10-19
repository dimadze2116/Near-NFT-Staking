import cron from 'node-cron';
import { getAllUsersInChannel, isMatureStakeNotified, markMatureStakeNotified } from './db';
import { checkNftOwnership, getStakesForAccount } from './near';

/** yoctoNEAR -> "1.2345" (4 знака после запятой) — для текста уведомления. */
function yoctoToNear(v: string): string {
  try {
    const n = BigInt(v || '0');
    const d = 1_000_000_000_000_000_000_000_000n;
    const whole = n / d;
    const frac = (n % d).toString().padStart(24, '0').substring(0, 4);
    return `${whole}.${frac}`;
  } catch {
    return '0';
  }
}

export function startCron() {
  cron.schedule('0 8,20 * * *', async () => {
    console.log('[CRON] Запуск проверки NFT у участников канала...');
    await checkAllMembers();
  });
  console.log('[CRON] Задачи запущены (08:00 и 20:00 ежедневно)');
}

async function checkAllMembers() {
  const members = getAllUsersInChannel();
  console.log(`[CRON] Проверка ${members.length} участников...`);

  for (const member of members) {
    if (!member.near_account) continue;
    try {
      // FIX 9: при ошибке RPC НЕ кикаем — требуем два независимых подтверждения
      let hasNft = false;
      let nftCheckOk = false;
      let hasActiveStake = false;
      let stakeCheckOk = false;

      try {
        const result = await checkNftOwnership(member.near_account);
        hasNft = result.hasNft;
        nftCheckOk = true;
      } catch (err) {
        console.error(`[CRON] NFT check failed for ${member.telegram_id}:`, err);
      }

      try {
        const stakes = await getStakesForAccount(member.near_account);
        hasActiveStake = Array.isArray(stakes) && stakes.some((s: any) => !s.claimed);
        stakeCheckOk = true;

        // Уведомление о созревших (готовых к клейму) стейках — по каждому
        // конкретному стейку не чаще одного раза (отслеживается в БД).
        if (Array.isArray(stakes)) {
          for (const s of stakes) {
            if (!s.is_mature || s.claimed) continue;
            if (isMatureStakeNotified(s.stake_id)) continue;
            try {
              const { bot } = await import('./bot');
              const rewardNear = yoctoToNear(s.reward_near);
              const text = member.lang === 'en'
                ? `🎉 Your NFT #${s.token_id} is ready to claim!\n\nReward: ${rewardNear} NEAR — claim it in the app.`
                : `🎉 Ваш NFT #${s.token_id} готов к клейму!\n\nНаграда: ${rewardNear} NEAR — заберите её в приложении.`;
              await bot.telegram.sendMessage(member.telegram_id, text);
              markMatureStakeNotified(s.stake_id, member.telegram_id);
            } catch (err) {
              console.error(`[CRON] Notify matured stake failed for ${member.telegram_id}/${s.stake_id}:`, err);
            }
          }
        }
      } catch (err) {
        console.error(`[CRON] Stakes check failed for ${member.telegram_id}:`, err);
      }

      // Кикаем ТОЛЬКО если оба запроса успешны и оба вернули false
      if (nftCheckOk && stakeCheckOk && !hasNft && !hasActiveStake) {
        console.log(`[CRON] NFT пропал у ${member.telegram_id} (${member.near_account}) — кик`);
        const { kickFromChannel } = await import('./bot');
        await kickFromChannel(member.telegram_id);
      } else if (!nftCheckOk || !stakeCheckOk) {
        console.log(`[CRON] Пропускаем кик ${member.telegram_id} — ошибка RPC`);
      }
    } catch (err) {
      console.error(`[CRON] Ошибка проверки ${member.telegram_id}:`, err);
    }
  }
}