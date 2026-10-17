import { AlertTriangle, ArrowRight, GitBranch, ShieldAlert, ShieldCheck } from "lucide-react";
import { useEffect, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { api } from "../lib/api";
import { localDateTime } from "../lib/format";
import { useLocale } from "../lib/hooks";
import type { RecipeResult, RunPlan } from "../lib/types";
import { errorText, useApp } from "../store/app";
import { Badge, Banner, Button, Dialog, Segmented, Skeleton } from "./ui";

const EXAMPLE = `[ports.web]
preferred = 5173

[run.dev]
command = "pnpm dev --port {ports.web} --strictPort"`;

function Row({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="grid grid-cols-[96px_minmax(0,1fr)] gap-3 py-2 text-[13px]">
      <dt className="pt-0.5 text-ink-muted">{label}</dt>
      <dd className="min-w-0">{children}</dd>
    </div>
  );
}

function Pre({ children }: { children: ReactNode }) {
  return <pre className="selectable max-h-[120px] overflow-auto whitespace-pre-wrap break-all rounded-md bg-surface-muted px-3 py-2 font-mono text-[12px] leading-relaxed text-ink">{children}</pre>;
}

/**
 * The only way a run starts from the app: it shows the exact command, folder, environment and
 * ports first, and asks for trust the first time. Nothing runs when the dialog opens.
 */
export function RunDialog({ folder, onClose }: { folder: string; onClose: () => void }) {
  const { t } = useTranslation();
  const locale = useLocale();
  const toast = useApp((s) => s.toast);
  const refreshRuns = useApp((s) => s.refreshRuns);
  const focusRun = useApp((s) => s.focusRun);
  const [result, setResult] = useState<RecipeResult | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [runName, setRunName] = useState<string | null>(null);
  const [plan, setPlan] = useState<RunPlan | null>(null);
  const [planError, setPlanError] = useState<string | null>(null);
  const [busy, setBusy] = useState<"move" | "start" | null>(null);
  const recipe = result?.recipe ?? null;

  useEffect(() => {
    let alive = true;
    api
      .recipe(folder)
      .then((r) => {
        if (!alive) return;
        setResult(r);
        setRunName(r.recipe?.runs[0]?.name ?? null);
      })
      .catch((e) => alive && setLoadError(errorText(e)));
    return () => {
      alive = false;
    };
  }, [folder]);

  useEffect(() => {
    if (!recipe || !runName) return;
    let alive = true;
    api
      .runPlan(recipe.folder, runName)
      .then((p) => alive && setPlan(p))
      .catch((e) => {
        if (!alive) return;
        setPlan(null);
        setPlanError(errorText(e));
      });
    return () => {
      alive = false;
    };
  }, [recipe, runName]);

  const chooseRun = (name: string) => {
    setPlan(null);
    setPlanError(null);
    setRunName(name);
  };

  const moveLease = (service: string) => {
    if (!plan) return;
    setBusy("move");
    api
      .runReallocate(plan.recipeFolder, plan.run, service)
      .then(setPlan)
      .catch((e) => setPlanError(errorText(e)))
      .finally(() => setBusy(null));
  };

  const start = (trust: boolean) => {
    if (!plan) return;
    const ports: Record<string, number> = {};
    for (const p of plan.ports) if (p.port !== null) ports[p.service] = p.port;
    setBusy("start");
    api
      .runStart(plan.recipeFolder, plan.run, plan.digest, ports, trust)
      .then((info) => {
        toast("positive", t("runs.started", { name: info.name }));
        void refreshRuns();
        focusRun(info.id);
        onClose();
      })
      .catch((e) => {
        setPlanError(errorText(e));
        setBusy(null);
      });
  };

  const ready = plan !== null && plan.run === runName && plan.blocked === null && busy === null;
  const footer = (
    <>
      <Button variant="ghost" onClick={onClose} data-autofocus>
        {t("common.cancel")}
      </Button>
      {recipe && recipe.runs.length > 0 && plan?.trusted && (
        <Button variant="primary" disabled={!ready} loading={busy === "start"} onClick={() => start(false)}>
          {busy === "start" ? t("runDialog.starting") : t("runDialog.run")}
        </Button>
      )}
      {recipe && recipe.runs.length > 0 && !plan?.trusted && (
        <>
          <Button variant="secondary" disabled={!ready} onClick={() => start(false)}>
            {t("runDialog.runOnce")}
          </Button>
          <Button variant="primary" disabled={!ready} loading={busy === "start"} onClick={() => start(true)}>
            {busy === "start" ? t("runDialog.starting") : t("runDialog.trustAndRun")}
          </Button>
        </>
      )}
    </>
  );

  return (
    <Dialog open wide onClose={busy === "start" ? () => undefined : onClose} title={recipe ? t("runDialog.title", { project: recipe.project }) : t("projects.run")} footer={footer}>
      {loadError && (
        <Banner tone="danger" icon={<AlertTriangle size={16} strokeWidth={1.75} />}>
          {loadError}
        </Banner>
      )}
      {!result && !loadError && (
        <div className="space-y-2" aria-label={t("runDialog.loading")}>
          <Skeleton className="h-5 w-2/3" />
          <Skeleton className="h-16 w-full" />
          <Skeleton className="h-10 w-full" />
        </div>
      )}
      {result && !recipe && !result.error && (
        <div className="space-y-3 text-[14px]">
          <p className="font-medium text-ink">{t("runDialog.missingTitle")}</p>
          <p className="text-ink-secondary">{t("runDialog.missingBody", { folder: result.folder })}</p>
          <Pre>{EXAMPLE}</Pre>
        </div>
      )}
      {result?.error && (
        <Banner tone="danger" icon={<AlertTriangle size={16} strokeWidth={1.75} />}>
          <div className="font-medium">{t("runDialog.invalid")}</div>
          <div className="selectable mt-1 whitespace-pre-wrap font-mono text-[12px]">{result.error}</div>
        </Banner>
      )}
      {recipe && recipe.runs.length === 0 && <Banner tone="warning">{t("runDialog.noRuns")}</Banner>}
      {recipe && recipe.runs.length > 0 && (
        <div className="space-y-3">
          <div className="flex flex-wrap items-center gap-2 text-[12px]">
            <span className="selectable min-w-0 truncate font-mono text-ink-muted" title={recipe.worktree}>
              {recipe.worktree}
            </span>
            {recipe.branch && (
              <Badge icon={<GitBranch size={10} strokeWidth={2} aria-hidden />}>{recipe.branch}</Badge>
            )}
          </div>
          {recipe.runs.length > 1 && runName && (
            <Segmented size="sm" label={t("runDialog.chooseRun")} value={runName} onChange={chooseRun} options={recipe.runs.map((r) => ({ value: r.name, label: <span className="font-mono">{r.name}</span> }))} />
          )}
          {planError && (
            <Banner tone="danger" icon={<AlertTriangle size={16} strokeWidth={1.75} />}>
              <span className="selectable">{planError}</span>
            </Banner>
          )}
          {!plan && !planError && (
            <div className="space-y-2" aria-label={t("runDialog.checking")}>
              <Skeleton className="h-6 w-full" />
              <Skeleton className="h-14 w-full" />
            </div>
          )}
          {plan && (
            <dl className="divide-y divide-line border-y border-line">
              <Row label={t("runDialog.folder")}>
                <span className="selectable break-all font-mono text-[12px] text-ink">{plan.folder}</span>
              </Row>
              <Row label={t("runDialog.command")}>
                <Pre>{plan.command}</Pre>
              </Row>
              {plan.env.length > 0 && (
                <Row label={t("runDialog.env")}>
                  <Pre>{plan.env.map((e) => `${e.key}=${e.value}`).join("\n")}</Pre>
                </Row>
              )}
              {plan.ports.length > 0 && (
                <Row label={t("runDialog.ports")}>
                  <ul className="space-y-2">
                    {plan.ports.map((p) => (
                      <li key={p.service} className="space-y-1.5">
                        <div className="flex flex-wrap items-center gap-2">
                          <span className="font-mono text-[12px] text-ink-secondary">{p.service}</span>
                          <ArrowRight size={12} strokeWidth={2} className="text-ink-muted" aria-hidden />
                          <span className="num font-mono text-[14px] font-medium text-ink">{p.port ?? "-"}</span>
                          {p.port !== null && <Badge tone={p.leased ? "primary" : "neutral"}>{p.leased ? t("runDialog.portLeased") : t("runDialog.portNew")}</Badge>}
                          <span className="font-mono text-[11px] text-ink-muted">{t("runDialog.portRange", { start: p.rangeStart, end: p.rangeEnd })}</span>
                        </div>
                        {p.port !== null && p.users.length > 0 && (
                          <Banner tone="warning" icon={<AlertTriangle size={16} strokeWidth={1.75} />}>
                            <div>{t("runDialog.conflict", { port: p.port, users: p.users.map((u) => (u.pid ? `${u.name} (PID ${u.pid})` : u.name)).join(", ") })}</div>
                            <Button size="sm" variant="secondary" className="mt-2" loading={busy === "move"} disabled={busy !== null} onClick={() => moveLease(p.service)}>
                              {t("runDialog.allocateNext")}
                            </Button>
                          </Banner>
                        )}
                      </li>
                    ))}
                  </ul>
                </Row>
              )}
            </dl>
          )}
          {plan?.blocked && (
            <Banner tone="danger" icon={<AlertTriangle size={16} strokeWidth={1.75} />}>
              {plan.blocked}
            </Banner>
          )}
          {plan &&
            (plan.trusted ? (
              <Banner tone="positive" icon={<ShieldCheck size={16} strokeWidth={1.75} />}>
                {t("runDialog.trusted", { date: plan.trustedAt ? localDateTime(new Date(plan.trustedAt).toISOString(), locale) : "" })}
              </Banner>
            ) : (
              <Banner tone="warning" icon={<ShieldAlert size={16} strokeWidth={1.75} />}>
                {t("runDialog.warning")}
              </Banner>
            ))}
        </div>
      )}
    </Dialog>
  );
}
