/**
 * The five-field cron expressions schedules are stored as, read the same way
 * the Node's cron reads them - numbers, `*`, ranges, lists and steps, and
 * nothing the backend's `validate_cron` would refuse.
 *
 * Times are the Node's wall-clock time, because the Node's cron is what
 * fires them. Everything here takes the Node's UTC offset and works in that
 * zone; the caller turns the result into the viewer's own time for display.
 */

interface ParsedCron {
  minutes: number[];
  hours: number[];
  days: Set<number>;
  months: Set<number>;
  weekdays: Set<number>;
  /** Cron's own rule: when both day fields are restricted, either may match. */
  daysRestricted: boolean;
  weekdaysRestricted: boolean;
}

const FIELDS: ReadonlyArray<readonly [number, number]> = [
  [0, 59],
  [0, 23],
  [1, 31],
  [1, 12],
  [0, 7],
];

function parseField(text: string, min: number, max: number): number[] | null {
  const values = new Set<number>();
  for (const item of text.split(",")) {
    const [range, stepText] = item.split("/");
    const step = stepText === undefined ? 1 : Number(stepText);
    if (!Number.isInteger(step) || step < 1 || step > max) return null;
    let from = min;
    let to = max;
    if (range !== "*") {
      const [fromText, toText] = range.split("-");
      if (!/^\d+$/.test(fromText) || (toText !== undefined && !/^\d+$/.test(toText))) return null;
      from = Number(fromText);
      to = toText === undefined ? (stepText === undefined ? from : max) : Number(toText);
      if (from < min || to > max || to < from) return null;
    }
    for (let value = from; value <= to; value += step) values.add(value);
  }
  return [...values].sort((a, b) => a - b);
}

export function parseCron(expression: string): ParsedCron | null {
  const fields = expression.trim().split(/\s+/);
  if (fields.length !== 5) return null;
  const parsed = fields.map((field, index) => parseField(field, FIELDS[index][0], FIELDS[index][1]));
  if (parsed.some((values) => values === null)) return null;
  const [minutes, hours, days, months, weekdays] = parsed as number[][];
  return {
    minutes,
    hours,
    days: new Set(days),
    months: new Set(months),
    // Both 0 and 7 are Sunday.
    weekdays: new Set(weekdays.map((day) => day % 7)),
    daysRestricted: fields[2] !== "*",
    weekdaysRestricted: fields[4] !== "*",
  };
}

export function isValidCron(expression: string): boolean {
  return parseCron(expression) !== null;
}

function dayMatches(cron: ParsedCron, nodeDate: Date): boolean {
  if (!cron.months.has(nodeDate.getUTCMonth() + 1)) return false;
  const day = cron.days.has(nodeDate.getUTCDate());
  const weekday = cron.weekdays.has(nodeDate.getUTCDay());
  if (cron.daysRestricted && cron.weekdaysRestricted) return day || weekday;
  if (cron.daysRestricted) return day;
  if (cron.weekdaysRestricted) return weekday;
  return true;
}

/**
 * When the expression next fires after `from`, on a Node `offsetMinutes` east
 * of UTC. `null` for an invalid expression, or one that never fires within a
 * year and a bit (31 February, say).
 *
 * A fixed offset: across a daylight-saving change this can be an hour out,
 * which is why the tab says "about" nothing and simply shows the date.
 */
export function nextRun(expression: string, from: Date, offsetMinutes: number): Date | null {
  const cron = parseCron(expression);
  if (!cron) return null;
  const offsetMs = offsetMinutes * 60_000;
  // The Node's wall clock, held in a Date's UTC fields.
  const nodeNow = new Date(from.getTime() + offsetMs);
  const startOfDay = Date.UTC(nodeNow.getUTCFullYear(), nodeNow.getUTCMonth(), nodeNow.getUTCDate());
  for (let dayIndex = 0; dayIndex < 400; dayIndex++) {
    const day = new Date(startOfDay + dayIndex * 86_400_000);
    if (!dayMatches(cron, day)) continue;
    for (const hour of cron.hours) {
      for (const minute of cron.minutes) {
        const candidate = day.getTime() + hour * 3_600_000 + minute * 60_000;
        if (candidate > nodeNow.getTime()) return new Date(candidate - offsetMs);
      }
    }
  }
  return null;
}

/** The friendly shapes the editor offers, and the one it falls back to. */
export type CronPreset =
  | { kind: "daily"; hour: number; minute: number }
  | { kind: "weekdays"; hour: number; minute: number; days: number[] }
  | { kind: "hours"; every: number }
  | { kind: "custom"; expression: string };

export function buildCron(preset: CronPreset): string {
  switch (preset.kind) {
    case "daily":
      return `${preset.minute} ${preset.hour} * * *`;
    case "weekdays":
      return `${preset.minute} ${preset.hour} * * ${[...preset.days].sort((a, b) => a - b).join(",")}`;
    case "hours":
      return `0 */${preset.every} * * *`;
    case "custom":
      return preset.expression.trim().split(/\s+/).join(" ");
  }
}

/** Reads an expression back into the shape that would have written it, so editing opens where it was left. */
export function presetOf(expression: string): CronPreset {
  const fields = expression.trim().split(/\s+/);
  const number = (text: string) => (/^\d+$/.test(text) ? Number(text) : null);
  if (fields.length === 5) {
    const [minute, hour, day, month, weekday] = fields;
    const m = number(minute);
    const h = number(hour);
    if (m !== null && h !== null && m <= 59 && h <= 23 && day === "*" && month === "*") {
      if (weekday === "*") return { kind: "daily", hour: h, minute: m };
      if (/^[0-7](,[0-7])*$/.test(weekday)) {
        const days = [...new Set(weekday.split(",").map((d) => Number(d) % 7))];
        return { kind: "weekdays", hour: h, minute: m, days };
      }
    }
    const every = /^\*\/(\d+)$/.exec(hour);
    if (minute === "0" && every && day === "*" && month === "*" && weekday === "*") {
      const n = Number(every[1]);
      if (n >= 1 && n <= 23) return { kind: "hours", every: n };
    }
  }
  return { kind: "custom", expression };
}

export function twoDigits(value: number): string {
  return String(value).padStart(2, "0");
}
