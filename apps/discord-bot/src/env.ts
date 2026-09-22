import "dotenv/config";

/**
 * The bot's environment - the secrets and ids that differ between the real
 * server and a test one, kept out of the code and out of git.
 *
 * Read once here and validated up front, so a missing token fails on start with
 * a clear message rather than deep inside the first API call.
 */
function required(name: string): string {
  const value = process.env[name];
  if (!value || value.trim().length === 0) {
    throw new Error(`Missing ${name} in the environment. Copy .env.example to .env and fill it in.`);
  }
  return value.trim();
}

function optional(name: string, fallback: string): string {
  const value = process.env[name];
  return value && value.trim().length > 0 ? value.trim() : fallback;
}

export const env = {
  token: required("DISCORD_TOKEN"),
  clientId: required("CLIENT_ID"),
  guildId: required("GUILD_ID"),
  statusUrl: optional("STATUS_URL", ""),
  statusFallbackVersion: optional("STATUS_FALLBACK_VERSION", "0.0.0"),
  statusFallbackDownloads: Number(optional("STATUS_FALLBACK_DOWNLOADS", "0")) || 0,
} as const;
