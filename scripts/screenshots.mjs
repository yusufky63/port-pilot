// Captures screenshots of the running dev app over the WebView2 DevTools protocol, in both
// themes and at several window widths. No browser is launched: the app's own WebView is driven,
// so there is nothing to clean up afterwards.
//
// Start the app with remote debugging, then run this script:
//   $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS="--remote-debugging-port=9333"; pnpm app:dev
//   node scripts/screenshots.mjs [--out screenshots-tmp] [--pages ports,projects] [--widths 1440,760]
//
// Without --demo the pictures show this machine's real processes, paths and user name: they are
// for checking the UI, not for the repository.
//
// README screenshots: `node scripts/demo-projects.mjs`, start the app as it prints, then
//   node scripts/screenshots.mjs --demo [C:\PortPilotDemo]
// --demo starts a run in the demo worktree through the run dialog, pauses discovery, keeps only
// listeners and projects inside the demo folder, hides the Windows user name and writes
// light and dark pictures to docs/screenshots.

import { mkdir, writeFile } from "node:fs/promises";
import { join } from "node:path";

const args = process.argv.slice(2);
const opt = (name, fallback) => (args.includes(name) ? args[args.indexOf(name) + 1] : fallback);
const demoArg = args.includes("--demo") ? opt("--demo", null) : undefined;
const demo = demoArg === undefined ? null : (demoArg && !demoArg.startsWith("--") ? demoArg : "C:\\PortPilotDemo").replace(/\//g, "\\");
const outDir = opt("--out", demo ? "docs/screenshots" : "screenshots-tmp");
const port = process.env.CDP_PORT ?? "9333";
const pages = opt("--pages", demo ? "ports,projects,runs" : "ports,projects,doctor,settings").split(",");
const themes = opt("--themes", "light,dark").split(",");
const widths = opt("--widths", demo ? "1440" : "1440,760").split(",").map(Number);
const lang = opt("--lang", demo ? "en" : null);
const select = demo !== null || args.includes("--select");

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

async function target() {
  for (let i = 0; i < 180; i++) {
    try {
      const list = await (await fetch(`http://127.0.0.1:${port}/json/list`)).json();
      const page = list.find((t) => t.type === "page" && /localhost:1430|tauri\.localhost/.test(t.url));
      if (page) return page;
    } catch {
      // The app is still starting.
    }
    await sleep(1000);
  }
  throw new Error(`no app page on port ${port}`);
}

function connect(wsUrl) {
  const ws = new WebSocket(wsUrl);
  let id = 0;
  const pending = new Map();
  ws.addEventListener("message", (ev) => {
    const msg = JSON.parse(ev.data);
    if (msg.id && pending.has(msg.id)) {
      const { resolve, reject } = pending.get(msg.id);
      pending.delete(msg.id);
      if (msg.error) reject(new Error(msg.error.message));
      else resolve(msg.result);
    }
  });
  const ready = new Promise((r) => ws.addEventListener("open", r));
  const send = async (method, params = {}) => {
    await ready;
    const n = ++id;
    ws.send(JSON.stringify({ id: n, method, params }));
    return new Promise((resolve, reject) => pending.set(n, { resolve, reject }));
  };
  return { send, close: () => ws.close() };
}

const t = await target();
const cdp = connect(t.webSocketDebuggerUrl);
const evaluate = async (expression) => {
  const r = await cdp.send("Runtime.evaluate", { expression, awaitPromise: true, returnByValue: true });
  if (r.exceptionDetails) throw new Error(r.exceptionDetails.text + " " + (r.exceptionDetails.exception?.description ?? ""));
  return r.result.value;
};
const shot = async (name) => {
  const s = await cdp.send("Page.captureScreenshot", { format: "png" });
  const file = join(outDir, `${name}.png`);
  await writeFile(file, Buffer.from(s.data, "base64"));
  console.log("saved", file);
};

await mkdir(outDir, { recursive: true });
for (let i = 0; ; i++) {
  try {
    const ok = await evaluate(
      `(async () => { for (let i = 0; i < 100 && !window.__portpilot?.getState().snapshot; i++) await new Promise(r => setTimeout(r, 200)); return !!window.__portpilot?.getState().snapshot })()`,
    );
    if (ok) break;
  } catch (e) {
    if (i >= 20) throw e;
  }
  await sleep(1000);
}
if (lang) await evaluate(`window.__portpilotSetLang(${JSON.stringify(lang)})`);

if (demo) {
  const root = JSON.stringify(demo.toLowerCase().replace(/\\+$/, "") + "\\");
  const worktree = JSON.stringify(`${demo}\\shop-checkout`);
  // The profile folder as it appears inside JSON text, where every backslash is doubled:
  // C:\\Users\\name or C:/Users/name.
  const parts = (process.env.USERPROFILE ?? "").split(/[\\/]+/).filter(Boolean);
  const profileRe = parts.length > 1 ? parts.map((p) => p.replace(/[.*+?^$()|{}[\]\\]/g, "\\$&")).join("(?:\\\\\\\\|/)+") : "";
  await cdp.send("Emulation.setDeviceMetricsOverride", { width: widths[0], height: 860, deviceScaleFactor: 1, mobile: false });
  await evaluate(`document.documentElement.setAttribute('data-theme', 'light')`);
  const ready = await evaluate(`(async () => {
    const g = window.__portpilot.getState;
    const inside = (p) => String(p ?? "").toLowerCase().startsWith(${root});
    for (let i = 0; i < 100; i++) {
      const ports = new Set(g().snapshot.listeners.filter((l) => inside(l.project?.root)).map((l) => l.port));
      if ([3000, 5173, 8080].every((p) => ports.has(p))) return true;
      await new Promise((r) => setTimeout(r, 300));
    }
    return false;
  })()`);
  if (!ready) throw new Error(`the demo servers are not running; start them with node scripts/demo-projects.mjs`);
  // Listing the projects registers the worktree folders the run dialog may read.
  await evaluate(`window.__TAURI_INTERNALS__.invoke("projects_overview")`);
  await evaluate(`(() => { const s = window.__portpilot.getState(); s.go("runs"); s.openRun(${worktree}); })()`);
  await sleep(4000);
  await shot("run-dialog-light");
  const started = await evaluate(`(() => { const b = [...document.querySelectorAll("button")].find((b) => b.textContent.trim() === "Run once"); b?.click(); return !!b; })()`);
  if (!started) throw new Error(`the run dialog for ${worktree} did not offer "Run once"`);
  await sleep(2000);
  await evaluate(`window.__portpilot.getState().closeRun()`);
  // Wait for the worktree's run to show up as a listener before the picture is frozen.
  await evaluate(`(async () => {
    const inside = (p) => String(p ?? "").toLowerCase().startsWith(${JSON.stringify(`${demo.toLowerCase()}\\shop-checkout`)});
    for (let i = 0; i < 40 && !window.__portpilot.getState().snapshot.listeners.some((l) => inside(l.project?.root)); i++) {
      await new Promise((r) => setTimeout(r, 500));
    }
  })()`);
  // Freeze the picture: no new snapshots, only what lives in the demo folder, no user name.
  await evaluate(`(async () => {
    const s = window.__portpilot.getState();
    if (!s.paused) await s.togglePause();
    // Discovery stays paused underneath; the sidebar shows the usual live state.
    window.__portpilot.setState({ paused: false });
    const inside = (p) => String(p ?? "").toLowerCase().startsWith(${root});
    const profileRe = ${JSON.stringify(profileRe)};
    const scrub = (v) => {
      const text = JSON.stringify(v);
      return JSON.parse(profileRe ? text.replace(new RegExp(profileRe, "gi"), "%USERPROFILE%") : text);
    };
    const snap = s.snapshot;
    const listeners = snap.listeners
      .filter((l) => inside(l.project?.root))
      .map((l) => ({ ...l, process: l.process && { ...l.process, user: null } }));
    const ids = new Set(listeners.map((l) => l.id));
    const conflicts = snap.conflicts.filter((c) => c.listenerIds.every((id) => ids.has(id)));
    window.__portpilot.setState({ snapshot: scrub({ ...snap, listeners, conflicts, warnings: [] }) });
    // Tauri's invoke is read-only, but every command reply comes through window.fetch, so the
    // replies are filtered there.
    const send = window.fetch.bind(window);
    window.fetch = async (input, init) => {
      const res = await send(input, init);
      const cmd = String(input?.url ?? input).split("/").pop().split("?")[0];
      const edit =
        cmd === "projects_overview" ? (r) => scrub(r.filter((g) => inside(g.root))) : cmd === "app_info" ? (r) => ({ ...r, user: null }) : null;
      if (!edit || res.headers.get("Tauri-Response") !== "ok") return res;
      return new Response(JSON.stringify(edit(await res.json())), { status: res.status, headers: res.headers });
    };
  })()`);
}

for (const width of widths) {
  await cdp.send("Emulation.setDeviceMetricsOverride", { width, height: 860, deviceScaleFactor: 1, mobile: false });
  for (const theme of themes) {
    await evaluate(`document.documentElement.setAttribute('data-theme', ${JSON.stringify(theme)})`);
    for (const page of pages) {
      await evaluate(`(() => {
        const s = window.__portpilot.getState();
        s.go(${JSON.stringify(page)});
        if (${select} && ${JSON.stringify(page)} === "ports") {
          const ls = s.snapshot.listeners;
          const l = ls.find((l) => l.port === 5173) ?? ls.find((l) => l.project) ?? ls[0];
          s.select(l ? l.id : null);
        } else s.select(null);
        document.getElementById('main')?.scrollTo(0, 0);
      })()`);
      // The Projects page fetches its data when it opens.
      await sleep(page === "projects" ? 3500 : 1500);
      const suffix = demo ? "" : `-${width}${lang ? `-${lang}` : ""}`;
      await shot(`${page}${select && page === "ports" && !demo ? "-detail" : ""}-${theme}${suffix}`);
    }
  }
}
await cdp.send("Emulation.clearDeviceMetricsOverride");
if (demo) {
  console.log("Discovery stays paused and the window shows filtered data; restart the app before using it.");
} else {
  // Put the user's own theme and language back.
  await evaluate(`(() => {
    const s = window.__portpilot.getState().settings;
    const root = document.documentElement;
    if (s.theme === "light" || s.theme === "dark") root.setAttribute("data-theme", s.theme); else root.removeAttribute("data-theme");
    window.__portpilotSetLang(s.language);
    window.__portpilot.getState().go("ports");
    window.__portpilot.getState().select(null);
  })()`);
}
cdp.close();
