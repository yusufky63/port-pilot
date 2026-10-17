import { Code2, FolderGit2, FolderOpen, GitBranch, Lock, SquareTerminal } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { Badge, Button, Card, EmptyState, PageHeader, Skeleton } from "../components/ui";
import { api } from "../lib/api";
import { cx } from "../lib/cx";
import type { Listener, ProjectGroup } from "../lib/types";
import { errorText, useApp } from "../store/app";

function PortChips({ ids, byId }: { ids: string[]; byId: Map<string, Listener> }) {
  const focus = useApp((s) => s.focusListener);
  return (
    <span className="flex flex-wrap gap-1.5">
      {ids.map((id) => {
        const l = byId.get(id);
        if (!l) return null;
        return (
          <button
            key={id}
            type="button"
            onClick={() => focus(id)}
            className="inline-flex h-6 items-center gap-1 rounded-sm border border-line px-1.5 font-mono text-[12px] text-ink hover:border-primary"
            title={l.process?.name ?? l.container?.name ?? ""}
          >
            :{l.port}
          </button>
        );
      })}
    </span>
  );
}

function GroupCard({ group, byId }: { group: ProjectGroup; byId: Map<string, Listener> }) {
  const { t } = useTranslation();
  const info = useApp((s) => s.info);
  const toast = useApp((s) => s.toast);
  const open = (path: string, how: "folder" | "terminal" | "editor") => {
    api.openPath(path, how).catch((e) => toast("danger", t("toast.failed", { error: errorText(e) })));
  };
  const actions = (path: string | null) =>
    path && (
      <span className="flex shrink-0 items-center gap-0.5">
        <Button size="icon" variant="ghost" title={t("detail.openFolder")} aria-label={t("detail.openFolder")} onClick={() => open(path, "folder")}>
          <FolderOpen size={15} strokeWidth={1.75} />
        </Button>
        <Button size="icon" variant="ghost" title={t("detail.terminal")} aria-label={t("detail.terminal")} onClick={() => open(path, "terminal")}>
          <SquareTerminal size={15} strokeWidth={1.75} />
        </Button>
        {info?.hasVscode && (
          <Button size="icon" variant="ghost" title={t("detail.editor")} aria-label={t("detail.editor")} onClick={() => open(path, "editor")}>
            <Code2 size={15} strokeWidth={1.75} />
          </Button>
        )}
      </span>
    );
  const runningCount = group.worktrees.reduce((n, w) => n + w.listenerIds.length, 0) + group.listenerIds.length;

  return (
    <Card className="overflow-hidden">
      <header className="flex items-start justify-between gap-4 border-b border-line px-4 py-3">
        <div className="min-w-0">
          <div className="flex items-center gap-2">
            <h2 className="display-medium truncate text-[16px] text-ink">{group.name}</h2>
            {group.runtime !== "windows" && <Badge>{t(`runtime.${group.runtime}`)}</Badge>}
            <Badge tone={runningCount > 0 ? "primary" : "neutral"}>{t("projects.listeners", { count: runningCount })}</Badge>
          </div>
          <div className="selectable mt-0.5 truncate font-mono text-[12px] text-ink-muted" title={group.root}>
            {group.root}
          </div>
        </div>
        {actions(group.openPath)}
      </header>
      {group.commonDir === null ? (
        <div className="flex items-center justify-between gap-3 px-4 py-3 text-[13px]">
          <span className="text-ink-muted">{t("projects.notGit")}</span>
          <PortChips ids={group.listenerIds} byId={byId} />
        </div>
      ) : (
        <ul className="divide-y divide-line">
          {group.worktrees.map((w) => (
            <li key={w.path} className={cx("grid grid-cols-[minmax(0,1fr)_auto] items-center gap-3 px-4 py-2.5", w.listenerIds.length === 0 && "text-ink-secondary")}>
              <div className="min-w-0">
                <div className="flex flex-wrap items-center gap-1.5">
                  <GitBranch size={13} strokeWidth={2} className="shrink-0 text-ink-muted" aria-hidden />
                  <span className="font-mono text-[13px] text-ink">{w.branch ?? t("projects.detached")}</span>
                  {w.head && <span className="font-mono text-[11px] text-ink-muted">{w.head}</span>}
                  <Badge tone={w.main ? "primary" : "neutral"}>{w.main ? t("projects.mainCheckout") : t("projects.worktree")}</Badge>
                  {w.dirty && <Badge tone="warning">{t("projects.dirty")}</Badge>}
                  {w.locked && (
                    <Badge icon={<Lock size={10} strokeWidth={2} aria-hidden />}>{t("projects.locked")}</Badge>
                  )}
                  {w.prunable && <Badge>{t("projects.prunable")}</Badge>}
                </div>
                <div className="selectable mt-0.5 truncate font-mono text-[11px] text-ink-muted" title={w.path}>
                  {w.path}
                </div>
              </div>
              <div className="flex items-center gap-2">
                {w.listenerIds.length > 0 ? <PortChips ids={w.listenerIds} byId={byId} /> : <span className="text-[12px] text-ink-muted">{t("projects.idle")}</span>}
                {group.runtime === "windows" && actions(w.path)}
              </div>
            </li>
          ))}
        </ul>
      )}
    </Card>
  );
}

export function ProjectsPage() {
  const { t } = useTranslation();
  const snapshot = useApp((s) => s.snapshot);
  const toast = useApp((s) => s.toast);
  const [groups, setGroups] = useState<ProjectGroup[] | null>(null);
  const byId = useMemo(() => new Map((snapshot?.listeners ?? []).map((l) => [l.id, l])), [snapshot]);
  // Worktree lists come from Git; re-read them when the set of listeners changes, not on
  // every refresh.
  const signature = useMemo(() => (snapshot?.listeners ?? []).map((l) => l.id).join("|"), [snapshot]);

  useEffect(() => {
    if (!snapshot) return;
    let alive = true;
    api
      .projects()
      .then((g) => alive && setGroups(g))
      .catch((e) => {
        if (alive) {
          setGroups([]);
          toast("danger", t("toast.failed", { error: errorText(e) }));
        }
      });
    return () => {
      alive = false;
    };
    // `snapshot` is read through the signature; toast and t are stable.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [signature]);

  return (
    <>
      <PageHeader title={t("projects.title")} subtitle={t("projects.subtitle")} />
      {groups === null ? (
        <div className="space-y-3">
          <Skeleton className="h-28 w-full" />
          <Skeleton className="h-28 w-full" />
        </div>
      ) : groups.length === 0 ? (
        <Card>
          <EmptyState icon={<FolderGit2 size={20} strokeWidth={1.75} />} title={t("projects.emptyTitle")} body={t("projects.emptyBody")} />
        </Card>
      ) : (
        <div className="space-y-3">
          {groups.map((g) => (
            <GroupCard key={g.key} group={g} byId={byId} />
          ))}
        </div>
      )}
    </>
  );
}
