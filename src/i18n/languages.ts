/** UI languages, in the order the picker shows them. `locale` drives number and date formats. */
export const LANGUAGES = [
  { code: "en", name: "English", locale: "en-US" },
  { code: "tr", name: "Türkçe", locale: "tr-TR" },
] as const;

export type Lang = (typeof LANGUAGES)[number]["code"];

export function isLang(code: string): code is Lang {
  return LANGUAGES.some((l) => l.code === code);
}

/** Picks the UI language: an explicit setting, else the system language, else English. */
export function resolveLang(pref: string | undefined, system = typeof navigator !== "undefined" ? navigator.language : "en"): Lang {
  if (pref && isLang(pref)) return pref;
  const base = system.toLowerCase().split("-")[0] ?? "en";
  return isLang(base) ? base : "en";
}

export function localeOf(code: string): string {
  return LANGUAGES.find((l) => l.code === code)?.locale ?? "en-US";
}
