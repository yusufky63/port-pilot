// Demo projects for README screenshots, so pictures never show a real machine's projects.
//
//   node scripts/demo-projects.mjs [C:\PortPilotDemo]          create the projects, start the servers
//   node scripts/demo-projects.mjs [C:\PortPilotDemo] --clean  stop them and delete the folder
//
// It creates `shop` (a monorepo with a linked worktree on feat/checkout), `api` (listens on every
// interface) and `docs` (two servers on port 3000, IPv4 and IPv6: a conflict), starts docs
// directly and shop and api through `portctl run`, and keeps PortPilot's leases and history in
// <root>\.data. Start the app with PORTPILOT_DATA_DIR set to that folder (the script prints the
// command), then run `node scripts/screenshots.mjs --demo <root>`.

import { execFileSync, spawn } from "node:child_process";
import { existsSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const args = process.argv.slice(2);
const root = args.find((a) => !a.startsWith("--")) ?? "C:\\PortPilotDemo";
const portctl = join(import.meta.dirname, "..", "target", "debug", "portctl.exe");
const data = join(root, ".data");
const env = { ...process.env, PORTPILOT_DATA_DIR: data };

if (!existsSync(portctl)) throw new Error("Build portctl first: cargo build -p portpilot-cli");

if (args.includes("--clean")) {
  const out = execFileSync(portctl, ["list", "--json", "--no-wsl", "--no-docker"], { env, encoding: "utf8" });
  const inside = (p) => String(p ?? "").toLowerCase().startsWith(root.toLowerCase());
  for (const l of JSON.parse(out).listeners) {
    if (!inside(l.project?.root) || !l.process) continue;
    try {
      execFileSync(portctl, ["stop", String(l.port), "--pid", String(l.process.pid), "--force", "--yes", "--no-wsl", "--no-docker"], { env, stdio: "inherit" });
    } catch {
      // Already gone together with its run.
    }
  }
  rmSync(root, { recursive: true, force: true });
  console.log(`removed ${root}`);
  process.exit(0);
}

const git = (cwd, ...a) => execFileSync("git", ["-c", "user.name=demo", "-c", "user.email=demo@example.invalid", "-C", cwd, ...a], { stdio: "ignore" });
const file = (path, text) => {
  mkdirSync(join(path, ".."), { recursive: true });
  writeFileSync(path, text);
};
const server = `// Demo dev server for PortPilot screenshots.
import http from "node:http";
const [host = "127.0.0.1", port = "3000"] = process.argv.slice(2);
http
  .createServer((_req, res) => res.end("ok\\n"))
  .listen(Number(port), host, () => console.log(\`ready on \${host.includes(":") ? \`[\${host}]\` : host}:\${port}\`));
`;

rmSync(root, { recursive: true, force: true });
mkdirSync(data, { recursive: true });

const shop = join(root, "shop");
file(join(shop, "package.json"), '{ "name": "shop", "private": true }\n');
file(join(shop, "apps", "web", "package.json"), '{ "name": "shop-web", "private": true, "type": "module" }\n');
file(join(shop, "apps", "web", "server.mjs"), server);
file(
  join(shop, ".portpilot.toml"),
  '[project]\nname = "shop"\n\n[ports.web]\npreferred = 5173\nrange = "5173-5199"\n\n[run.dev]\ncommand = "node server.mjs 127.0.0.1 {ports.web}"\ncwd = "apps/web"\n',
);
git(shop, "init", "-q", "-b", "main");
git(shop, "add", ".");
git(shop, "commit", "-qm", "Initial shop");
git(shop, "worktree", "add", "-q", "-b", "feat/checkout", join(root, "shop-checkout"));
writeFileSync(join(root, "shop-checkout", "apps", "web", "server.mjs"), `${server}// Checkout flow, work in progress.\n`);

const api = join(root, "api");
file(join(api, "package.json"), '{ "name": "api", "private": true, "type": "module" }\n');
file(join(api, "server.mjs"), server);
file(join(api, ".portpilot.toml"), '[project]\nname = "api"\n\n[ports.api]\npreferred = 8080\nrange = "8080-8099"\n\n[run.dev]\ncommand = "node server.mjs 0.0.0.0 {ports.api}"\n');
git(api, "init", "-q", "-b", "main");
git(api, "add", ".");
git(api, "commit", "-qm", "Initial api");

const docs = join(root, "docs");
file(join(docs, "package.json"), '{ "name": "docs", "private": true, "type": "module" }\n');
file(join(docs, "server.mjs"), server);
file(
  join(docs, "dev.mjs"),
  `// Starts the docs preview twice, the way a stray second \`pnpm dev\` would.
import { spawn } from "node:child_process";
import { fileURLToPath } from "node:url";
const server = fileURLToPath(new URL("./server.mjs", import.meta.url));
for (const host of ["127.0.0.1", "::1"]) spawn(process.execPath, [server, host, "3000"], { stdio: "inherit" });
`,
);
git(docs, "init", "-q", "-b", "main");
git(docs, "add", ".");
git(docs, "commit", "-qm", "Initial docs");

const detached = (cmd, argv, cwd) => spawn(cmd, argv, { cwd, env, detached: true, stdio: "ignore", windowsHide: true }).unref();
detached(process.execPath, [join(docs, "dev.mjs")], docs);
detached(portctl, ["run", "dev", "--yes"], shop);
detached(portctl, ["run", "dev", "--yes"], api);

console.log(`Demo projects in ${root}; servers on 3000 (twice), 5173 and 8080.`);
console.log(`Start the app on the demo data folder:`);
console.log(`  $env:PORTPILOT_DATA_DIR="${data}"; $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS="--remote-debugging-port=9333"; pnpm app:dev`);
console.log(`Then: node scripts/screenshots.mjs --demo ${root}`);
