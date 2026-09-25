import { ChannelType, type Client } from "discord.js";
import { getInstallations, getStatus } from "../lib/status.js";
import { ids } from "../store.js";
import { log } from "../lib/logger.js";

/**
 * How often the status voice channels are refreshed.
 *
 * Renaming a channel is rate-limited hard by Discord - roughly two changes per
 * ten minutes per channel - so this stays well clear of that, and each channel
 * is only renamed when its value actually changed. The numbers themselves move
 * slowly (a release, a download tick), so a quarter-hour is plenty.
 */
const INTERVAL_MS = 15 * 60 * 1000;

async function renameIfChanged(client: Client, channelId: string | undefined, name: string): Promise<void> {
  if (!channelId) return;
  const channel = await client.channels.fetch(channelId).catch(() => null);
  if (channel?.type === ChannelType.GuildVoice && channel.name !== name) {
    await channel.setName(name).catch((error) => log.warn(`couldn't rename a status channel: ${error instanceof Error ? error.message : String(error)}`));
  }
}

/** Writes the current version, download count and active installations - yesterday's
 *  and last week's - into the STATUS voice channels' names. */
export async function updateStatusChannels(client: Client): Promise<void> {
  const [status, installations] = await Promise.all([getStatus(), getInstallations()]);
  await renameIfChanged(client, ids.channel("status-version"), `🏷️ Wersja: ${status.version}`);
  await renameIfChanged(client, ids.channel("status-downloads"), `📥 Pobrania: ${status.downloads.toLocaleString("pl-PL")}`);
  // Null means the backend didn't answer, or has no whole week yet: leave the
  // channel on its last real number (or its "..." placeholder).
  if (installations.daily !== null) {
    await renameIfChanged(client, ids.channel("status-installs"), `💻 Aktywne wczoraj: ${installations.daily.toLocaleString("pl-PL")}`);
  }
  if (installations.weekly !== null) {
    await renameIfChanged(client, ids.channel("status-installs-week"), `📅 Aktywne w tygodniu: ${installations.weekly.toLocaleString("pl-PL")}`);
  }
}

/** Starts the periodic refresh - once on boot, then on the interval. */
export function startStatusUpdater(client: Client): void {
  const tick = () => void updateStatusChannels(client).catch((error) => log.error("status update failed", error));
  tick();
  setInterval(tick, INTERVAL_MS);
}
