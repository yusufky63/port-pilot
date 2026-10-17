import { AlertTriangle, CheckCircle2, Info } from "lucide-react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { api } from "../lib/api";
import { localDateTime } from "../lib/format";
import { useLocale } from "../lib/hooks";
import type { Listener, RunInfo, StopOutcome, StopReport } from "../lib/types";
import { errorText, useApp } from "../store/app";
import { ownerName } from "../lib/listener";
import { Banner, Button, Checkbox, Dialog } from "./ui";

type Phase = "confirm" | "running" | "result";

const DONE: StopOutcome[] = ["stopped", "forced", "alreadyExited"];
/** Outcomes after which terminating is the remaining option. */
const FORCEABLE: StopOutcome[] = ["gracefulTimeout", "gracefulUnavailable"];

/**
 * Two steps on purpose: the first request is always graceful, and terminating is a second,
 * separate decision with its own explanation. A managed run (opened from the Runs page, or a
 * listener that belongs to one) is stopped as a whole: CTRL+BREAK to its process group, then
 * its job is ended on consent.
 */
export function StopDialog({ listener, run, onClose }: { listener?: Listener; run?: RunInfo; onClose: () => void }) {
  const { t } = useTranslation();
  const locale = useLocale();
  const toast = useApp((s) => s.toast);
  const refreshRuns = useApp((s) => s.refreshRuns);
  const grace = useApp((s) => s.settings?.graceSeconds ?? 5);
  const [phase, setPhase] = useState<Phase>("confirm");
  const [report, setReport] = useState<StopReport | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [tree, setTree] = useState(false);
  // A managed listener stops with its whole run, so the run is what the dialog names.
  const name = run ? run.name : listener?.managed ? listener.managed.name : listener ? ownerName(listener) : "";
  const p = listener?.process ?? null;
  const pid = run?.pid ?? p?.pid ?? "";
  const startedAt = run?.startedAt ?? p?.startedAt ?? null;
  const command = run?.command ?? p?.commandLine ?? null;
  const kind = run || listener?.managed ? "run" : (listener?.stop.target?.kind ?? "windows");

  const go = async (force: boolean) => {
    setPhase("running");
    setError(null);
    try {
      const r = run ? await api.runStop(run.id, force) : await api.stop(listener?.id ?? "", force, force && tree);
      setReport(r);
      if (DONE.includes(r.outcome)) toast("positive", t(`stop.outcome.${r.outcome}`, { name, pid, seconds: grace }));
      if (kind === "run") void refreshRuns();
    } catch (e) {
      setError(errorText(e));
    }
    setPhase("result");
  };

  const body =
    kind === "run"
      ? t("stop.runBody", { seconds: grace })
      : kind === "docker"
        ? t("stop.dockerBody", { name: listener?.container?.name ?? name, seconds: grace })
        : kind === "wsl"
          ? t("stop.wslBody", { distro: listener?.wsl?.distro ?? "WSL", seconds: grace })
          : t("stop.windowsBody", { seconds: grace });

  const canForce = report && FORCEABLE.includes(report.outcome) && kind !== "docker";
  const done = report && DONE.includes(report.outcome);

  const footer =
    phase === "confirm" ? (
      <>
        <Button variant="ghost" onClick={onClose}>
          {t("common.cancel")}
        </Button>
        <Button variant="danger" onClick={() => void go(false)} data-autofocus>
          {t("stop.confirm")}
        </Button>
      </>
    ) : phase === "running" ? (
      <Button variant="danger" loading>
        {t("stop.running")}
      </Button>
    ) : canForce ? (
      <>
        <Button variant="ghost" onClick={onClose}>
          {t("common.cancel")}
        </Button>
        <Button variant="dangerSolid" onClick={() => void go(true)}>
          {kind === "run" ? t("stop.runForceConfirm") : t("stop.forceConfirm")}
        </Button>
      </>
    ) : (
      <Button variant="primary" onClick={onClose} data-autofocus>
        {t("stop.done")}
      </Button>
    );

  return (
    <Dialog open onClose={phase === "running" ? () => undefined : onClose} title={canForce ? t("stop.forceTitle") : phase === "result" ? name : t("stop.title", { name })} footer={footer}>
      {phase !== "result" && (
        <div className="space-y-3 text-[14px] text-ink-secondary">
          <p>{body}</p>
          {startedAt && (kind === "windows" || kind === "run") && (
            <p className="font-mono text-[12px] text-ink-muted">{t("stop.identity", { pid, time: localDateTime(startedAt, locale) })}</p>
          )}
          {command && <pre className="selectable max-h-[96px] overflow-auto whitespace-pre-wrap break-all rounded-md bg-surface-muted px-3 py-2 font-mono text-[12px] text-ink">{command}</pre>}
        </div>
      )}
      {phase === "result" && (
        <div className="space-y-3 text-[14px]">
          {error && (
            <Banner tone="danger" icon={<AlertTriangle size={16} strokeWidth={1.75} />}>
              {error}
            </Banner>
          )}
          {report && (
            <Banner tone={done ? "positive" : canForce ? "warning" : "danger"} icon={done ? <CheckCircle2 size={16} strokeWidth={1.75} /> : <AlertTriangle size={16} strokeWidth={1.75} />}>
              <div className="font-medium">{t(`stop.outcome.${report.outcome}`, { name, pid, seconds: grace })}</div>
              {report.channel && <div className="mt-0.5">{t(`stop.channel.${report.channel}`)}</div>}
              {report.gracefulSkipped && <div className="mt-0.5">{t(`stop.skip.${report.gracefulSkipped}`)}</div>}
              {report.childrenTerminated > 0 && <div className="mt-0.5">{t("stop.children", { count: report.childrenTerminated })}</div>}
              {report.detail && <div className="selectable mt-0.5 font-mono text-[12px]">{report.detail}</div>}
            </Banner>
          )}
          {canForce && (
            <>
              <Banner tone="neutral" icon={<Info size={16} strokeWidth={1.75} />}>
                {kind === "run" ? t("stop.runForceBody") : t("stop.forceBody")}
              </Banner>
              {kind === "windows" && <Checkbox checked={tree} onChange={setTree} label={t("stop.tree")} />}
            </>
          )}
        </div>
      )}
    </Dialog>
  );
}
