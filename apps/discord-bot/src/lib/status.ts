import { env } from "../env.js";
import { log } from "./logger.js";

export interface AppStatus {
  version: string;
  downloads: number;
}

/** The public releases feed - the same source the website's download page
 * reads, so the two never disagree, and always current without an endpoint
 * being kept running. */
const RELEASES_LATEST = "https://api.github.com/repos/VibeSSH/vibessh-releases/releases/latest";

/** How long to wait for a version fetch. Generous on purpose: from the server
 * the bot runs on, GitHub sometimes answers in well over the few seconds a
 * desktop would, and a timeout here means the status channel drops back to the
 * stale env fallback for a whole interval. Better a slow read than a wrong one. */
const FETCH_TIMEOUT_MS = 20000;

function reason(error: unknown): string {
  return error instanceof Error ? error.message : "unknown";
}

/** Version and total downloads from the latest public release, or null when the
 * feed cannot be reached. Never invents a number. */
async function fetchFromReleases(): Promise<AppStatus | null> {
  const response = await fetch(RELEASES_LATEST, {
    headers: { Accept: "application/vnd.github+json" },
    signal: AbortSignal.timeout(FETCH_TIMEOUT_MS),
  });
  if (!response.ok) return null;
  const data = (await response.json()) as { tag_name?: string; assets?: { download_count?: number }[] };
  const version = (data.tag_name ?? "").replace(/^v/, "");
  if (!version) return null;
  const downloads = (data.assets ?? []).reduce((sum, asset) => sum + (typeof asset.download_count === "number" ? asset.download_count : 0), 0);
  return { version, downloads };
}

/** Version from `STATUS_URL`, when it is set and answers `{ version }`. */
async function fetchFromStatusUrl(): Promise<AppStatus | null> {
  if (!env.statusUrl) return null;
  const response = await fetch(env.statusUrl, { signal: AbortSignal.timeout(FETCH_TIMEOUT_MS) });
  if (!response.ok) return null;
  const data = (await response.json()) as Partial<AppStatus>;
  if (typeof data.version !== "string" || data.version.length === 0) return null;
  return {
    version: data.version,
    downloads: typeof data.downloads === "number" && Number.isFinite(data.downloads) ? data.downloads : 0,
  };
}

/**
 * The live version and download count shown in the About panel and on the
 * status voice channels.
 *
 * `STATUS_URL`, when set, wins - a self-hoster can point it at their own
 * `{ version, downloads }` endpoint. Otherwise both come straight from the
 * public releases on GitHub. The `STATUS_FALLBACK_*` env values are the last
 * resort, only when neither can be reached, so the bot always has something
 * honest to show.
 */
export async function getStatus(): Promise<AppStatus> {
  try {
    const fromUrl = await fetchFromStatusUrl();
    if (fromUrl) return fromUrl;
  } catch (error) {
    log.warn(`STATUS_URL unreachable, trying the releases feed (${reason(error)})`);
  }
  try {
    const fromReleases = await fetchFromReleases();
    if (fromReleases) return fromReleases;
  } catch (error) {
    log.warn(`releases feed unreachable, using fallback (${reason(error)})`);
  }
  return { version: env.statusFallbackVersion, downloads: env.statusFallbackDownloads };
}

/**
 * Just the current version, and only when it came from a live source - never
 * the static `STATUS_FALLBACK_VERSION`.
 *
 * The release watcher must not mistake the fallback for a real release and
 * announce it, so a failure here returns null and the watcher simply tries
 * again next tick.
 */
export async function fetchLiveVersion(): Promise<string | null> {
  try {
    const fromUrl = await fetchFromStatusUrl();
    if (fromUrl) return fromUrl.version;
  } catch (error) {
    log.warn(`STATUS_URL unreachable for the release check (${reason(error)})`);
  }
  try {
    const fromReleases = await fetchFromReleases();
    if (fromReleases) return fromReleases.version;
  } catch (error) {
    log.warn(`releases feed unreachable for the release check (${reason(error)})`);
  }
  return null;
}
