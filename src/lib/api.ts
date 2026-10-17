import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  AppInfo,
  Environment,
  HistoryEvent,
  LeaseView,
  OutputChunk,
  PortDiagnosis,
  ProjectGroup,
  RecentFolder,
  RecipeResult,
  RunInfo,
  RunPlan,
  Settings,
  Snapshot,
  StopReport,
  TrustEntry,
  UpdateInfo,
} from "./types";

export function isDesktop(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

/** Error text from a Rust command; commands return plain strings as errors. */
export class ApiError extends Error {}

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (!isDesktop()) throw new ApiError("This view needs the PortPilot desktop app.");
  try {
    return await invoke<T>(cmd, args);
  } catch (e) {
    throw new ApiError(typeof e === "string" ? e : e instanceof Error ? e.message : String(e));
  }
}

export const api = {
  appInfo: () => call<AppInfo>("app_info"),
  snapshot: () => call<Snapshot | null>("snapshot_get"),
  refresh: () => call<void>("refresh_now"),
  setPaused: (paused: boolean) => call<void>("set_paused", { paused }),
  settings: () => call<Settings>("settings_get"),
  saveSettings: (settings: Settings) => call<Settings>("settings_set", { settings }),
  setUiLanguage: (language: string) => call<void>("set_ui_language", { language }),
  stop: (listenerId: string, force: boolean, tree: boolean) => call<StopReport>("stop_listener", { listenerId, force, tree }),
  diagnose: (port: number) => call<PortDiagnosis>("diagnose_port", { port }),
  findFree: (range: string, count: number) => call<number[]>("find_free", { range, count }),
  environment: () => call<Environment>("environment"),
  projects: () => call<ProjectGroup[]>("projects_overview"),
  openUrl: (listenerId: string) => call<void>("open_url", { listenerId }),
  openPath: (path: string, how: "folder" | "terminal" | "editor") => call<void>("open_path", { path, how }),
  quit: () => call<void>("quit_app"),
  pickFolder: () => call<string | null>("pick_project_folder"),
  recentProjects: () => call<RecentFolder[]>("recent_projects"),
  recipe: (path: string) => call<RecipeResult>("recipe_load", { path }),
  runPlan: (path: string, run: string) => call<RunPlan>("run_plan", { path, run }),
  runReallocate: (path: string, run: string, service: string) => call<RunPlan>("run_reallocate", { path, run, service }),
  runStart: (path: string, run: string, digest: string, ports: Record<string, number>, trust: boolean) =>
    call<RunInfo>("run_start", { path, run, digest, ports, trust }),
  runs: () => call<RunInfo[]>("runs_list"),
  runOutput: (runId: string, after: number) => call<OutputChunk>("run_output", { runId, after }),
  runOutputText: (runId: string) => call<string>("run_output_text", { runId }),
  runStop: (runId: string, force: boolean) => call<StopReport>("run_stop", { runId, force }),
  runForget: (runId: string) => call<boolean>("run_forget", { runId }),
  leases: () => call<LeaseView[]>("leases_list"),
  leaseRelease: (ownerKey: string, service: string) => call<void>("lease_release", { ownerKey, service }),
  history: (kind: "all" | "runs" | "ports", before: [number, number] | null, limit: number) =>
    call<HistoryEvent[]>("history_list", { kind: kind === "all" ? null : kind, before, limit }),
  historyClear: () => call<void>("history_clear"),
  trusted: () => call<TrustEntry[]>("trust_list"),
  trustRevoke: (projectKey: string, run: string) => call<void>("trust_revoke", { projectKey, run }),
  updateStatus: () => call<UpdateInfo | null>("update_status"),
  updateCheck: () => call<UpdateInfo | null>("update_check"),
  updateInstall: () => call<void>("update_install"),
  openReleasePage: () => call<void>("open_release_page"),
};

export function onSnapshot(cb: (s: Snapshot) => void): Promise<UnlistenFn> {
  return listen<Snapshot>("snapshot", (e) => cb(e.payload));
}

export function onPaused(cb: (paused: boolean) => void): Promise<UnlistenFn> {
  return listen<boolean>("paused", (e) => cb(e.payload));
}

export function onFocusListener(cb: (id: string) => void): Promise<UnlistenFn> {
  return listen<string>("focus-listener", (e) => cb(e.payload));
}

export function onUpdateAvailable(cb: (u: UpdateInfo) => void): Promise<UnlistenFn> {
  return listen<UpdateInfo>("update-available", (e) => cb(e.payload));
}

/** The tray's update entry was chosen. */
export function onShowUpdate(cb: () => void): Promise<UnlistenFn> {
  return listen("show-update", () => cb());
}

/** Quit was asked for while runs are active; the payload is how many. */
export function onQuitRequested(cb: (active: number) => void): Promise<UnlistenFn> {
  return listen<number>("quit-requested", (e) => cb(e.payload));
}
