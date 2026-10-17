// Captures screenshots of the running dev app over the WebView2 DevTools protocol, for local UI
// checks in both themes and at several window widths. No browser is launched: the app's own
// WebView is driven, so there is nothing to clean up afterwards.
//
// Start the app with remote debugging, then run this script:
//   $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS="--remote-debugging-port=9333"; pnpm app:dev
//   node scripts/screenshots.mjs [--out screenshots-tmp] [--pages ports,projects] [--widths 1440,760]
//
// The pictures show this machine's real processes, paths and user name. They are for checking
// the UI, not for the repository.

import { mkdir, writeFile } from "node:fs/promises";
import { join } from "node:path";

const args = process.argv.slice(2);
const opt = (name, fallback) => (args.includes(name) ? args[args.indexOf(name) + 1] : fallback);
const outDir = opt("--out", "screenshots-tmp");
const port = process.env.CDP_PORT ?? "9333";
const pages = opt("--pages", "ports,projects,doctor,settings").split(",");
const themes = opt("--themes", "light,dark").split(",");
const widths = opt("--widths", "1440,760").split(",").map(Number);
const lang = opt("--lang", null);
const select = args.includes("--select");

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

for (const width of widths) {
  await cdp.send("Emulation.setDeviceMetricsOverride", { width, height: 860, deviceScaleFactor: 1, mobile: false });
  for (const theme of themes) {
    await evaluate(`document.documentElement.setAttribute('data-theme', ${JSON.stringify(theme)})`);
    for (const page of pages) {
      await evaluate(`(() => {
        const s = window.__portpilot.getState();
        s.go(${JSON.stringify(page)});
        if (${select} && ${JSON.stringify(page)} === "ports") {
          const l = s.snapshot.listeners.find((l) => l.project) ?? s.snapshot.listeners[0];
          s.select(l ? l.id : null);
        } else s.select(null);
        document.getElementById('main')?.scrollTo(0, 0);
      })()`);
      await sleep(1200);
      const shot = await cdp.send("Page.captureScreenshot", { format: "png" });
      const file = join(outDir, `${page}${select && page === "ports" ? "-detail" : ""}-${theme}-${width}${lang ? `-${lang}` : ""}.png`);
      await writeFile(file, Buffer.from(shot.data, "base64"));
      console.log("saved", file);
    }
  }
}
await cdp.send("Emulation.clearDeviceMetricsOverride");
// Put the user's own theme and language back.
await evaluate(`(() => {
  const s = window.__portpilot.getState().settings;
  const root = document.documentElement;
  if (s.theme === "light" || s.theme === "dark") root.setAttribute("data-theme", s.theme); else root.removeAttribute("data-theme");
  window.__portpilotSetLang(s.language);
  window.__portpilot.getState().go("ports");
  window.__portpilot.getState().select(null);
})()`);
cdp.close();
