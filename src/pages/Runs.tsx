import { AlertTriangle, CirclePlay, Copy, FolderSearch, Play, RotateCcw, Square, X } from "lucide-react";
import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Badge, Banner, Button, Card, CardHeader, EmptyState, PageHeader, Skeleton, Switch, type Tone } from "../components/ui";
import { api } from "../lib/api";
import { cx } from "../lib/cx";
import { exitCodeText } from "../lib/format";
import { useAge } from "../lib/hooks";
import type { LeaseView, OutputLine, RecentFolder, RunInfo, RunState } from "../lib/types";
import { errorText, useApp } from "../store/app";

const STATE_TONE: Record<RunState, Tone> = { running: "positive", stopping: "warning", exited: "neutral" };

function PortChip({ port }: { port: number }) {
  const snapshot = useApp((s) => s.snapshot);
  const focus = useApp((s) => s.focusListener);
  const listener = snapshot?.listeners.find((l) => l.port === port && l.protocol === "tcp");
  if (!listener) return <span className="inline-flex h-6 items-center rounded-sm border border-line px-1.5 font-mono text-[12px] text-ink-muted">:{port}</span>;
  return (
    <button type="button" onClick={() => focus(listener.id)} className="inline-flex h-6 items-center rounded-sm border border-line px-1.5 font-mono text-[12px] text-ink hover:border-primary" title={listener.process?.name ?? ""}>
      :{port}
    </button>
  );
}

function RunRow({ run, selected }: { run: RunInfo; selected: boolean }) {
  const { t } = useTranslation();
  const age = useAge();
  const selectRun = useApp((s) => s.selectRun);
  const openRunStop = useApp((s) => s.openRunStop);
  const openRun = useApp((s) => s.openRun);
  const refreshRuns = useApp((s) => s.refreshRuns);
  const live = run.state !== "exited";
  const outcome =
    run.state !== "exited"
      ? t("runs.processes", { count: run.processes })
      : run.endedBy
        ? t(`runs.endedBy.${run.endedBy}`)
        : run.exitCode !== null
          ? t("runs.exitCode", { code: exitCodeText(run.exitCode) })
          : "";
  return (
    <li
      className={cx("rail grid cursor-pointer grid-cols-[minmax(0,1fr)_auto] items-center gap-3 px-4 py-3 hover:bg-surface", selected && "rail-active bg-surface")}
      onClick={() => selectRun(run.id)}
      onKeyDown={(e) => {
        if (e.target === e.currentTarget && (e.key === "Enter" || e.key === " ")) {
          e.preventDefault();
          selectRun(run.id);
        }
      }}
      tabIndex={0}
      aria-current={selected ? "true" : undefined}
    >
      <div className="min-w-0 space-y-1">
        <div className="flex flex-wrap items-center gap-2">
          <span className={cx("h-2 w-2 shrink-0 rounded-full", live ? "bg-positive" : "bg-line-strong")} aria-hidden />
          <span className="font-mono text-[14px] font-medium text-ink">{run.name}</span>
          <span className="truncate text-[13px] text-ink-secondary">{run.project}</span>
          <Badge tone={STATE_TONE[run.state]}>{t(`runs.state.${run.state}`)}</Badge>
          {run.ports.map((p) => (
            <PortChip key={p.service} port={p.port} />
          ))}
        </div>
        <div className="truncate font-mono text-[11px] text-ink-muted" title={run.folder}>
          {t("runs.pid", { pid: run.pid })} · {age(run.startedAt)} · {outcome}
        </div>
      </div>
      <div className="flex items-center gap-1" onClick={(e) => e.stopPropagation()}>
        {live ? (
          <Button size="sm" variant="danger" icon={<Square size={13} strokeWidth={2} />} onClick={() => openRunStop(run)}>
            {t("runs.stop")}
          </Button>
        ) : (
          <>
            <Button size="sm" icon={<RotateCcw size={13} strokeWidth={1.75} />} onClick={() => openRun(run.folder)}>
              {t("runs.again")}
            </Button>
            <Button
              size="icon"
              variant="ghost"
              title={t("runs.forget")}
              aria-label={t("runs.forget")}
              onClick={() => {
                void api.runForget(run.id).then(() => refreshRuns());
              }}
            >
              <X size={15} strokeWidth={1.75} />
            </Button>
          </>
        )}
      </div>
    </li>
  );
}

/** Polls the run's output while it is shown; the lines live in the app's memory only. */
function OutputViewer({ run }: { run: RunInfo }) {
  const { t } = useTranslation();
  const toast = useApp((s) => s.toast);
  const [lines, setLines] = useState<OutputLine[]>([]);
  const [truncated, setTruncated] = useState(false);
  const [follow, setFollow] = useState(true);
  const box = useRef<HTMLDivElement>(null);
  // Survives the effect restarting when the run exits, so no line is fetched twice.
  const next = useRef(0);
  const live = run.state !== "exited";

  useEffect(() => {
    let alive = true;
    let inFlight = false;
    const tick = async () => {
      if (inFlight) return;
      inFlight = true;
      try {
        const chunk = await api.runOutput(run.id, next.current);
        if (!alive) return;
        if (chunk.truncated) setTruncated(true);
        if (chunk.lines.length > 0) {
          next.current = chunk.next;
          setLines((prev) => [...prev, ...chunk.lines].slice(-5000));
        }
      } catch {
        // The run was removed; the list above updates on its own.
      } finally {
        inFlight = false;
      }
    };
    void tick();
    // A finished run produces nothing new after its pipes close.
    const id = live ? setInterval(() => void tick(), 800) : undefined;
    return () => {
      alive = false;
      if (id) clearInterval(id);
    };
  }, [run.id, live]);

  useLayoutEffect(() => {
    if (follow && box.current) box.current.scrollTop = box.current.scrollHeight;
  }, [lines, follow]);

  const copy = () => {
    api
      .runOutputText(run.id)
      .then((text) => navigator.clipboard.writeText(text))
      .then(() => toast("positive", t("common.copied")))
      .catch((e) => toast("danger", t("toast.failed", { error: errorText(e) })));
  };

  return (
    <Card>
      <CardHeader
        title={t("runs.outputOf", { name: run.name })}
        hint={t("runs.outputHint")}
        actions={
          <>
            <span className="hidden items-center gap-2 text-[12px] text-ink-secondary sm:inline-flex">
              {t("runs.follow")}
              <Switch checked={follow} onChange={setFollow} label={t("runs.follow")} />
            </span>
            <Button size="icon" variant="ghost" title={t("runs.copyOutput")} aria-label={t("runs.copyOutput")} onClick={copy}>
              <Copy size={15} strokeWidth={1.75} />
            </Button>
          </>
        }
      />
      <div className="space-y-2 p-4">
        <dl className="grid gap-x-4 gap-y-1 text-[12px] sm:grid-cols-[auto_minmax(0,1fr)]">
          <dt className="text-ink-muted">{t("runs.folder")}</dt>
          <dd className="selectable truncate font-mono text-ink" title={run.folder}>
            {run.folder}
          </dd>
          <dt className="text-ink-muted">{t("runs.command")}</dt>
          <dd className="selectable break-all font-mono text-ink">{run.command}</dd>
        </dl>
        {truncated && <Banner tone="neutral">{t("runs.outputTruncated")}</Banner>}
        <div
          ref={box}
          onScroll={(e) => {
            const el = e.currentTarget;
            const atEnd = el.scrollHeight - el.scrollTop - el.clientHeight < 24;
            if (atEnd !== follow) setFollow(atEnd);
          }}
          className="selectable h-[360px] overflow-auto rounded-md border border-line bg-surface px-3 py-2 font-mono text-[12px] leading-relaxed"
          role="log"
          aria-live="off"
        >
          {lines.length === 0 ? (
            <p className="text-ink-muted">{t("runs.outputEmpty")}</p>
          ) : (
            lines.map((l) => (
              <div key={l.seq} className={cx("whitespace-pre-wrap break-all", l.stream === "stderr" ? "text-ink-secondary" : "text-ink")}>
                {l.text || " "}
              </div>
            ))
          )}
        </div>
      </div>
    </Card>
  );
}

function RecentProjects() {
  const { t } = useTranslation();
  const openRun = useApp((s) => s.openRun);
  const runCount = useApp((s) => s.runs.length);
  const [list, setList] = useState<RecentFolder[] | null>(null);
  useEffect(() => {
    let alive = true;
    api
      .recentProjects()
      .then((l) => alive && setList(l))
      .catch(() => alive && setList([]));
    return () => {
      alive = false;
    };
  }, [runCount]);
  return (
    <Card>
      <CardHeader title={t("runs.recent")} />
      {list === null ? (
        <div className="p-4">
          <Skeleton className="h-10 w-full" />
        </div>
      ) : list.length === 0 ? (
        <p className="px-4 py-3 text-[13px] text-ink-muted">{t("runs.recentEmpty")}</p>
      ) : (
        <ul className="divide-y divide-line">
          {list.map((f) => (
            <li key={f.folder} className="flex items-center justify-between gap-3 px-4 py-2.5">
              <div className="min-w-0">
                <div className="truncate text-[14px] text-ink">{f.project}</div>
                <div className="truncate font-mono text-[11px] text-ink-muted" title={f.folder}>
                  {f.folder}
                </div>
              </div>
              <Button size="sm" icon={<Play size={13} strokeWidth={1.75} />} onClick={() => openRun(f.folder)}>
                {t("runs.start")}
              </Button>
            </li>
          ))}
        </ul>
      )}
    </Card>
  );
}

function Leases() {
  const { t } = useTranslation();
  const toast = useApp((s) => s.toast);
  const snapshot = useApp((s) => s.snapshot);
  const [leases, setLeases] = useState<LeaseView[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [version, setVersion] = useState(0);
  // Who uses a leased port changes with the listeners, not on every refresh.
  const signature = useMemo(() => (snapshot?.listeners ?? []).map((l) => l.id).join("|"), [snapshot]);

  useEffect(() => {
    let alive = true;
    api
      .leases()
      .then((l) => {
        if (!alive) return;
        setLeases(l);
        setError(null);
      })
      .catch((e) => {
        if (!alive) return;
        setLeases([]);
        setError(errorText(e));
      });
    return () => {
      alive = false;
    };
  }, [signature, version]);

  const release = (l: LeaseView) => {
    api
      .leaseRelease(l.ownerKey, l.service)
      .then(() => {
        toast("positive", t("runs.released", { service: l.service }));
        setVersion((v) => v + 1);
      })
      .catch((e) => toast("danger", t("toast.failed", { error: errorText(e) })));
  };

  return (
    <Card>
      <CardHeader title={t("runs.leasesTitle")} hint={t("runs.leasesHint")} />
      {error && (
        <div className="p-4">
          <Banner tone="danger">{error}</Banner>
        </div>
      )}
      {leases === null ? (
        <div className="p-4">
          <Skeleton className="h-10 w-full" />
        </div>
      ) : leases.length === 0 ? (
        !error && <p className="px-4 py-3 text-[13px] text-ink-muted">{t("runs.leasesEmpty")}</p>
      ) : (
        <ul className="divide-y divide-line">
          {leases.map((l) => {
            const own = l.users.length > 0 && l.users.every((u) => u.sameWorktree);
            const state = l.stale ? t("runs.leaseStale") : l.users.length === 0 ? t("runs.leaseFree") : own ? t("runs.leaseOwn") : t("runs.leaseInUse", { name: l.users.map((u) => u.name).join(", ") });
            const tone: Tone = l.stale ? "neutral" : l.users.length === 0 ? "positive" : own ? "primary" : "warning";
            return (
              <li key={`${l.ownerKey}|${l.service}`} className="grid grid-cols-[64px_minmax(0,1fr)_auto] items-center gap-3 px-4 py-2.5">
                <span className="num font-mono text-[15px] font-medium text-ink">{l.port}</span>
                <div className="min-w-0">
                  <div className="flex flex-wrap items-center gap-1.5">
                    <span className="font-mono text-[12px] text-ink">{l.service}</span>
                    <span className="truncate text-[13px] text-ink-secondary">{l.project}</span>
                    <Badge tone={tone}>{state}</Badge>
                  </div>
                  <div className="truncate font-mono text-[11px] text-ink-muted" title={l.worktree}>
                    {l.worktree}
                  </div>
                </div>
                <Button size="sm" variant="ghost" onClick={() => release(l)}>
                  {t("runs.release")}
                </Button>
              </li>
            );
          })}
        </ul>
      )}
    </Card>
  );
}

export function RunsPage() {
  const { t } = useTranslation();
  const runs = useApp((s) => s.runs);
  const selectedRunId = useApp((s) => s.selectedRunId);
  const info = useApp((s) => s.info);
  const openRun = useApp((s) => s.openRun);
  const toast = useApp((s) => s.toast);
  const [picking, setPicking] = useState(false);
  const selected = runs.find((r) => r.id === selectedRunId) ?? null;

  const pick = () => {
    setPicking(true);
    api
      .pickFolder()
      .then((folder) => {
        if (folder) openRun(folder);
      })
      .catch((e) => toast("danger", t("toast.failed", { error: errorText(e) })))
      .finally(() => setPicking(false));
  };

  return (
    <>
      <PageHeader
        title={t("runs.title")}
        subtitle={t("runs.subtitle")}
        actions={
          <Button variant="primary" loading={picking} icon={<FolderSearch size={15} strokeWidth={1.75} />} onClick={pick}>
            {t("runs.openFolder")}
          </Button>
        }
      />
      {info?.storeError && (
        <div className="mb-4">
          <Banner tone="danger" icon={<AlertTriangle size={16} strokeWidth={1.75} />}>
            {t("settings.storeError", { error: info.storeError })}
          </Banner>
        </div>
      )}
      <div className="grid gap-4 xl:grid-cols-[minmax(0,1fr)_380px]">
        <div className="min-w-0 space-y-4">
          <Card>
            <CardHeader title={t("runs.listTitle")} />
            {runs.length === 0 ? (
              <EmptyState icon={<CirclePlay size={20} strokeWidth={1.75} />} title={t("runs.emptyTitle")} body={t("runs.emptyBody")} />
            ) : (
              <ul className="divide-y divide-line">
                {runs.map((r) => (
                  <RunRow key={r.id} run={r} selected={r.id === selectedRunId} />
                ))}
              </ul>
            )}
          </Card>
          {selected && <OutputViewer key={selected.id} run={selected} />}
        </div>
        <div className="min-w-0 space-y-4">
          <RecentProjects />
          <Leases />
        </div>
      </div>
    </>
  );
}
