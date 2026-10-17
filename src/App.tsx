import { AlertTriangle, MonitorX } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { DetailPanel } from "./components/DetailPanel";
import { RunDialog } from "./components/RunDialog";
import { StopDialog } from "./components/StopDialog";
import { Shell } from "./components/Shell";
import { Banner, Button, Dialog, EmptyState } from "./components/ui";
import { UpdateBanner } from "./components/UpdateBanner";
import { api, isDesktop, onFocusListener, onPaused, onQuitRequested, onShowUpdate, onSnapshot, onUpdateAvailable } from "./lib/api";
import { useMediaQuery } from "./lib/hooks";
import { DoctorPage } from "./pages/Doctor";
import { HistoryPage } from "./pages/History";
import { PortsPage } from "./pages/Ports";
import { ProjectsPage } from "./pages/Projects";
import { RunsPage } from "./pages/Runs";
import { SettingsPage } from "./pages/Settings";
import { errorText, useApp } from "./store/app";

function Page() {
  const page = useApp((s) => s.page);
  switch (page) {
    case "ports":
      return <PortsPage />;
    case "projects":
      return <ProjectsPage />;
    case "runs":
      return <RunsPage />;
    case "history":
      return <HistoryPage />;
    case "doctor":
      return <DoctorPage />;
    case "settings":
      return <SettingsPage />;
  }
}

function Detail() {
  const page = useApp((s) => s.page);
  const selectedId = useApp((s) => s.selectedId);
  const snapshot = useApp((s) => s.snapshot);
  const select = useApp((s) => s.select);
  // Side by side on wide windows, a drawer over the list on narrow ones.
  const wide = useMediaQuery("(min-width: 1280px)");
  const listener = useMemo(() => snapshot?.listeners.find((l) => l.id === selectedId) ?? null, [snapshot, selectedId]);
  if (page !== "ports" || !listener) return null;
  return <DetailPanel listener={listener} overlay={!wide} onClose={() => select(null)} />;
}

function Stop() {
  const stopping = useApp((s) => s.stopping);
  const stoppingRun = useApp((s) => s.stoppingRun);
  const closeStop = useApp((s) => s.closeStop);
  // Keyed so a new dialog never shows the previous result.
  if (stoppingRun) return <StopDialog key={stoppingRun.id} run={stoppingRun} onClose={closeStop} />;
  if (!stopping) return null;
  return <StopDialog key={stopping.id} listener={stopping} onClose={closeStop} />;
}

function Run() {
  const folder = useApp((s) => s.runFolder);
  const closeRun = useApp((s) => s.closeRun);
  if (!folder) return null;
  return <RunDialog key={folder} folder={folder} onClose={closeRun} />;
}

/** Quitting with active runs: they are stopped first, and ended after the grace period. */
function Quit() {
  const { t } = useTranslation();
  const active = useApp((s) => s.quitRequested);
  const grace = useApp((s) => s.settings?.graceSeconds ?? 5);
  const close = useApp((s) => s.setQuitRequested);
  const toast = useApp((s) => s.toast);
  const [busy, setBusy] = useState(false);
  if (active === null) return null;
  const quit = () => {
    setBusy(true);
    api.quit().catch((e) => {
      setBusy(false);
      toast("danger", t("toast.failed", { error: errorText(e) }));
    });
  };
  return (
    <Dialog
      open
      onClose={busy ? () => undefined : () => close(null)}
      title={t("quit.title")}
      footer={
        <>
          <Button variant="ghost" disabled={busy} onClick={() => close(null)} data-autofocus>
            {t("common.cancel")}
          </Button>
          <Button variant="dangerSolid" loading={busy} onClick={quit}>
            {busy ? t("quit.stopping") : t("quit.confirm")}
          </Button>
        </>
      }
    >
      <p className="text-[14px] text-ink-secondary">{t("quit.body", { count: active, seconds: grace })}</p>
    </Dialog>
  );
}

export function App() {
  const { t } = useTranslation();
  const boot = useApp((s) => s.boot);
  const ready = useApp((s) => s.ready);
  const bootError = useApp((s) => s.bootError);

  useEffect(() => {
    if (!isDesktop()) return;
    void boot();
    const subs = [
      onSnapshot((s) => useApp.getState().setSnapshot(s)),
      onPaused((p) => useApp.getState().setPaused(p)),
      onFocusListener((id) => useApp.getState().focusListener(id)),
      onQuitRequested((n) => useApp.getState().setQuitRequested(n)),
      onUpdateAvailable((u) => useApp.getState().setUpdate(u)),
      onShowUpdate(() => useApp.getState().dismissUpdate(null)),
    ];
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape" && !document.querySelector("[role=dialog]")) useApp.getState().select(null);
    };
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("keydown", onKey);
      for (const s of subs) void s.then((off) => off());
    };
  }, [boot]);

  if (!isDesktop()) {
    return (
      <div className="flex h-full items-center justify-center">
        <EmptyState icon={<MonitorX size={20} strokeWidth={1.75} />} title={t("app.name")} body={t("common.desktopOnly")} />
      </div>
    );
  }
  if (!ready) return <div className="h-full bg-canvas" />;
  return (
    <Shell aside={<Detail />}>
      {bootError && (
        <div className="mb-4">
          <Banner tone="danger" icon={<AlertTriangle size={16} strokeWidth={1.75} />}>
            {bootError}
          </Banner>
        </div>
      )}
      <UpdateBanner />
      <Page />
      <Stop />
      <Run />
      <Quit />
    </Shell>
  );
}
