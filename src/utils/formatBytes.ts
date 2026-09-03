/**
 * Bytes as a person reads them.
 *
 * Binary units (1024), because every number this formats comes from
 * `/proc/meminfo` or `df -B1`, which are themselves binary - reporting a
 * 16 GiB machine as "17.2 GB" would be arithmetically defensible and would
 * not match what the machine's own tools say.
 *
 * One decimal past kilobytes and none below it: "1.5 GB" is worth reading,
 * "1536.0 B" is not.
 */
export function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes < 0) return "—";
  if (bytes < 1024) return `${Math.round(bytes)} B`;

  const units = ["KB", "MB", "GB", "TB", "PB"];
  let value = bytes / 1024;
  let unitIndex = 0;
  while (value >= 1024 && unitIndex < units.length - 1) {
    value /= 1024;
    unitIndex += 1;
  }
  return `${value.toFixed(1)} ${units[unitIndex]}`;
}

/**
 * "4.9 / 16.0 GB" - a used-of-total pair sharing one unit.
 *
 * The unit is chosen from the total and applied to both, so the two halves
 * can be compared at a glance. Formatting them independently produces
 * "980.0 MB / 16.0 GB", which is the same fact stated in a way that has to
 * be converted before it means anything.
 */
export function formatBytesOf(used: number, total: number): string {
  if (!Number.isFinite(total) || total <= 0) return "—";

  const units = ["B", "KB", "MB", "GB", "TB", "PB"];
  let unitIndex = 0;
  let scale = 1;
  while (total / scale >= 1024 && unitIndex < units.length - 1) {
    scale *= 1024;
    unitIndex += 1;
  }
  const decimals = unitIndex === 0 ? 0 : 1;
  return `${(used / scale).toFixed(decimals)} / ${(total / scale).toFixed(decimals)} ${units[unitIndex]}`;
}
