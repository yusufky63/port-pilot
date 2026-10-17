import { AlertTriangle, Search } from "lucide-react";
import { useEffect, useState, type FormEvent, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { ExposureBadge } from "../components/labels";
import { ownerName } from "../lib/listener";
import { Badge, Banner, Button, Card, CardHeader, CopyButton, Field, PageHeader, Skeleton, type Tone } from "../components/ui";
import { api } from "../lib/api";
import { portQuery } from "../lib/filter";
import type { AdapterState, Environment, PortDiagnosis } from "../lib/types";
import { errorText, useApp } from "../store/app";

const ADAPTER_TONE: Record<AdapterState["status"], Tone> = { available: "positive", disabled: "neutral", notInstalled: "neutral", notRunning: "warning", error: "danger" };

function AdapterValue({ state, version }: { state: AdapterState; version: string | null }) {
  const { t } = useTranslation();
  return (
    <span className="flex flex-col gap-1">
      <span className="flex flex-wrap items-center gap-1.5">
        <Badge tone={ADAPTER_TONE[state.status]}>{t(`doctor.adapter.${state.status}`)}</Badge>
        {state.status === "available" && <span className="text-ink-secondary">{state.items.length > 0 ? state.items.join(", ") : t("doctor.nothingRunning")}</span>}
      </span>
      {version && <span className="font-mono text-[12px] text-ink-muted">{version}</span>}
      {state.detail && <span className="text-[12px] text-ink-muted">{state.detail}</span>}
    </span>
  );
}

function PortCheck() {
  const { t } = useTranslation();
  const [text, setText] = useState("");
  const [diag, setDiag] = useState<PortDiagnosis | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const focus = useApp((s) => s.focusListener);

  const submit = (e: FormEvent) => {
    e.preventDefault();
    const port = portQuery(text);
    if (port === null) {
      setError(t("doctor.invalidPort"));
      return;
    }
    setBusy(true);
    setError(null);
    api
      .diagnose(port)
      .then(setDiag)
      .catch((err) => setError(errorText(err)))
      .finally(() => setBusy(false));
  };

  const verdictText = (d: PortDiagnosis) =>
    d.verdict === "free"
      ? t("ports.lookupFree", { port: d.port })
      : d.verdict === "inUse"
        ? t("ports.lookupInUse", { port: d.port })
        : d.verdict === "shared"
          ? t("ports.lookupShared", { port: d.port })
          : d.verdict === "reserved"
            ? t("ports.lookupReserved", { port: d.port, start: d.excludedRange?.start, end: d.excludedRange?.end })
            : t("ports.lookupBlocked", { port: d.port });

  return (
    <Card>
      <CardHeader title={t("doctor.portTitle")} />
      <div className="space-y-3 p-4">
        <form onSubmit={submit} className="flex gap-2">
          <input value={text} onChange={(e) => setText(e.target.value)} inputMode="numeric" placeholder="3000" aria-label={t("doctor.portLabel")} className="field w-[140px] font-mono" />
          <Button type="submit" variant="primary" loading={busy} icon={<Search size={15} strokeWidth={1.75} />}>
            {t("doctor.check")}
          </Button>
        </form>
        {error && <Banner tone="danger">{error}</Banner>}
        {diag && (
          <div className="space-y-2">
            <Banner tone={diag.verdict === "free" ? "positive" : diag.verdict === "inUse" ? "neutral" : diag.verdict === "blocked" ? "danger" : "warning"}>{verdictText(diag)}</Banner>
            {diag.listeners.map((l) => (
              <button
                key={l.id}
                type="button"
                onClick={() => focus(l.id)}
                className="rail flex w-full items-center justify-between gap-3 rounded-md border border-line px-3 py-2 text-left text-[13px] hover:bg-surface"
              >
                <span className="min-w-0 truncate">
                  <span className="text-ink">{ownerName(l)}</span>
                  {l.process && <span className="ml-1.5 font-mono text-[11px] text-ink-muted">PID {l.process.pid}</span>}
                  {l.project && <span className="ml-1.5 text-ink-secondary">· {l.project.name}</span>}
                </span>
                <ExposureBadge exposure={l.exposure} />
              </button>
            ))}
            {diag.suggestions.length > 0 && (
              <div className="flex flex-wrap items-center gap-1.5 text-[13px]">
                <span className="text-ink-muted">{t("ports.lookupSuggest")}:</span>
                {diag.suggestions.map((s) => (
                  <span key={s} className="inline-flex items-center gap-0.5 rounded-sm bg-surface-muted pl-1.5 font-mono text-[12px] text-ink">
                    {s}
                    <CopyButton text={String(s)} />
                  </span>
                ))}
              </div>
            )}
          </div>
        )}
      </div>
    </Card>
  );
}

function FreePorts() {
  const { t } = useTranslation();
  const [range, setRange] = useState("3000-3999");
  const [count, setCount] = useState("3");
  const [ports, setPorts] = useState<number[] | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const submit = (e: FormEvent) => {
    e.preventDefault();
    if (!/^\s*\d{1,5}\s*(-\s*\d{1,5}\s*)?$/.test(range)) {
      setError(t("doctor.invalidRange"));
      return;
    }
    setBusy(true);
    setError(null);
    api
      .findFree(range.replace(/\s+/g, ""), Math.min(50, Math.max(1, Number(count) || 1)))
      .then(setPorts)
      .catch((err) => setError(errorText(err)))
      .finally(() => setBusy(false));
  };

  return (
    <Card>
      <CardHeader title={t("doctor.freeTitle")} hint={t("doctor.freeHint")} />
      <div className="space-y-3 p-4">
        <form onSubmit={submit} className="flex flex-wrap gap-2">
          <input value={range} onChange={(e) => setRange(e.target.value)} aria-label={t("doctor.range")} className="field w-[150px] font-mono" />
          <input value={count} onChange={(e) => setCount(e.target.value)} inputMode="numeric" aria-label={t("doctor.count")} className="field w-[72px] font-mono" />
          <Button type="submit" variant="primary" loading={busy}>
            {t("doctor.find")}
          </Button>
        </form>
        {error && <Banner tone="danger">{error}</Banner>}
        {ports && ports.length === 0 && <Banner tone="warning">{t("doctor.noFree")}</Banner>}
        {ports && ports.length > 0 && (
          <div className="flex flex-wrap gap-1.5">
            {ports.map((p) => (
              <span key={p} className="inline-flex items-center gap-0.5 rounded-sm bg-surface-muted pl-2 font-mono text-[13px] text-ink">
                {p}
                <CopyButton text={String(p)} />
              </span>
            ))}
          </div>
        )}
      </div>
    </Card>
  );
}

function EnvRow({ label, children }: { label: string; children: ReactNode }) {
  return <Field label={label}>{children}</Field>;
}

function EnvironmentCard() {
  const { t } = useTranslation();
  const [env, setEnv] = useState<Environment | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    let alive = true;
    api
      .environment()
      .then((e) => alive && setEnv(e))
      .catch((e) => alive && setError(errorText(e)));
    return () => {
      alive = false;
    };
  }, []);
  return (
    <Card ticks>
      <CardHeader title={t("doctor.envTitle")} />
      <div className="p-4">
        {error && <Banner tone="danger">{error}</Banner>}
        {!env && !error && (
          <div className="space-y-2">
            {Array.from({ length: 5 }, (_, i) => (
              <Skeleton key={i} className="h-6 w-full" />
            ))}
          </div>
        )}
        {env && (
          <dl>
            <EnvRow label={t("doctor.windows")}>{env.os ?? t("common.unknown")}</EnvRow>
            <EnvRow label={t("doctor.user")}>
              {env.user ?? t("common.unknown")}
              {env.elevated && <span className="text-ink-muted"> ({t("doctor.elevated")})</span>}
            </EnvRow>
            <EnvRow label={t("doctor.git")}>{env.git ?? t("doctor.notFound")}</EnvRow>
            <EnvRow label={t("doctor.wsl")}>
              <AdapterValue state={env.wsl} version={env.wslVersion} />
            </EnvRow>
            <EnvRow label={t("doctor.docker")}>
              <AdapterValue state={env.docker} version={env.dockerVersion} />
            </EnvRow>
            <EnvRow label={t("doctor.reserved")}>
              <span className="flex flex-col gap-1">
                <span className="font-mono text-[12px]">{env.excludedTcp.length > 0 ? env.excludedTcp.map((r) => (r.start === r.end ? `${r.start}` : `${r.start}-${r.end}`)).join(", ") : t("common.none")}</span>
                <span className="text-[12px] text-ink-muted">{t("doctor.reservedHint")}</span>
              </span>
            </EnvRow>
          </dl>
        )}
        {env?.windows10 && (
          <div className="mt-3">
            <Banner tone="warning" icon={<AlertTriangle size={16} strokeWidth={1.75} />}>
              {t("doctor.windows10")}
            </Banner>
          </div>
        )}
      </div>
    </Card>
  );
}

export function DoctorPage() {
  const { t } = useTranslation();
  return (
    <>
      <PageHeader title={t("doctor.title")} subtitle={t("doctor.subtitle")} />
      <div className="grid gap-4 lg:grid-cols-2">
        <div className="space-y-4">
          <PortCheck />
          <FreePorts />
        </div>
        <EnvironmentCard />
      </div>
    </>
  );
}
