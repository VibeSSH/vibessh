import { readFileSync, writeFileSync, existsSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import { log } from "./lib/logger.js";

const HERE = dirname(fileURLToPath(import.meta.url));
const FILE = join(HERE, "..", "config.runtime.json");

/**
 * What the self-setup discovered or created, keyed by our own stable keys
 * (`role.moderator`, `channel.support-pl`, `category.polski`) rather than by
 * Discord's snowflakes - so the rest of the bot refers to "the moderator role"
 * and this file is the one place that knows which snowflake that is on this
 * server. Persisted to disk so a restart does not re-run discovery.
 */
export interface RuntimeConfig {
  /** our role key -> Discord role id */
  roles: Record<string, string>;
  /** our channel key -> Discord channel id */
  channels: Record<string, string>;
  /** our category key -> Discord category (channel) id */
  categories: Record<string, string>;
  /** Arbitrary keyed message ids the bot posts and later edits (panels, status). */
  messages: Record<string, string>;
  /** Arbitrary keyed scalar state the bot remembers between restarts - e.g. the
   * last release version it announced, so it does not announce one twice. */
  values: Record<string, string>;
}

const EMPTY: RuntimeConfig = { roles: {}, channels: {}, categories: {}, messages: {}, values: {} };

let cache: RuntimeConfig | null = null;

export function loadConfig(): RuntimeConfig {
  if (cache) return cache;
  if (!existsSync(FILE)) {
    cache = structuredClone(EMPTY);
    return cache;
  }
  try {
    const parsed = JSON.parse(readFileSync(FILE, "utf8")) as Partial<RuntimeConfig>;
    cache = { ...structuredClone(EMPTY), ...parsed };
    // Guard against a hand-edited file that dropped a whole section.
    cache.roles ??= {};
    cache.channels ??= {};
    cache.categories ??= {};
    cache.messages ??= {};
    cache.values ??= {};
    return cache;
  } catch (error) {
    log.error(`config.runtime.json is unreadable, starting from empty`, error);
    cache = structuredClone(EMPTY);
    return cache;
  }
}

export function saveConfig(config: RuntimeConfig): void {
  cache = config;
  writeFileSync(FILE, JSON.stringify(config, null, 2), "utf8");
}

/** Convenience getters - undefined means "setup has not recorded this yet". */
export const ids = {
  role: (key: string) => loadConfig().roles[key],
  channel: (key: string) => loadConfig().channels[key],
  category: (key: string) => loadConfig().categories[key],
  message: (key: string) => loadConfig().messages[key],
  setMessage(key: string, id: string) {
    const config = loadConfig();
    config.messages[key] = id;
    saveConfig(config);
  },
  clearMessage(key: string) {
    const config = loadConfig();
    if (config.messages[key] !== undefined) {
      delete config.messages[key];
      saveConfig(config);
    }
  },
  value: (key: string) => loadConfig().values[key],
  setValue(key: string, value: string) {
    const config = loadConfig();
    config.values[key] = value;
    saveConfig(config);
  },
};
