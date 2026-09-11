/**
 * A short date, formatted once by a reused formatter.
 *
 * `new Date(x).toLocaleDateString()` builds an `Intl.DateTimeFormat` behind
 * the scenes on every call. One row of a file listing is nothing; two
 * hundred rows re-rendered on every keystroke in the filter box is a real
 * share of the frame, and the file browser is where that was felt.
 *
 * The formatter is built lazily and rebuilt when the interface language
 * changes, so a date still reads in the user's own language - it is the
 * construction that is shared, not the locale.
 */
let cached: { locale: string; formatter: Intl.DateTimeFormat } | null = null;

function formatterFor(locale: string): Intl.DateTimeFormat {
  if (cached?.locale !== locale) {
    cached = { locale, formatter: new Intl.DateTimeFormat(locale) };
  }
  return cached.formatter;
}

export function formatShortDate(value: string | number | Date, locale: string): string {
  const date = value instanceof Date ? value : new Date(value);
  // A listing can carry a timestamp the Node wrote in a shape we cannot
  // read; showing nothing beats showing "Invalid Date".
  if (Number.isNaN(date.getTime())) return "";
  return formatterFor(locale).format(date);
}
