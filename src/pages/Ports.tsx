import { AlertTriangle, CircleCheck, Network, RefreshCw, Search, SearchX, ShieldAlert, Ban } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { ListenerTable } from "../components/ListenerTable";
import { Banner, Button, Card, Chip, EmptyState, PageHeader, Segmented, Skeleton, Stat, Switch, type Tone } from "../components/ui";
import { api } from "../lib/api";
import { applyFilters, counts, portQuery, type ProtocolFilter, type Scope } from "../lib/filter";
import type { PortDiagnosis, Runtime, Verdict } from "../lib/types";
import { useApp } from "../store/app";

const VERDICT_TONE: Record<Verdict, Tone> = { free: "positive", inUse: "neutral", shared: "warning", reserved: "warning", blocked: "danger" };

function PortLookup({ port }: { port: number }) {
  const { t } = useTranslation();
  const takenAt = useApp((s) => s.snapshot?.takenAt);
  const [diag, setDiag] = useState<PortDiagnosis | null>(null);

  useEffect(() => {
    let alive = true;
    const id = setTimeout(() => {
      api
        .diagnose(port)
        .then((d) => alive && setDiag(d))
        .catch(() => alive && setDiag(null));
    }, 250);
    return () => {
      alive = false;
      clearTimeout(id);
    };
    // Re-check when a new snapshot arrives: the port may have been freed or taken.
  }, [port, takenAt]);

  if (!diag || diag.port !== port) return null;
  const text =
    diag.verdict === "free"
      ? t("ports.lookupFree", { port })
      : diag.verdict === "inUse"
        ? t("ports.lookupInUse", { port })
        : diag.verdict === "shared"
          ? t("ports.lookupShared", { port })
          : diag.verdict === "reserved"
            ? t("ports.lookupReserved", { port, start: diag.excludedRange?.start, end: diag.excludedRange?.end })
            : t("ports.lookupBlocked", { port });
  const Icon = diag.verdict === "free" ? CircleCheck : diag.verdict === "blocked" ? Ban : diag.verdict === "inUse" ? Network : ShieldAlert;
  return (
    <div className="mb-3">
      <Banner tone={VERDICT_TONE[diag.verdict]} icon={<Icon size={16} strokeWidth={1.75} />}>
        <span>{text}</span>
        {diag.suggestions.length > 0 && (
          <span className="ml-2 inline-flex flex-wrap items-center gap-1.5">
            <span className="text-ink-muted">{t("ports.lookupSuggest")}:</span>
            {diag.suggestions.map((s) => (
              <span key={s} className="selectable rounded-sm bg-canvas px-1.5 font-mono text-[12px] text-ink">
                {s}
              </span>
            ))}
          </span>
        )}
      </Banner>
    </div>
  );
}

const RUNTIMES: Runtime[] = ["windows", "wsl", "docker"];

export function PortsPage() {
  const { t } = useTranslation();
  const snapshot = useApp((s) => s.snapshot);
  const filters = useApp((s) => s.filters);
  const setFilters = useApp((s) => s.setFilters);
  const resetFilters = useApp((s) => s.resetFilters);
  const selectedId = useApp((s) => s.selectedId);
  const select = useApp((s) => s.select);
  const refresh = useApp((s) => s.refresh);
  const [refreshing, setRefreshing] = useState(false);

  const listeners = useMemo(() => snapshot?.listeners ?? [], [snapshot]);
  const visible = useMemo(() => applyFilters(listeners, filters), [listeners, filters]);
  const c = useMemo(() => counts(listeners), [listeners]);
  const port = portQuery(filters.query);
  // What clearing the search and the scope would show, with the protocol and system toggles kept.
  const reachable = useMemo(() => applyFilters(listeners, { ...filters, query: "", scope: "all", runtimes: [] }).length, [listeners, filters]);
  const runtimeCount = (r: Runtime) => c[r];

  const doRefresh = () => {
    setRefreshing(true);
    void refresh().finally(() => setTimeout(() => setRefreshing(false), 600));
  };

  const toggleRuntime = (r: Runtime) => {
    const has = filters.runtimes.includes(r);
    setFilters({ runtimes: has ? filters.runtimes.filter((x) => x !== r) : [...filters.runtimes, r] });
  };

  return (
    <>
      <PageHeader
        title={t("ports.title")}
        subtitle={t("ports.subtitle")}
        actions={
          <Button icon={<RefreshCw size={15} strokeWidth={1.75} className={refreshing ? "spin" : ""} />} onClick={doRefresh}>
            {t("common.refresh")}
          </Button>
        }
      />

      <div className="module-grid mb-4 grid-cols-2 lg:grid-cols-4">
        <Stat label={t("ports.statListeners")} value={snapshot ? c.total : "-"} />
        <Stat label={t("ports.statConflicts")} value={snapshot ? c.conflicts : "-"} />
        <Stat label={t("ports.statExposed")} value={snapshot ? c.exposed : "-"} />
        <Stat label={t("ports.statAgents")} value={snapshot ? c.agents : "-"} sub={t("ports.runtimeCounts", { windows: c.windows, wsl: c.wsl, docker: c.docker })} />
      </div>

      <div className="mb-3 flex flex-wrap items-center gap-2">
        <label className="field flex min-w-[240px] flex-1 items-center gap-2">
          <Search size={15} strokeWidth={1.75} className="shrink-0 text-ink-muted" aria-hidden />
          <input
            value={filters.query}
            onChange={(e) => setFilters({ query: e.target.value })}
            placeholder={t("ports.search")}
            aria-label={t("ports.search")}
            className="h-full w-full min-w-0 bg-transparent text-[14px] text-ink outline-none placeholder:text-ink-muted"
            spellCheck={false}
          />
        </label>
        <Segmented<Scope>
          value={filters.scope}
          onChange={(scope) => setFilters({ scope })}
          label={t("ports.filters")}
          options={[
            { value: "all", label: t("ports.scopeAll") },
            { value: "conflicts", label: t("ports.scopeConflicts") },
            { value: "exposed", label: t("ports.scopeExposed") },
          ]}
        />
        <Segmented<ProtocolFilter>
          value={filters.protocol}
          onChange={(protocol) => setFilters({ protocol })}
          options={[
            { value: "tcp", label: t("ports.protocolTcp") },
            { value: "udp", label: t("ports.protocolUdp") },
            { value: "both", label: t("ports.protocolBoth") },
          ]}
        />
      </div>
      <div className="mb-4 flex flex-wrap items-center gap-2">
        {RUNTIMES.map((r) => (
          <Chip key={r} active={filters.runtimes.includes(r)} onClick={() => toggleRuntime(r)} count={runtimeCount(r)}>
            {t(`runtime.${r}`)}
          </Chip>
        ))}
        <label className="ml-auto inline-flex items-center gap-2 text-[13px] text-ink-secondary">
          <Switch checked={filters.showSystem} onChange={(showSystem) => setFilters({ showSystem })} label={t("ports.showSystem")} />
          {t("ports.showSystem")}
        </label>
      </div>

      {port !== null && <PortLookup port={port} />}

      {snapshot?.warnings.map((w) => (
        <div key={w} className="mb-3">
          <Banner tone="warning" icon={<AlertTriangle size={16} strokeWidth={1.75} />}>
            {t("ports.warning", { text: w })}
          </Banner>
        </div>
      ))}

      <Card className="overflow-hidden">
        {!snapshot ? (
          <div className="space-y-px p-4">
            {Array.from({ length: 6 }, (_, i) => (
              <Skeleton key={i} className="mb-2 h-10 w-full" />
            ))}
          </div>
        ) : visible.length > 0 ? (
          <ListenerTable listeners={visible} selectedId={selectedId} onSelect={(id) => select(id === selectedId ? null : id)} />
        ) : c.total === 0 && filters.scope === "all" && !filters.query ? (
          <EmptyState icon={<Network size={20} strokeWidth={1.75} />} title={t("ports.emptyTitle")} body={t("ports.emptyBody")} />
        ) : (
          <EmptyState
            icon={<SearchX size={20} strokeWidth={1.75} />}
            title={t("ports.noMatchTitle")}
            body={t("ports.noMatchBody", { count: reachable })}
            action={
              <Button size="sm" onClick={resetFilters}>
                {t("ports.clearFilters")}
              </Button>
            }
          />
        )}
      </Card>
    </>
  );
}
