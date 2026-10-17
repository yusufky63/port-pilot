/** "12 s", "5 min", "3 h", "2 d": how long ago an RFC 3339 instant was. */
export function ageParts(iso: string | null, now: number = Date.now()): { value: number; unit: "s" | "min" | "h" | "d" } | null {
  if (!iso) return null;
  const t = Date.parse(iso);
  if (Number.isNaN(t)) return null;
  const secs = Math.max(0, Math.floor((now - t) / 1000));
  if (secs < 60) return { value: secs, unit: "s" };
  if (secs < 3600) return { value: Math.floor(secs / 60), unit: "min" };
  if (secs < 86400) return { value: Math.floor(secs / 3600), unit: "h" };
  return { value: Math.floor(secs / 86400), unit: "d" };
}

/** Local date and time for an RFC 3339 instant, in the UI locale. */
export function localDateTime(iso: string | null, locale: string): string {
  if (!iso) return "";
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return "";
  return new Intl.DateTimeFormat(locale, { dateStyle: "medium", timeStyle: "medium" }).format(d);
}

export function localTime(iso: string | null, locale: string): string {
  if (!iso) return "";
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return "";
  return new Intl.DateTimeFormat(locale, { timeStyle: "medium" }).format(d);
}

/** `127.0.0.1:3000`, `[::1]:3000`. */
export function hostPort(address: string, port: number): string {
  return address.includes(":") ? `[${address}]:${port}` : `${address}:${port}`;
}

/** Shortens long paths in the middle so both the drive and the folder name stay visible. */
export function middleEllipsis(text: string, max: number): string {
  if (text.length <= max) return text;
  const keep = max - 1;
  const head = Math.ceil(keep * 0.4);
  const tail = keep - head;
  return `${text.slice(0, head)}…${text.slice(text.length - tail)}`;
}

/** Exit codes as people look them up: NTSTATUS values such as 0xC000013A in hex. */
export function exitCodeText(code: number): string {
  return code < 0 ? `0x${(code >>> 0).toString(16).toUpperCase()}` : String(code);
}
