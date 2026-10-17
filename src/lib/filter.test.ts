import { describe, expect, it } from "vitest";
import { applyFilters, counts, DEFAULT_FILTERS, matchesQuery, portQuery } from "./filter";
import type { Listener } from "./types";

function listener(over: Partial<Listener> & { port: number }): Listener {
  return {
    id: `windows:tcp:${over.port}:1`,
    protocol: "tcp",
    endpoints: [{ address: "127.0.0.1", port: over.port, exposure: "loopback" }],
    exposure: "loopback",
    runtime: "windows",
    process: { pid: 100 + over.port, start: 1, startedAt: null, name: "node.exe", path: null, commandLine: null, user: null, class: "user", accessDenied: false },
    ancestors: [],
    launcher: null,
    relay: null,
    container: null,
    wsl: null,
    project: null,
    url: null,
    stop: { target: null, blocked: null },
    conflict: false,
    notes: [],
    ...over,
  };
}

const vite = listener({
  port: 5173,
  project: {
    root: "C:\\src\\shop",
    name: "shop",
    confidence: "inferred",
    source: "commandLine",
    manifests: ["package.json"],
    git: { topLevel: "C:\\src\\shop", repoName: "shop", commonDir: "C:\\src\\shop\\.git", branch: "feat/auth", head: "5f32a0d", dirty: false, linkedWorktree: true },
    openPath: "C:\\src\\shop",
  },
});
const svc = listener({ port: 135, process: { pid: 900, start: 1, startedAt: null, name: "svchost.exe", path: null, commandLine: null, user: null, class: "service", accessDenied: true } });
const redis = listener({ port: 6379, runtime: "docker", process: null, exposure: "all", endpoints: [{ address: "0.0.0.0", port: 6379, exposure: "all" }] });
const dns = listener({ port: 5353, protocol: "udp" });
const a = listener({ port: 3000, conflict: true });
const b = listener({ port: 3000, conflict: true, id: "windows:tcp:3000:2", runtime: "wsl" });
const all = [vite, svc, redis, dns, a, b];

describe("filters", () => {
  it("hides system services and UDP by default", () => {
    expect(applyFilters(all, DEFAULT_FILTERS).map((l) => l.port)).toEqual([5173, 6379, 3000, 3000]);
    expect(applyFilters(all, { ...DEFAULT_FILTERS, showSystem: true, protocol: "both" })).toHaveLength(6);
    expect(applyFilters(all, { ...DEFAULT_FILTERS, protocol: "udp" }).map((l) => l.port)).toEqual([5353]);
  });

  it("scopes and runtimes", () => {
    expect(applyFilters(all, { ...DEFAULT_FILTERS, scope: "conflicts" })).toHaveLength(2);
    expect(applyFilters(all, { ...DEFAULT_FILTERS, scope: "exposed" }).map((l) => l.port)).toEqual([6379]);
    expect(applyFilters(all, { ...DEFAULT_FILTERS, runtimes: ["docker", "wsl"] }).map((l) => l.runtime)).toEqual(["docker", "wsl"]);
  });

  it("searches ports, pids, projects and branches", () => {
    expect(matchesQuery(vite, "51")).toBe(true);
    expect(matchesQuery(vite, ":5173")).toBe(true);
    expect(matchesQuery(vite, ":517")).toBe(false);
    expect(matchesQuery(vite, String(100 + 5173))).toBe(true);
    expect(matchesQuery(vite, "shop auth")).toBe(true);
    expect(matchesQuery(vite, "shop docker")).toBe(false);
    expect(matchesQuery(redis, "docker")).toBe(true);
    expect(matchesQuery(vite, "")).toBe(true);
  });

  it("reads a single port from the query", () => {
    expect(portQuery("3000")).toBe(3000);
    expect(portQuery(" :8080 ")).toBe(8080);
    expect(portQuery("70000")).toBeNull();
    expect(portQuery("0")).toBeNull();
    expect(portQuery("node")).toBeNull();
  });

  it("counts what the default view shows", () => {
    expect(counts(all)).toEqual({ total: 4, conflicts: 1, exposed: 1, windows: 2, wsl: 1, docker: 1, agents: 0 });
    expect(counts([{ ...vite, launcher: { kind: "claude", pid: 1, name: "claude.exe" } }]).agents).toBe(1);
  });
});
