import { env } from "../env.js";
import { log } from "./logger.js";

export interface AppStatus {
  version: string;
  downloads: number;
}

/**
 * The live version and download count shown in the About panel and on the
 * status voice channels.
 *
 * Read from `STATUS_URL` when it is set - the site/updater endpoint that
 * answers `{ "version": "...", "downloads": 123 }` - and falls back to the
 * `STATUS_FALLBACK_*` env values when the URL is unset, unreachable, or
 * answers something unexpected. So the bot always has something honest to show
 * even before that endpoint exists.
 */
export async function getStatus(): Promise<AppStatus> {
  const fallback: AppStatus = { version: env.statusFallbackVersion, downloads: env.statusFallbackDownloads };
  if (!env.statusUrl) return fallback;
  try {
    const response = await fetch(env.statusUrl, { signal: AbortSignal.timeout(8000) });
    if (!response.ok) return fallback;
    const data = (await response.json()) as Partial<AppStatus>;
    return {
      version: typeof data.version === "string" && data.version.length > 0 ? data.version : fallback.version,
      downloads: typeof data.downloads === "number" && Number.isFinite(data.downloads) ? data.downloads : fallback.downloads,
    };
  } catch (error) {
    log.warn(`status endpoint unreachable, using fallback (${error instanceof Error ? error.message : "unknown"})`);
    return fallback;
  }
}
