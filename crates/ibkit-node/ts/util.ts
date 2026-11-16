// Date helpers (ib_async `util.py`).

const pad = (n: number) => String(n).padStart(2, '0');

/** Format a time for TWS: a `Date` as UTC `YYYYMMDD-HH:MM:SS`; a string is
 * passed through; empty input means "now" to TWS. */
export function formatIBDatetime(t: string | Date | null | undefined): string {
  if (!t) return '';
  if (typeof t === 'string') return t;
  return (
    `${t.getUTCFullYear()}${pad(t.getUTCMonth() + 1)}${pad(t.getUTCDate())}-` +
    `${pad(t.getUTCHours())}:${pad(t.getUTCMinutes())}:${pad(t.getUTCSeconds())}`
  );
}

/** Parse the time formats TWS sends: `YYYYMMDD` (UTC midnight), Unix
 * seconds, or `YYYYMMDD[-| ]HH:MM:SS` (read as UTC). Unknown input returns null. */
export function parseIBDatetime(s: string): Date | null {
  const text = s.trim();
  if (/^\d{8}$/.test(text)) return new Date(Date.UTC(+text.slice(0, 4), +text.slice(4, 6) - 1, +text.slice(6, 8)));
  if (/^\d+$/.test(text)) return new Date(Number(text) * 1000);
  const m = /^(\d{4})(\d{2})(\d{2})[- ]+(\d{2}):(\d{2}):(\d{2})/.exec(text);
  return m ? new Date(Date.UTC(+m[1]!, +m[2]! - 1, +m[3]!, +m[4]!, +m[5]!, +m[6]!)) : null;
}
