import { ActivityType, type Client } from "discord.js";
import { getStatus } from "../lib/status.js";
import { log } from "../lib/logger.js";

/** How often the shown status rotates. */
const ROTATE_MS = 30_000;
/** How often the version-bearing line is refreshed. */
const REFRESH_MS = 15 * 60 * 1000;

/**
 * The rotating custom status, in both languages.
 *
 * A mix of Polish and English lines cycled every half-minute, so the bot's
 * presence reads to everyone. One line carries the live version from
 * `getStatus`, refreshed on its own slower timer.
 */
function lines(version: string): string[] {
  return [
    "🛠️ Pracuję nad nową wersją",
    "🛠️ Working on a new version",
    "🔒 Zarządzam serwerami przez SSH",
    "🔒 Managing servers over SSH",
    "🎫 /setup · tickety · releases",
    `📦 VibeSSH ${version}`,
    "🌐 vibessh.dev",
  ];
}

export function startPresence(client: Client): void {
  let current = lines("");
  let index = 0;

  const refresh = async () => {
    try {
      const { version } = await getStatus();
      current = lines(version);
    } catch (error) {
      log.warn(`presence refresh failed: ${error instanceof Error ? error.message : String(error)}`);
    }
  };

  const tick = () => {
    const text = current[index % current.length] ?? "vibessh.dev";
    index += 1;
    client.user?.setPresence({
      status: "online",
      activities: [{ name: text, type: ActivityType.Custom, state: text }],
    });
  };

  void refresh().then(tick);
  setInterval(tick, ROTATE_MS);
  setInterval(() => void refresh(), REFRESH_MS);
}
