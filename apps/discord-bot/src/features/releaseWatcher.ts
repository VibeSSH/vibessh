import { MessageFlags, type Client, type GuildTextBasedChannel } from "discord.js";
import { fetchLiveVersion } from "../lib/status.js";
import { buildReleasePanel } from "./release.js";
import { ids } from "../store.js";
import { log } from "../lib/logger.js";

/** How often the public releases are checked for a version newer than the last
 * one announced. Cheap (one GitHub request) and well under the unauthenticated
 * rate limit even at this cadence. */
const INTERVAL_MS = 5 * 60 * 1000;

/** Where the last-announced version is remembered, so a restart or a redeploy
 * never re-announces a release people already have. */
const STATE_KEY = "release:last-announced";

/** Records a version as announced, so the watcher will not announce it again.
 * Called by the manual `/release` command too, so announcing by hand and the
 * automatic check stay in agreement. */
export function markReleaseAnnounced(version: string): void {
  ids.setValue(STATE_KEY, version);
}

async function tick(client: Client): Promise<void> {
  const version = await fetchLiveVersion();
  if (!version) return; // No live version this time; try again next tick.

  const announced = ids.value(STATE_KEY);
  if (!announced) {
    // First run on this server: adopt the current version as the baseline
    // without announcing, so the bot never fires a notification for a release
    // that was already out (or already announced by hand) when it started.
    markReleaseAnnounced(version);
    log.info(`release watcher baseline set to ${version}`);
    return;
  }
  if (version === announced) return;

  const channelId = ids.channel("releases");
  const channel = channelId ? await client.channels.fetch(channelId).catch(() => null) : null;
  if (!channel?.isTextBased()) {
    log.warn(`version ${version} is out but there is no #releases channel to announce it in - run /setup`);
    return;
  }

  const roleId = ids.role("notify-release");
  try {
    await (channel as GuildTextBasedChannel).send({
      components: [buildReleasePanel(version, roleId)],
      flags: MessageFlags.IsComponentsV2,
      allowedMentions: { roles: roleId ? [roleId] : [] },
    });
    // Only recorded once the message is actually sent, so a failed post is
    // retried on the next tick rather than silently skipped.
    markReleaseAnnounced(version);
    log.info(`announced release ${version} in #releases`);
  } catch (error) {
    log.error("automatic release announcement failed", error);
  }
}

/**
 * Starts the release watcher: once on boot, then every five minutes. It is the
 * automatic half of `/release` - the command stays for re-posting an
 * announcement by hand.
 */
export function startReleaseWatcher(client: Client): void {
  const run = () => void tick(client).catch((error) => log.error("release watcher failed", error));
  run();
  setInterval(run, INTERVAL_MS);
}
