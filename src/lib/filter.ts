import type { LauncherKind, Listener, Runtime } from "./types";

const AGENTS: LauncherKind[] = ["claude", "codex", "gemini"];

export type Scope = "all" | "conflicts" | "exposed";
export type ProtocolFilter = "tcp" | "udp" | "both";

export interface Filters {
  query: string;
  scope: Scope;
  /** Empty means every runtime. */
  runtimes: Runtime[];
  protocol: ProtocolFilter;
  showSystem: boolean;
}

export const DEFAULT_FILTERS: Filters = { query: "", scope: "all", runtimes: [], protocol: "tcp", showSystem: false };

/** Windows' own listeners: services and the kernel. Forwarders for WSL and Docker stay visible. */
export function isSystem(l: Listener): boolean {
  return l.process?.class === "system" || l.process?.class === "service";
}

export function isExposed(l: Listener): boolean {
  return l.exposure !== "loopback";
}

function haystack(l: Listener): string[] {
  const out: (string | null | undefined)[] = [
    l.process?.name,
    l.process?.path,
    l.project?.name,
    l.project?.root,
    l.project?.git?.repoName,
    l.project?.git?.branch,
    l.project?.git?.topLevel,
    l.container?.name,
    l.container?.image,
    l.container?.composeProject,
    l.container?.composeService,
    l.wsl?.distro,
    l.launcher?.name,
    l.relay?.name,
    l.runtime,
    ...l.endpoints.map((e) => e.address),
  ];
  return out.filter((s): s is string => !!s).map((s) => s.toLowerCase());
}

/** Every whitespace-separated token must match. Numbers match port prefixes and exact PIDs. */
export function matchesQuery(l: Listener, query: string): boolean {
  const tokens = query.toLowerCase().split(/\s+/).filter(Boolean);
  if (tokens.length === 0) return true;
  const hay = haystack(l);
  return tokens.every((t) => {
    if (/^\d+$/.test(t)) {
      if (String(l.port).startsWith(t)) return true;
      if (l.process && String(l.process.pid) === t) return true;
    }
    if (t.startsWith(":") && /^:\d+$/.test(t)) return String(l.port) === t.slice(1);
    return hay.some((h) => h.includes(t));
  });
}

export function applyFilters(listeners: Listener[], f: Filters): Listener[] {
  return listeners.filter((l) => {
    if (f.protocol !== "both" && l.protocol !== f.protocol) return false;
    if (!f.showSystem && isSystem(l)) return false;
    if (f.scope === "conflicts" && !l.conflict) return false;
    if (f.scope === "exposed" && !isExposed(l)) return false;
    if (f.runtimes.length > 0 && !f.runtimes.includes(l.runtime)) return false;
    return matchesQuery(l, f.query);
  });
}

/** A query that is exactly one port number. */
export function portQuery(query: string): number | null {
  const q = query.trim().replace(/^:/, "");
  if (!/^\d{1,5}$/.test(q)) return null;
  const n = Number(q);
  return n >= 1 && n <= 65535 ? n : null;
}

export interface Counts {
  total: number;
  conflicts: number;
  exposed: number;
  windows: number;
  wsl: number;
  docker: number;
  /** Started by Claude, Codex or Gemini CLI. */
  agents: number;
}

/** Counts over the listeners the default view would show (TCP, no system services). */
export function counts(listeners: Listener[]): Counts {
  const visible = listeners.filter((l) => l.protocol === "tcp" && !isSystem(l));
  return {
    total: visible.length,
    conflicts: new Set(visible.filter((l) => l.conflict).map((l) => l.port)).size,
    exposed: visible.filter(isExposed).length,
    windows: visible.filter((l) => l.runtime === "windows").length,
    wsl: visible.filter((l) => l.runtime === "wsl").length,
    docker: visible.filter((l) => l.runtime === "docker").length,
    agents: visible.filter((l) => AGENTS.includes(l.launcher?.kind ?? "terminal")).length,
  };
}
