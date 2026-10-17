import { create } from "zustand";
import i18n, { setLanguage } from "../i18n";
import { api } from "../lib/api";
import { DEFAULT_FILTERS, type Filters } from "../lib/filter";
import type { AppInfo, Listener, RunInfo, Settings, Snapshot, UpdateInfo } from "../lib/types";

export type PageId = "ports" | "projects" | "runs" | "history" | "doctor" | "settings";

export interface Toast {
  id: number;
  tone: "positive" | "danger" | "warning" | "neutral";
  text: string;
}

interface AppState {
  page: PageId;
  ready: boolean;
  bootError: string | null;
  info: AppInfo | null;
  settings: Settings | null;
  snapshot: Snapshot | null;
  paused: boolean;
  filters: Filters;
  selectedId: string | null;
  /** The listener in the stop dialog, kept as it was when the dialog opened: the process may
   * leave the snapshot while the dialog still shows the result. */
  stopping: Listener | null;
  toasts: Toast[];
  /** Runs this app started, newest first. */
  runs: RunInfo[];
  /** The run in the stop dialog, opened from the Runs page. */
  stoppingRun: RunInfo | null;
  /** The run whose output is open on the Runs page. */
  selectedRunId: string | null;
  /** The folder whose recipe the run dialog shows. */
  runFolder: string | null;
  /** Set when quitting was asked for while runs are active: how many. */
  quitRequested: number | null;
  update: UpdateInfo | null;
  /** The version whose banner the user set aside for this session. */
  updateDismissed: string | null;

  go: (page: PageId) => void;
  boot: () => Promise<void>;
  setSnapshot: (s: Snapshot) => void;
  setPaused: (paused: boolean) => void;
  togglePause: () => Promise<void>;
  refresh: () => Promise<void>;
  setFilters: (patch: Partial<Filters>) => void;
  resetFilters: () => void;
  select: (id: string | null) => void;
  focusListener: (id: string) => void;
  openStop: (l: Listener) => void;
  closeStop: () => void;
  saveSettings: (patch: Partial<Settings>) => Promise<void>;
  toast: (tone: Toast["tone"], text: string) => void;
  dismissToast: (id: number) => void;
  refreshRuns: () => Promise<void>;
  selectRun: (id: string | null) => void;
  focusRun: (id: string) => void;
  openRun: (folder: string) => void;
  openRunStop: (run: RunInfo) => void;
  closeRun: () => void;
  setQuitRequested: (active: number | null) => void;
  setUpdate: (u: UpdateInfo | null) => void;
  dismissUpdate: (version: string | null) => void;
}

let toastId = 0;

export function errorText(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

export function applyTheme(pref: string) {
  const root = document.documentElement;
  if (pref === "light" || pref === "dark") root.setAttribute("data-theme", pref);
  else root.removeAttribute("data-theme");
  try {
    localStorage.setItem("portpilot:theme", pref);
  } catch {
    // Not fatal: the theme still applies for this session.
  }
}

function applyLanguage(pref: string) {
  const lang = setLanguage(pref);
  void api.setUiLanguage(lang).catch(() => undefined);
}

function loadFilters(): Filters {
  try {
    const raw = localStorage.getItem("portpilot:filters");
    if (!raw) return DEFAULT_FILTERS;
    // The search box starts empty; scope and toggles are remembered.
    return { ...DEFAULT_FILTERS, ...(JSON.parse(raw) as Partial<Filters>), query: "" };
  } catch {
    return DEFAULT_FILTERS;
  }
}

function saveFilters(f: Filters) {
  try {
    localStorage.setItem("portpilot:filters", JSON.stringify({ ...f, query: "" }));
  } catch {
    // Remembering filters is a convenience.
  }
}

export const useApp = create<AppState>((set, get) => ({
  page: "ports",
  ready: false,
  bootError: null,
  info: null,
  settings: null,
  snapshot: null,
  paused: false,
  filters: loadFilters(),
  selectedId: null,
  stopping: null,
  toasts: [],
  runs: [],
  stoppingRun: null,
  selectedRunId: null,
  runFolder: null,
  quitRequested: null,
  update: null,
  updateDismissed: null,

  go: (page) => set({ page }),

  boot: async () => {
    try {
      const [info, settings, snapshot] = await Promise.all([api.appInfo(), api.settings(), api.snapshot()]);
      applyTheme(settings.theme);
      applyLanguage(settings.language);
      set({ info, settings, snapshot, paused: info.paused, ready: true });
      void get().refreshRuns();
      void api.updateStatus().then((update) => set({ update }), () => undefined);
    } catch (e) {
      set({ bootError: errorText(e), ready: true });
    }
  },

  setSnapshot: (snapshot) => {
    set({ snapshot });
    // Run states change with their processes; a snapshot is a good moment to ask.
    void get().refreshRuns();
  },

  setPaused: (paused) => set({ paused }),

  togglePause: async () => {
    const next = !get().paused;
    set({ paused: next });
    try {
      await api.setPaused(next);
    } catch (e) {
      set({ paused: !next });
      get().toast("danger", i18n.t("toast.failed", { error: errorText(e) }));
    }
  },

  refresh: async () => {
    try {
      await api.refresh();
    } catch (e) {
      get().toast("danger", i18n.t("toast.failed", { error: errorText(e) }));
    }
  },

  setFilters: (patch) => {
    const filters = { ...get().filters, ...patch };
    saveFilters(filters);
    set({ filters });
  },

  resetFilters: () => {
    saveFilters(DEFAULT_FILTERS);
    set({ filters: DEFAULT_FILTERS });
  },

  select: (selectedId) => set({ selectedId }),

  focusListener: (id) => {
    // Coming from the tray: show the listener even if the current filters would hide it.
    const filters = { ...DEFAULT_FILTERS, protocol: "both" as const, showSystem: true };
    set({ page: "ports", selectedId: id, filters });
  },

  openStop: (stopping) => set({ stopping }),

  closeStop: () => set({ stopping: null, stoppingRun: null }),

  saveSettings: async (patch) => {
    const current = get().settings;
    if (!current) return;
    const next = { ...current, ...patch };
    set({ settings: next });
    if (patch.theme) applyTheme(next.theme);
    if (patch.language) applyLanguage(next.language);
    try {
      const saved = await api.saveSettings(next);
      const info = get().info;
      set({ settings: saved, info: info ? { ...info, settingsError: null } : info });
    } catch (e) {
      set({ settings: current });
      get().toast("danger", i18n.t("toast.failed", { error: errorText(e) }));
    }
  },

  toast: (tone, text) => {
    const id = ++toastId;
    set({ toasts: [...get().toasts, { id, tone, text }].slice(-4) });
    setTimeout(() => get().dismissToast(id), tone === "danger" ? 8000 : 4000);
  },

  dismissToast: (id) => set({ toasts: get().toasts.filter((t) => t.id !== id) }),

  refreshRuns: async () => {
    try {
      const runs = await api.runs();
      const selected = get().selectedRunId;
      set({ runs, selectedRunId: selected && runs.some((r) => r.id === selected) ? selected : (runs[0]?.id ?? null) });
    } catch {
      // The list stays as it was; the next snapshot asks again.
    }
  },

  selectRun: (selectedRunId) => set({ selectedRunId }),

  focusRun: (id) => set({ page: "runs", selectedRunId: id }),

  openRun: (runFolder) => set({ runFolder }),

  openRunStop: (stoppingRun) => set({ stoppingRun }),

  closeRun: () => set({ runFolder: null }),

  setQuitRequested: (quitRequested) => set({ quitRequested }),

  setUpdate: (update) => set({ update }),

  dismissUpdate: (updateDismissed) => set({ updateDismissed }),
}));
