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

/** Every public release, newest first, a page at a time. */
const RELEASES_LIST = "https://api.github.com/repos/VibeSSH/vibessh-releases/releases?per_page=100";

/** Enough pages for 1,000 releases - a bound, so a misbehaving API cannot keep the bot paging. */
const MAX_RELEASE_PAGES = 10;

interface ReleaseAsset {
  name?: string;
  download_count?: number;
}

/**
 * The files a person downloads to install the app. Only these are counted.
 *
 * Summing every asset counted mostly the app itself: `latest.json` is fetched
 * by every installation each time it checks for an update, and on beta.20 it
 * was 43 of the 49 "downloads". Signatures, checksums, the Vibe Agent binary
 * and its install script are left out for the same reason - they are fetched
 * by machines, not by somebody installing VibeSSH.
 */
const INSTALLER = /\.(exe|msi|AppImage|deb|rpm|dmg)$/i;

export function installerDownloads(assets: ReleaseAsset[]): number {
  return assets.reduce(
    (sum, asset) => sum + (typeof asset.name === "string" && INSTALLER.test(asset.name) && typeof asset.download_count === "number" ? asset.download_count : 0),
    0,
  );
}

async function fetchJson<T>(url: string): Promise<T | null> {
  const response = await fetch(url, { headers: { Accept: "application/vnd.github+json" }, signal: AbortSignal.timeout(FETCH_TIMEOUT_MS) });
  return response.ok ? ((await response.json()) as T) : null;
}

/** The latest public release's version, or null when the feed cannot be reached. */
async function fetchLatestVersion(): Promise<string | null> {
  const data = await fetchJson<{ tag_name?: string }>(RELEASES_LATEST);
  const version = (data?.tag_name ?? "").replace(/^v/, "");
  return version || null;
}

/**
 * Installer downloads summed over every release, not only the latest.
 *
 * Only the latest used to be counted, so the figure fell back to nearly zero
 * each time a version shipped. It still includes updates: on Windows and in
 * the AppImage the updater downloads the same installer a new user does, and
 * GitHub cannot tell the two apart.
 */
async function fetchTotalDownloads(): Promise<number | null> {
  let total = 0;
  for (let page = 1; page <= MAX_RELEASE_PAGES; page++) {
    const releases = await fetchJson<{ assets?: ReleaseAsset[] }[]>(`${RELEASES_LIST}&page=${page}`);
    if (!releases) return null;
    for (const release of releases) total += installerDownloads(release.assets ?? []);
    if (releases.length < 100) break;
  }
  return total;
}

/** Version and installer downloads from the public releases, or null when either cannot be read. Never invents a number. */
async function fetchFromReleases(): Promise<AppStatus | null> {
  const [version, downloads] = await Promise.all([fetchLatestVersion(), fetchTotalDownloads()]);
  return version && downloads !== null ? { version, downloads } : null;
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

/** The backend's public numbers. Installations are counted there, anonymously - see the
 * backend's `migrations/0013_update_checks.sql` for what it keeps and what it refuses to. */
const STATS_URL = "https://api.vibessh.dev/updates/stats";

/**
 * Distinct installations that checked for an update yesterday, or null when the backend
 * cannot be reached or has no number yet.
 *
 * Always one complete day, never a sum: the backend's per-machine hash is not comparable
 * across days, so there is no honest weekly figure to show. Null rather than zero on a
 * failure, so the status channel keeps its last real number instead of announcing nobody.
 */
export async function getInstallations(): Promise<number | null> {
  try {
    const response = await fetch(STATS_URL, { signal: AbortSignal.timeout(FETCH_TIMEOUT_MS) });
    if (!response.ok) return null;
    const data = (await response.json()) as { installations?: unknown };
    return typeof data.installations === "number" && Number.isFinite(data.installations) ? data.installations : null;
  } catch (error) {
    log.warn(`stats endpoint unreachable, keeping the installations figure as it is (${reason(error)})`);
    return null;
  }
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
    // Only the version: this runs every five minutes, and paging through every
    // release for a download count nobody reads here would spend GitHub's
    // 60-an-hour unauthenticated limit.
    const version = await fetchLatestVersion();
    if (version) return version;
  } catch (error) {
    log.warn(`releases feed unreachable for the release check (${reason(error)})`);
  }
  return null;
}
