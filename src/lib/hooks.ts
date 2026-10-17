import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { localeOf } from "../i18n/languages";
import { ageParts } from "./format";

/** BCP 47 locale for Intl formatting, following the UI language. */
export function useLocale(): string {
  const { i18n } = useTranslation();
  return localeOf(i18n.language);
}

/** The current time, updated every `ms`, so relative ages stay true between snapshots. */
export function useNow(ms = 15_000): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const id = setInterval(() => setNow(Date.now()), ms);
    return () => clearInterval(id);
  }, [ms]);
  return now;
}

export function useMediaQuery(query: string): boolean {
  const [matches, setMatches] = useState(() => window.matchMedia(query).matches);
  useEffect(() => {
    const mq = window.matchMedia(query);
    const onChange = () => setMatches(mq.matches);
    onChange();
    mq.addEventListener("change", onChange);
    return () => mq.removeEventListener("change", onChange);
  }, [query]);
  return matches;
}

/** "36 min ago" for an RFC 3339 instant. */
export function useAge() {
  const { t } = useTranslation();
  const now = useNow();
  return (iso: string | null) => {
    const a = ageParts(iso, now);
    return a ? t(`age.${a.unit}`, { value: a.value }) : "";
  };
}
