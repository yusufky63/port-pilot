import { Bot, Code2, Cog, Container, FolderOpen, GitBranch, Globe, Laptop, Monitor, SquareTerminal, Wifi, type LucideIcon } from "lucide-react";
import { useTranslation } from "react-i18next";
import { ownerName } from "../lib/listener";
import type { Exposure, Launcher, LauncherKind, Listener, ProjectInfo, Runtime } from "../lib/types";
import { Badge } from "./ui";

const RUNTIME_ICONS: Record<Runtime, LucideIcon> = { windows: Monitor, wsl: SquareTerminal, docker: Container };

export function RuntimeLabel({ listener }: { listener: Listener }) {
  const { t } = useTranslation();
  const Icon = RUNTIME_ICONS[listener.runtime];
  return (
    <span className="flex min-w-0 max-w-full items-center gap-1.5 text-[13px] text-ink-secondary">
      <Icon size={14} strokeWidth={1.75} className="shrink-0" aria-hidden />
      <span className="truncate">
        {t(`runtime.${listener.runtime}`)}
        {listener.wsl && <span className="text-ink-muted"> · {listener.wsl.distro}</span>}
      </span>
    </span>
  );
}

const EXPOSURE_ICONS: Record<Exposure, LucideIcon> = { loopback: Laptop, lan: Wifi, all: Globe };

export function ExposureBadge({ exposure }: { exposure: Exposure }) {
  const { t } = useTranslation();
  const Icon = EXPOSURE_ICONS[exposure];
  return (
    <Badge tone={exposure === "loopback" ? "neutral" : "warning"} title={t(`exposure.${exposure}Hint`)} icon={<Icon size={11} strokeWidth={2} aria-hidden />}>
      {t(`exposure.${exposure}`)}
    </Badge>
  );
}

const LAUNCHER_ICONS: Record<LauncherKind, LucideIcon> = {
  claude: Bot,
  codex: Bot,
  gemini: Bot,
  vsCode: Code2,
  cursor: Code2,
  windsurf: Code2,
  jetBrains: Code2,
  zed: Code2,
  terminal: SquareTerminal,
  explorer: FolderOpen,
  service: Cog,
};

export function LauncherLabel({ launcher }: { launcher: Launcher | null }) {
  const { t } = useTranslation();
  if (!launcher) return <span className="text-ink-muted">-</span>;
  const Icon = LAUNCHER_ICONS[launcher.kind];
  return (
    <span className="flex min-w-0 max-w-full items-center gap-1.5 text-[13px] text-ink-secondary" title={`${launcher.name} (PID ${launcher.pid})`}>
      <Icon size={14} strokeWidth={1.75} className="shrink-0" aria-hidden />
      <span className="truncate">{t(`launcher.${launcher.kind}`)}</span>
    </span>
  );
}

export function OwnerLabel({ listener }: { listener: Listener }) {
  const { t } = useTranslation();
  const l = listener;
  return (
    <span className="flex min-w-0 flex-col">
      <span className="truncate text-[14px] text-ink">{ownerName(l)}</span>
      <span className="truncate font-mono text-[11px] text-ink-muted">
        {l.container ? `${t("ports.container")} · ${l.container.image}` : l.process ? t("ports.pid", { pid: l.process.pid }) : "-"}
      </span>
    </span>
  );
}

export function ProjectLabel({ project }: { project: ProjectInfo | null }) {
  if (!project) return <span className="text-ink-muted">-</span>;
  const git = project.git;
  return (
    <span className="flex min-w-0 flex-col">
      <span className="truncate text-[14px] text-ink" title={project.root}>
        {project.name}
      </span>
      {git && (
        <span className="inline-flex min-w-0 items-center gap-1 font-mono text-[11px] text-ink-muted">
          <GitBranch size={11} strokeWidth={2} className="shrink-0" aria-hidden />
          <span className="truncate">{git.branch ?? git.head ?? "HEAD"}</span>
          {git.dirty && <span className="h-1.5 w-1.5 shrink-0 rounded-full bg-warning" aria-hidden />}
        </span>
      )}
    </span>
  );
}
