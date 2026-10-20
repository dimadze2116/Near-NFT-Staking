import cron from 'node-cron';
import { getAllUsersInChannel, isMatureStakeNotified, markMatureStakeNotified } from './db';
import { checkNftOwnership, getStakesForAccount } from './near';

/** yoctoNEAR -> "1.2345" (4 decimal places) - for the notification text. */
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
    console.log('[CRON] Checking NFT ownership of channel members...');
    await checkAllMembers();
  });
  console.log('[CRON] Jobs scheduled (08:00 and 20:00 daily)');
}

async function checkAllMembers() {
  const members = getAllUsersInChannel();
      console.log(`[CRON] Checking ${members.length} members...`);

  for (const member of members) {
    if (!member.near_account) continue;
    try {
      // FIX 9: on an RPC error do NOT kick - require two independent confirmations
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

        // Notification about matured stakes (ready to claim) - at most once per
        // individual stake (tracked in the database).
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

      // Kick ONLY if both requests succeeded and both returned false
      if (nftCheckOk && stakeCheckOk && !hasNft && !hasActiveStake) {
            console.log(`[CRON] NFT gone for ${member.telegram_id} (${member.near_account}) - kicking`);
        const { kickFromChannel } = await import('./bot');
        await kickFromChannel(member.telegram_id);
      } else if (!nftCheckOk || !stakeCheckOk) {
            console.log(`[CRON] Skipping kick of ${member.telegram_id} - RPC error`);
      }
    } catch (err) {
          console.error(`[CRON] Failed to check ${member.telegram_id}:`, err);
    }
  }
}