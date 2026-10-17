import i18n from "i18next";
import { initReactI18next } from "react-i18next";
import { en } from "./en";
import { resolveLang, type Lang } from "./languages";
import { tr } from "./tr";

export { resolveLang, type Lang } from "./languages";

export const TRANSLATIONS = { en, tr } as const;

function cachedLang(): string | undefined {
  try {
    return localStorage.getItem("portpilot:lang") ?? undefined;
  } catch {
    return undefined;
  }
}

const resources = Object.fromEntries(Object.entries(TRANSLATIONS).map(([code, translation]) => [code, { translation }]));

void i18n.use(initReactI18next).init({
  resources,
  lng: resolveLang(cachedLang()),
  fallbackLng: "en",
  interpolation: { escapeValue: false },
  returnNull: false,
});
document.documentElement.lang = i18n.language;

export function setLanguage(pref: string): Lang {
  const lang = resolveLang(pref);
  try {
    localStorage.setItem("portpilot:lang", pref);
  } catch {
    // Storage can be unavailable; the language still applies for this session.
  }
  document.documentElement.lang = lang;
  void i18n.changeLanguage(lang);
  return lang;
}

export default i18n;
