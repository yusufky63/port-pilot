// Mirrors crates/core/src/model.rs and the engine's diagnostic types (camelCase on the wire).

export type Protocol = "tcp" | "udp";
export type Exposure = "loopback" | "lan" | "all";
export type Runtime = "windows" | "wsl" | "docker";
export type Confidence = "exact" | "inferred";
export type AttributionSource = "procCwd" | "composeLabel" | "commandLine" | "executablePath" | "launched";
export type ProcessClass = "user" | "service" | "system" | "relay";
export type LauncherKind = "claude" | "codex" | "gemini" | "vsCode" | "cursor" | "windsurf" | "jetBrains" | "zed" | "terminal" | "explorer" | "service";
export type StopBlock = "system" | "service" | "relay" | "accessDenied" | "identityUnknown" | "selfProcess";
export type Note = "relayWithoutListener" | "relayWithoutContainer" | "notForwarded" | "parentExited" | "sharedPort";
export type AdapterStatus = "available" | "disabled" | "notInstalled" | "notRunning" | "error";
export type StopOutcome =
  | "stopped"
  | "forced"
  | "alreadyExited"
  | "identityChanged"
  | "gracefulTimeout"
  | "gracefulUnavailable"
  | "accessDenied"
  | "refused"
  | "runtimeUnavailable"
  | "failed";
export type GracefulChannel = "closeWindow" | "consoleCtrlC" | "consoleCtrlBreak" | "groupCtrlBreak" | "sigterm" | "dockerStop";
export type GracefulSkip = "noConsole" | "sharedConsole" | "signalFailed";

export interface Endpoint {
  address: string;
  port: number;
  exposure: Exposure;
}

export interface ProcessInfo {
  pid: number;
  /** FILETIME on Windows, clock ticks in WSL; serialized as a number. */
  start: number | null;
  startedAt: string | null;
  name: string;
  path: string | null;
  commandLine: string | null;
  user: string | null;
  class: ProcessClass;
  accessDenied: boolean;
}

export interface ProcessRef {
  pid: number;
  name: string;
  start: number | null;
  startedAt: string | null;
  commandLine: string | null;
}

export interface Launcher {
  kind: LauncherKind;
  pid: number;
  name: string;
}

export interface GitInfo {
  topLevel: string;
  repoName: string;
  commonDir: string;
  branch: string | null;
  head: string | null;
  dirty: boolean | null;
  linkedWorktree: boolean;
}

export interface ProjectInfo {
  root: string;
  name: string;
  confidence: Confidence;
  source: AttributionSource;
  manifests: string[];
  git: GitInfo | null;
  openPath: string | null;
}

export interface ContainerPort {
  protocol: Protocol;
  hostIp: string | null;
  hostPort: number | null;
  containerPort: number;
}

export interface ContainerInfo {
  id: string;
  name: string;
  image: string;
  state: string;
  status: string;
  composeProject: string | null;
  composeService: string | null;
  composeWorkingDir: string | null;
  ports: ContainerPort[];
}

export interface WslInfo {
  distro: string;
  cwd: string | null;
  binds: Endpoint[];
}

export type StopTarget =
  | { kind: "windows"; pid: number; start: number }
  | { kind: "wsl"; distro: string; pid: number; start: number }
  | { kind: "docker"; containerId: string };

export interface StopInfo {
  target: StopTarget | null;
  blocked: StopBlock | null;
}

export interface ManagedRef {
  runId: string;
  name: string;
}

export interface Listener {
  id: string;
  protocol: Protocol;
  port: number;
  endpoints: Endpoint[];
  exposure: Exposure;
  runtime: Runtime;
  process: ProcessInfo | null;
  ancestors: ProcessRef[];
  launcher: Launcher | null;
  relay: ProcessRef | null;
  container: ContainerInfo | null;
  wsl: WslInfo | null;
  project: ProjectInfo | null;
  url: string | null;
  stop: StopInfo;
  conflict: boolean;
  notes: Note[];
  /** Set when PortPilot started the owner as part of a managed run. */
  managed: ManagedRef | null;
}

export interface Conflict {
  protocol: Protocol;
  port: number;
  listenerIds: string[];
}

export interface AdapterState {
  status: AdapterStatus;
  detail: string | null;
  items: string[];
}

export interface Snapshot {
  schemaVersion: number;
  takenAt: string;
  durationMs: number;
  listeners: Listener[];
  conflicts: Conflict[];
  adapters: { wsl: AdapterState; docker: AdapterState };
  warnings: string[];
}

export interface StopReport {
  outcome: StopOutcome;
  channel: GracefulChannel | null;
  gracefulSkipped: GracefulSkip | null;
  detail: string | null;
  childrenTerminated: number;
  elapsedMs: number;
}

export interface PortRange {
  start: number;
  end: number;
}

export type Verdict = "free" | "inUse" | "shared" | "reserved" | "blocked";

export interface PortDiagnosis {
  port: number;
  verdict: Verdict;
  listeners: Listener[];
  excludedRange: PortRange | null;
  suggestions: number[];
}

export interface Environment {
  os: string | null;
  windows10: boolean;
  elevated: boolean;
  user: string | null;
  git: string | null;
  wsl: AdapterState;
  wslVersion: string | null;
  docker: AdapterState;
  dockerVersion: string | null;
  excludedTcp: PortRange[];
}

export interface WorktreeView {
  path: string;
  branch: string | null;
  head: string | null;
  detached: boolean;
  locked: boolean;
  prunable: boolean;
  main: boolean;
  dirty: boolean | null;
  listenerIds: string[];
}

export interface ProjectGroup {
  key: string;
  name: string;
  runtime: Runtime;
  commonDir: string | null;
  root: string;
  openPath: string | null;
  worktrees: WorktreeView[];
  listenerIds: string[];
}

export interface Settings {
  theme: "system" | "light" | "dark";
  language: "system" | "en" | "tr";
  refreshSeconds: number;
  includeWsl: boolean;
  includeDocker: boolean;
  maskSecrets: boolean;
  graceSeconds: number;
  closeToTray: boolean;
  history: boolean;
  historyCommandLines: boolean;
}

export interface AppInfo {
  version: string;
  portable: boolean;
  dataDir: string;
  elevated: boolean;
  user: string | null;
  hasVscode: boolean;
  settingsError: string | null;
  storeError: string | null;
  paused: boolean;
}

// Managed runs, leases and history (crates/engine/src/runs.rs, leases.rs, store.rs and
// src-tauri/src/runs.rs).

export interface RunPort {
  service: string;
  port: number;
}

export type RunState = "running" | "stopping" | "exited";

export interface RunInfo {
  id: string;
  name: string;
  project: string;
  folder: string;
  command: string;
  ports: RunPort[];
  pid: number;
  start: number;
  startedAt: string;
  state: RunState;
  exitCode: number | null;
  endedAt: string | null;
  endedBy: "stopped" | "terminated" | null;
  processes: number;
  lines: number;
}

export interface OutputLine {
  seq: number;
  stream: "stdout" | "stderr";
  text: string;
}

export interface OutputChunk {
  lines: OutputLine[];
  next: number;
  truncated: boolean;
}

export interface EnvVar {
  key: string;
  value: string;
}

export interface RecipeRunView {
  name: string;
  command: string;
  cwd: string | null;
  env: EnvVar[];
  services: string[];
  trusted: boolean;
  trustedAt: number | null;
}

export interface RecipePortView {
  service: string;
  preferred: number;
  rangeStart: number;
  rangeEnd: number;
  leased: number | null;
}

export interface RecipeView {
  folder: string;
  project: string;
  worktree: string;
  commonDir: string | null;
  branch: string | null;
  linkedWorktree: boolean;
  runs: RecipeRunView[];
  ports: RecipePortView[];
}

export interface RecipeResult {
  folder: string;
  recipe: RecipeView | null;
  error: string | null;
}

export interface LeaseUser {
  listenerId: string;
  pid: number | null;
  name: string;
  project: string | null;
  sameWorktree: boolean;
}

export interface PlanPort {
  service: string;
  port: number | null;
  leased: boolean;
  rangeStart: number;
  rangeEnd: number;
  users: LeaseUser[];
}

export interface RunPlan {
  run: string;
  project: string;
  recipeFolder: string;
  folder: string;
  worktree: string;
  branch: string | null;
  command: string;
  env: EnvVar[];
  ports: PlanPort[];
  digest: string;
  trusted: boolean;
  trustedAt: number | null;
  blocked: string | null;
}

export interface LeaseView {
  ownerKey: string;
  service: string;
  port: number;
  worktree: string;
  commonDir: string | null;
  project: string;
  acquiredAt: number;
  users: LeaseUser[];
  stale: boolean;
}

export type EventKind = "runStarted" | "runExited" | "portOpened" | "portClosed";

export interface HistoryEvent {
  id: number;
  /** Unix milliseconds. */
  at: number;
  kind: EventKind;
  port: number | null;
  runId: string | null;
  runName: string | null;
  pid: number | null;
  process: string | null;
  project: string | null;
  folder: string | null;
  runtime: string | null;
  exitCode: number | null;
  detail: string | null;
  command: string | null;
}

export interface TrustEntry {
  projectKey: string;
  run: string;
  digest: string;
  folder: string;
  project: string;
  trustedAt: number;
}

export interface RecentFolder {
  folder: string;
  project: string;
  lastUsed: number;
}
