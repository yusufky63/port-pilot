import { AlertTriangle, MonitorX } from "lucide-react";
import { useEffect, useMemo } from "react";
import { useTranslation } from "react-i18next";
import { DetailPanel } from "./components/DetailPanel";
import { StopDialog } from "./components/StopDialog";
import { Shell } from "./components/Shell";
import { Banner, EmptyState } from "./components/ui";
import { isDesktop, onFocusListener, onPaused, onSnapshot } from "./lib/api";
import { useMediaQuery } from "./lib/hooks";
import { DoctorPage } from "./pages/Doctor";
import { PortsPage } from "./pages/Ports";
import { ProjectsPage } from "./pages/Projects";
import { SettingsPage } from "./pages/Settings";
import { useApp } from "./store/app";

function Page() {
  const page = useApp((s) => s.page);
  switch (page) {
    case "ports":
      return <PortsPage />;
    case "projects":
      return <ProjectsPage />;
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
  const closeStop = useApp((s) => s.closeStop);
  if (!stopping) return null;
  // Keyed by listener so a new dialog never shows the previous result.
  return <StopDialog key={stopping.id} listener={stopping} onClose={closeStop} />;
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
      <Page />
      <Stop />
    </Shell>
  );
}
