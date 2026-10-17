import { FolderGit2, Network, Pause, Play, Settings as SettingsIcon, ShieldCheck, Stethoscope, User, X, type LucideIcon } from "lucide-react";
import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { cx } from "../lib/cx";
import { localTime } from "../lib/format";
import { useLocale } from "../lib/hooks";
import { useApp, type PageId } from "../store/app";

interface NavItem {
  id: PageId;
  icon: LucideIcon;
}

const ITEMS: NavItem[] = [
  { id: "ports", icon: Network },
  { id: "projects", icon: FolderGit2 },
  { id: "doctor", icon: Stethoscope },
];

export function Logo({ size = 26 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 1024 1024" aria-hidden>
      <rect width="1024" height="1024" rx="224" fill="var(--primary-fill)" />
      <path d="M264 296H760V632H648V744H376V632H264Z" fill="none" stroke="var(--primary-contrast)" strokeWidth="72" strokeLinejoin="round" />
      <path d="M408 296V420M512 296V420M616 296V420" stroke="var(--primary-contrast)" strokeWidth="56" strokeLinecap="round" />
    </svg>
  );
}

function NavButton({ item, active, onClick, badge }: { item: NavItem; active: boolean; onClick: () => void; badge?: number }) {
  const { t } = useTranslation();
  const Icon = item.icon;
  return (
    <button
      type="button"
      onClick={onClick}
      aria-current={active ? "page" : undefined}
      className={cx(
        "rail flex h-9 w-full items-center gap-2.5 rounded-md pl-3 pr-2 text-left text-[14px] transition-colors transition-fast",
        active ? "rail-active bg-primary-soft font-medium text-ink" : "text-ink-secondary hover:bg-surface-muted hover:text-ink",
      )}
    >
      <Icon size={17} strokeWidth={1.75} className={active ? "text-primary" : ""} aria-hidden />
      <span className="flex-1 truncate">{t(`nav.${item.id}`)}</span>
      {badge ? <span className="num rounded-sm bg-danger-soft px-1.5 font-mono text-[11px] text-danger-fg">{badge}</span> : null}
    </button>
  );
}

function StatusFooter() {
  const { t } = useTranslation();
  const locale = useLocale();
  const snapshot = useApp((s) => s.snapshot);
  const paused = useApp((s) => s.paused);
  const info = useApp((s) => s.info);
  const togglePause = useApp((s) => s.togglePause);
  return (
    <div className="space-y-2 border-t border-line px-4 py-3 text-[12px]">
      <div className="flex items-center gap-2">
        <span className={cx("h-2 w-2 shrink-0 rounded-full", paused ? "bg-warning" : "bg-positive")} aria-hidden />
        <span className="font-medium text-ink">{paused ? t("status.paused") : t("status.live")}</span>
        <button
          type="button"
          onClick={() => void togglePause()}
          className="ml-auto inline-flex h-6 items-center gap-1 rounded-md px-1.5 text-ink-secondary hover:bg-surface-muted hover:text-ink"
          title={paused ? t("common.resume") : t("common.pause")}
        >
          {paused ? <Play size={12} strokeWidth={2} /> : <Pause size={12} strokeWidth={2} />}
          <span>{paused ? t("common.resume") : t("common.pause")}</span>
        </button>
      </div>
      <div className="font-mono text-[11px] text-ink-muted">
        {snapshot ? t("status.updated", { time: localTime(snapshot.takenAt, locale) }) : t("status.waiting")}
      </div>
      <div className="flex flex-wrap items-center gap-1.5 text-[11px] text-ink-muted">
        {info?.elevated ? <ShieldCheck size={13} strokeWidth={1.75} className="text-warning-fg" /> : <User size={13} strokeWidth={1.75} />}
        <span>{info?.elevated ? t("status.admin") : t("status.standard")}</span>
        {info?.portable && <span className="ml-auto rounded-sm bg-surface-muted px-1.5 font-mono text-[10px]">{t("status.portable")}</span>}
      </div>
    </div>
  );
}

function Sidebar() {
  const { t } = useTranslation();
  const page = useApp((s) => s.page);
  const go = useApp((s) => s.go);
  const conflicts = useApp((s) => s.snapshot?.conflicts.length ?? 0);
  return (
    <aside className="flex h-full w-[216px] shrink-0 flex-col border-r border-line bg-surface">
      <div className="flex h-14 items-center gap-2.5 px-4">
        <Logo />
        <div className="leading-tight">
          <div className="display text-[16px] text-ink">{t("app.name")}</div>
          <div className="font-mono text-[10px] uppercase tracking-[0.12em] text-ink-muted">{t("app.tagline")}</div>
        </div>
      </div>
      <nav className="flex-1 space-y-0.5 overflow-y-auto px-3 pb-3 pt-2" aria-label={t("nav.main")}>
        {ITEMS.map((item) => (
          <NavButton key={item.id} item={item} active={page === item.id} onClick={() => go(item.id)} badge={item.id === "ports" ? conflicts : undefined} />
        ))}
      </nav>
      <div className="border-t border-line px-3 py-3">
        <NavButton item={{ id: "settings", icon: SettingsIcon }} active={page === "settings"} onClick={() => go("settings")} />
      </div>
      <StatusFooter />
    </aside>
  );
}

function Toasts() {
  const { t } = useTranslation();
  const toasts = useApp((s) => s.toasts);
  const dismiss = useApp((s) => s.dismissToast);
  return (
    <div className="z-toast pointer-events-none fixed bottom-5 right-5 flex w-[360px] flex-col gap-2" aria-live="polite">
      {toasts.map((toast) => (
        <div
          key={toast.id}
          className={cx(
            "anim-rise pointer-events-auto flex items-start gap-3 rounded-md border bg-canvas px-3.5 py-3 text-[14px] shadow-popover",
            toast.tone === "positive" ? "border-positive" : toast.tone === "danger" ? "border-danger" : toast.tone === "warning" ? "border-warning" : "border-line",
          )}
        >
          <span className={cx("mt-1.5 h-2 w-2 shrink-0 rounded-full", toast.tone === "positive" ? "bg-positive" : toast.tone === "danger" ? "bg-danger" : toast.tone === "warning" ? "bg-warning" : "bg-primary")} />
          <span className="selectable min-w-0 flex-1 text-ink">{toast.text}</span>
          <button type="button" aria-label={t("common.close")} onClick={() => dismiss(toast.id)} className="text-ink-muted hover:text-ink">
            <X size={15} strokeWidth={1.75} />
          </button>
        </div>
      ))}
    </div>
  );
}

export function Shell({ children, aside }: { children: ReactNode; aside?: ReactNode }) {
  return (
    <div className="app-shell flex h-full">
      <Sidebar />
      <main className="relative min-w-0 flex-1 overflow-y-auto" id="main">
        <div className="mx-auto w-full max-w-[1320px] px-6 pb-6 pt-6">{children}</div>
      </main>
      {aside}
      <Toasts />
    </div>
  );
}
