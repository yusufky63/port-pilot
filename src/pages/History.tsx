import { CirclePlay, CircleStop, History as HistoryIcon, Info, Plug, RefreshCw, Unplug, type LucideIcon } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { Banner, Button, Card, EmptyState, PageHeader, Segmented, Skeleton } from "../components/ui";
import { api } from "../lib/api";
import { exitCodeText } from "../lib/format";
import { useLocale, useNow } from "../lib/hooks";
import type { EventKind, HistoryEvent } from "../lib/types";
import { errorText, useApp } from "../store/app";

type Filter = "all" | "runs" | "ports";
const PAGE = 100;

const ICONS: Record<EventKind, LucideIcon> = { portOpened: Plug, portClosed: Unplug, runStarted: CirclePlay, runExited: CircleStop };

function dayKey(ms: number): string {
  const d = new Date(ms);
  return `${d.getFullYear()}-${d.getMonth()}-${d.getDate()}`;
}

function useEventText() {
  const { t } = useTranslation();
  return (e: HistoryEvent) => {
    const who = e.process ? (e.pid ? `${e.process} (PID ${e.pid})` : e.process) : t("history.unknown");
    const name = e.runName ?? "?";
    switch (e.kind) {
      case "portOpened":
        return t("history.event.portOpened", { port: e.port ?? "", who });
      case "portClosed":
        return t("history.event.portClosed", { port: e.port ?? "", who });
      case "runStarted":
        return t("history.event.runStarted", { name });
      case "runExited":
        if (e.detail === "stopped") return t("history.event.runStopped", { name });
        if (e.detail === "terminated") return t("history.event.runTerminated", { name });
        if (e.detail === "ownerExited") return t("history.event.runLost", { name });
        return e.exitCode !== null ? t("history.event.runExited", { name, code: exitCodeText(e.exitCode) }) : t("history.event.runExitedNoCode", { name });
    }
  };
}

export function HistoryPage() {
  const { t } = useTranslation();
  const locale = useLocale();
  const takenAt = useApp((s) => s.snapshot?.takenAt);
  const historyOn = useApp((s) => s.settings?.history ?? true);
  const [filter, setFilter] = useState<Filter>("all");
  const [events, setEvents] = useState<HistoryEvent[] | null>(null);
  const [more, setMore] = useState(false);
  // Once older pages are open, new snapshots no longer replace the list.
  const [paged, setPaged] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const text = useEventText();
  const now = useNow(60_000);

  const loadFirst = useCallback(
    (f: Filter) =>
      api
        .history(f, null, PAGE)
        .then((list) => {
          setEvents(list);
          setMore(list.length === PAGE);
          setError(null);
        })
        .catch((e) => {
          setEvents([]);
          setError(errorText(e));
        }),
    [],
  );

  useEffect(() => {
    if (!paged) void loadFirst(filter);
  }, [filter, takenAt, paged, loadFirst]);

  const loadMore = () => {
    const last = events?.at(-1);
    if (!last) return;
    setPaged(true);
    api
      .history(filter, [last.at, last.id], PAGE)
      .then((list) => {
        setEvents((prev) => [...(prev ?? []), ...list]);
        setMore(list.length === PAGE);
      })
      .catch((e) => setError(errorText(e)));
  };

  const groups = useMemo(() => {
    const out: { key: string; at: number; items: HistoryEvent[] }[] = [];
    for (const e of events ?? []) {
      const key = dayKey(e.at);
      const g = out.at(-1);
      if (g && g.key === key) g.items.push(e);
      else out.push({ key, at: e.at, items: [e] });
    }
    return out;
  }, [events]);

  const dayLabel = (ms: number) => {
    if (dayKey(ms) === dayKey(now)) return t("history.today");
    if (dayKey(ms) === dayKey(now - 86_400_000)) return t("history.yesterday");
    return new Intl.DateTimeFormat(locale, { dateStyle: "full" }).format(new Date(ms));
  };
  const time = (ms: number) => new Intl.DateTimeFormat(locale, { timeStyle: "medium" }).format(new Date(ms));

  return (
    <>
      <PageHeader
        title={t("history.title")}
        subtitle={t("history.subtitle")}
        actions={
          <>
            <Segmented<Filter>
              value={filter}
              onChange={(f) => {
                setPaged(false);
                setEvents(null);
                setFilter(f);
              }}
              label={t("history.title")}
              options={[
                { value: "all", label: t("history.filterAll") },
                { value: "runs", label: t("history.filterRuns") },
                { value: "ports", label: t("history.filterPorts") },
              ]}
            />
            <Button
              size="icon"
              variant="ghost"
              title={t("common.refresh")}
              aria-label={t("common.refresh")}
              onClick={() => {
                setPaged(false);
                void loadFirst(filter);
              }}
            >
              <RefreshCw size={15} strokeWidth={1.75} />
            </Button>
          </>
        }
      />
      {!historyOn && (
        <div className="mb-4">
          <Banner tone="neutral" icon={<Info size={16} strokeWidth={1.75} />}>
            {t("history.off")}
          </Banner>
        </div>
      )}
      {error && (
        <div className="mb-4">
          <Banner tone="danger">{error}</Banner>
        </div>
      )}
      {events === null ? (
        <div className="space-y-2">
          <Skeleton className="h-10 w-full" />
          <Skeleton className="h-10 w-full" />
          <Skeleton className="h-10 w-full" />
        </div>
      ) : events.length === 0 ? (
        <Card>
          <EmptyState icon={<HistoryIcon size={20} strokeWidth={1.75} />} title={t("history.emptyTitle")} body={t("history.emptyBody")} />
        </Card>
      ) : (
        <div className="space-y-4">
          {groups.map((g) => (
            <Card key={g.key} className="overflow-hidden">
              <div className="border-b border-line bg-surface px-4 py-2">
                <span className="label">{dayLabel(g.at)}</span>
              </div>
              <ol className="divide-y divide-line">
                {g.items.map((e) => {
                  const Icon = ICONS[e.kind];
                  return (
                    <li key={e.id} className="grid grid-cols-[72px_20px_minmax(0,1fr)] items-start gap-3 px-4 py-2.5">
                      <span className="num pt-0.5 font-mono text-[12px] text-ink-muted">{time(e.at)}</span>
                      <Icon size={15} strokeWidth={1.75} className={e.kind === "runStarted" || e.kind === "portOpened" ? "mt-0.5 text-primary" : "mt-0.5 text-ink-muted"} aria-hidden />
                      <div className="min-w-0">
                        <div className="text-[14px] text-ink">{text(e)}</div>
                        <div className="truncate font-mono text-[11px] text-ink-muted" title={e.folder ?? undefined}>
                          {[e.project, e.kind === "runStarted" ? e.detail : null, e.folder].filter(Boolean).join(" · ")}
                        </div>
                        {e.command && <div className="selectable mt-1 truncate font-mono text-[11px] text-ink-secondary">{e.command}</div>}
                      </div>
                    </li>
                  );
                })}
              </ol>
            </Card>
          ))}
          {more && (
            <div className="flex justify-center">
              <Button onClick={loadMore}>{t("history.more")}</Button>
            </div>
          )}
        </div>
      )}
    </>
  );
}
