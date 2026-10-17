import { AlertTriangle, BadgeCheck, Code2, ExternalLink, FolderOpen, GitBranch, Info, ShieldQuestion, SquareTerminal, X } from "lucide-react";
import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { api } from "../lib/api";
import { cx } from "../lib/cx";
import { hostPort, localDateTime } from "../lib/format";
import { useAge, useLocale } from "../lib/hooks";
import type { Listener } from "../lib/types";
import { errorText, useApp } from "../store/app";
import { ExposureBadge, LauncherLabel } from "./labels";
import { Badge, Banner, Button, CopyButton, Field, SectionTitle } from "./ui";

function Section({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section className="border-b border-line">
      <SectionTitle>{title}</SectionTitle>
      <dl className="px-4 py-2">{children}</dl>
    </section>
  );
}

function CommandLine({ text }: { text: string }) {
  return (
    <div className="group relative">
      <pre className="selectable max-h-[140px] overflow-auto whitespace-pre-wrap break-all rounded-md bg-surface-muted px-3 py-2 pr-9 font-mono text-[12px] leading-relaxed text-ink">{text}</pre>
      <div className="absolute right-1.5 top-1.5">
        <CopyButton text={text} />
      </div>
    </div>
  );
}

export function DetailPanel({ listener, overlay, onClose }: { listener: Listener; overlay: boolean; onClose: () => void }) {
  const { t } = useTranslation();
  const locale = useLocale();
  const age = useAge();
  const info = useApp((s) => s.info);
  const maskSecrets = useApp((s) => s.settings?.maskSecrets ?? true);
  const toast = useApp((s) => s.toast);
  const openStop = useApp((s) => s.openStop);
  const l = listener;
  const p = l.process;
  const project = l.project;
  const openPath = project?.openPath ?? null;

  const act = (fn: () => Promise<void>) => {
    fn().catch((e) => toast("danger", t("toast.failed", { error: errorText(e) })));
  };

  const panel = (
    <aside
      className={cx(
        "flex h-full w-[420px] shrink-0 flex-col border-l border-line bg-canvas",
        overlay && "z-drawer anim-slide fixed inset-y-0 right-0 shadow-overlay",
      )}
      aria-label={t("detail.title", { port: l.port })}
    >
      <header className="flex items-start justify-between gap-3 border-b border-line px-4 py-3">
        <div className="min-w-0">
          <div className="eyebrow">{l.protocol.toUpperCase()}</div>
          <h2 className="display mt-0.5 text-[22px] leading-tight text-ink">{t("detail.title", { port: l.port })}</h2>
          <div className="mt-1 flex flex-wrap items-center gap-1.5">
            <ExposureBadge exposure={l.exposure} />
            {l.conflict && <Badge tone="danger">{t("ports.conflict")}</Badge>}
          </div>
        </div>
        <button type="button" onClick={onClose} aria-label={t("detail.close")} title={t("detail.close")} className="-mr-1 rounded-md p-1 text-ink-muted hover:bg-surface-muted hover:text-ink">
          <X size={18} strokeWidth={1.75} />
        </button>
      </header>

      <div className="flex flex-wrap gap-2 border-b border-line px-4 py-3">
        {l.url && (
          <Button size="sm" variant="primary" icon={<ExternalLink size={14} strokeWidth={1.75} />} onClick={() => act(() => api.openUrl(l.id))}>
            {t("detail.openUrl")}
          </Button>
        )}
        {openPath && (
          <>
            <Button size="sm" icon={<FolderOpen size={14} strokeWidth={1.75} />} onClick={() => act(() => api.openPath(openPath, "folder"))}>
              {t("detail.openFolder")}
            </Button>
            <Button size="sm" icon={<SquareTerminal size={14} strokeWidth={1.75} />} onClick={() => act(() => api.openPath(openPath, "terminal"))}>
              {t("detail.terminal")}
            </Button>
            {info?.hasVscode && (
              <Button size="sm" icon={<Code2 size={14} strokeWidth={1.75} />} onClick={() => act(() => api.openPath(openPath, "editor"))}>
                {t("detail.editor")}
              </Button>
            )}
          </>
        )}
        {l.stop.target && (
          <Button size="sm" variant="danger" onClick={() => openStop(l)}>
            {t("detail.stop")}
          </Button>
        )}
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto">
        {l.stop.blocked && (
          <div className="border-b border-line px-4 py-3">
            <Banner tone="neutral" icon={<Info size={16} strokeWidth={1.75} />}>
              {t(`block.${l.stop.blocked}`)}
            </Banner>
          </div>
        )}
        {l.notes.length > 0 && (
          <div className="space-y-2 border-b border-line px-4 py-3">
            {l.notes.map((n) => (
              <Banner key={n} tone="warning" icon={<AlertTriangle size={16} strokeWidth={1.75} />}>
                {t(`note.${n}`)}
              </Banner>
            ))}
          </div>
        )}

        <Section title={t("detail.endpoints")}>
          {l.endpoints.map((e) => (
            <div key={e.address} className="flex items-center justify-between gap-3 py-1.5">
              <span className="selectable font-mono text-[13px] text-ink">{hostPort(e.address, e.port)}</span>
              <ExposureBadge exposure={e.exposure} />
            </div>
          ))}
          {l.relay && <p className="pb-1 pt-0.5 text-[12px] text-ink-muted">{t("detail.relay", { name: l.relay.name, pid: l.relay.pid })}</p>}
        </Section>

        {p && (
          <Section title={t("detail.owner")}>
            <Field label={t("detail.process")}>
              <span className="inline-flex items-center gap-1.5">
                {p.name}
                {p.start !== null ? (
                  <span title={t("detail.verifiedHint")} className="inline-flex">
                    <BadgeCheck size={15} strokeWidth={1.75} className="text-positive-fg" aria-label={t("detail.verified")} />
                  </span>
                ) : (
                  <span title={t("detail.unverifiedHint")} className="inline-flex">
                    <ShieldQuestion size={15} strokeWidth={1.75} className="text-ink-muted" aria-label={t("detail.unverified")} />
                  </span>
                )}
              </span>
            </Field>
            <Field label={t("detail.pid")} mono>
              <span className="inline-flex items-center gap-1">
                {p.pid}
                <CopyButton text={String(p.pid)} label={t("detail.copyPid")} />
              </span>
            </Field>
            {p.startedAt && (
              <Field label={t("detail.started")}>
                {localDateTime(p.startedAt, locale)} <span className="text-ink-muted">· {age(p.startedAt)}</span>
              </Field>
            )}
            {p.user && <Field label={t("detail.user")}>{p.user}</Field>}
            {p.path && (
              <Field label={t("detail.executable")} mono>
                {p.path}
              </Field>
            )}
            <div className="py-1.5">
              <div className="mb-1.5 text-[13px] text-ink-muted">{t("detail.command")}</div>
              {p.commandLine ? <CommandLine text={p.commandLine} /> : <p className="text-[13px] text-ink-secondary">{p.accessDenied || l.runtime !== "windows" ? t("detail.commandHidden") : "-"}</p>}
              {p.commandLine && maskSecrets && <p className="mt-1 text-[11px] text-ink-muted">{t("detail.commandMasked")}</p>}
            </div>
          </Section>
        )}

        {(l.ancestors.length > 0 || l.launcher) && (
          <Section title={t("detail.chain")}>
            {l.launcher && (
              <div className="pb-2 pt-1">
                <LauncherLabel launcher={l.launcher} />
              </div>
            )}
            <ol className="chain">
              {p && (
                <li className="py-1.5">
                  <div className="text-[13px] text-ink">
                    {p.name} <span className="font-mono text-[11px] text-ink-muted">PID {p.pid}</span>
                  </div>
                </li>
              )}
              {l.ancestors.map((a) => (
                <li key={`${a.pid}-${a.start}`} className="py-1.5">
                  <div className="text-[13px] text-ink">
                    {a.name} <span className="font-mono text-[11px] text-ink-muted">PID {a.pid}</span>
                  </div>
                  {a.commandLine && (
                    <div className="selectable truncate font-mono text-[11px] text-ink-muted" title={a.commandLine}>
                      {a.commandLine}
                    </div>
                  )}
                </li>
              ))}
            </ol>
          </Section>
        )}

        <Section title={t("detail.project")}>
          {project ? (
            <>
              <Field label={t("detail.project")}>
                <span className="font-medium">{project.name}</span>
              </Field>
              <Field label={t("detail.folder")} mono>
                {project.root}
              </Field>
              <Field label={t("detail.attribution")}>
                <span className="inline-flex flex-wrap items-center gap-1.5">
                  <Badge tone={project.confidence === "exact" ? "positive" : "neutral"}>{project.confidence === "exact" ? t("detail.exact") : t("detail.inferred")}</Badge>
                  <span className="text-ink-secondary">{t(`detail.source.${project.source}`)}</span>
                </span>
              </Field>
              {project.manifests.length > 0 && (
                <Field label={t("detail.manifests")} mono>
                  {project.manifests.join(", ")}
                </Field>
              )}
              {project.git && (
                <>
                  <Field label={t("detail.repository")}>{project.git.repoName}</Field>
                  <Field label={t("detail.branch")}>
                    <span className="inline-flex items-center gap-1 font-mono text-[12px]">
                      <GitBranch size={12} strokeWidth={2} aria-hidden />
                      {project.git.branch ?? t("detail.detached")}
                    </span>
                  </Field>
                  {project.git.head && (
                    <Field label={t("detail.commit")} mono>
                      {project.git.head}
                    </Field>
                  )}
                  {project.git.dirty !== null && <Field label={t("detail.changes")}>{project.git.dirty ? t("detail.dirty") : t("detail.clean")}</Field>}
                  <Field label={t("detail.worktree")}>
                    {project.git.linkedWorktree ? (
                      <span>
                        {t("detail.linkedWorktree")} <span className="font-mono text-[12px] text-ink-muted">{project.git.topLevel}</span>
                      </span>
                    ) : (
                      t("detail.mainCheckout")
                    )}
                  </Field>
                </>
              )}
            </>
          ) : (
            <p className="py-1.5 text-[13px] text-ink-secondary">{l.runtime === "windows" && p?.class === "user" ? t("detail.noProject") : "-"}</p>
          )}
        </Section>

        {(l.wsl || l.container) && (
          <Section title={t("detail.runtime")}>
            {l.wsl && (
              <>
                <Field label={t("detail.distro")}>{l.wsl.distro}</Field>
                {l.wsl.cwd && (
                  <Field label={t("detail.linuxCwd")} mono>
                    {l.wsl.cwd}
                  </Field>
                )}
                <Field label={t("detail.linuxBind")} mono>
                  {l.wsl.binds.map((b) => hostPort(b.address, b.port)).join(", ")}
                </Field>
              </>
            )}
            {l.container && (
              <>
                <Field label={t("detail.container")}>{l.container.name}</Field>
                <Field label={t("detail.image")} mono>
                  {l.container.image}
                </Field>
                {l.container.composeProject && (
                  <Field label={t("detail.compose")}>
                    {l.container.composeProject}
                    {l.container.composeService && <span className="text-ink-muted"> / {l.container.composeService}</span>}
                  </Field>
                )}
                <Field label={t("detail.containerStatus")}>{l.container.status}</Field>
                <Field label={t("detail.containerId")} mono>
                  {l.container.id.slice(0, 12)}
                </Field>
              </>
            )}
          </Section>
        )}
      </div>
    </aside>
  );

  if (!overlay) return panel;
  return (
    <>
      <div className="z-drawer anim-fade fixed inset-0 bg-scrim" onMouseDown={onClose} aria-hidden />
      {panel}
    </>
  );
}

