//! `portpilot.db`: the one local database. It holds port leases, the run commands the user
//! trusted, the runs PortPilot started, and the history timeline. History entries are deleted
//! after 14 days; command lines are stored only when the user turned that on, and then masked.
//!
//! The app and `portctl` open the same file, so writes that must not race (leases) run in an
//! immediate transaction.

use std::path::Path;
use std::time::Duration;

use portpilot_core::lease::Lease;
use rusqlite::{Connection, OptionalExtension, Row, TransactionBehavior, params};
use serde::{Deserialize, Serialize};

/// History older than this is deleted.
pub const RETENTION_DAYS: i64 = 14;
const DAY_MS: i64 = 24 * 60 * 60 * 1000;
const SCHEMA: i32 = 1;

pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunPort {
    pub service: String,
    pub port: u16,
}

/// A run command the user trusted for a repository (or a folder outside Git).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrustEntry {
    pub project_key: String,
    pub run: String,
    /// SHA-256 of the run's command, folder and environment as the recipe writes them.
    pub digest: String,
    /// The recipe folder the user saw when they trusted it.
    pub folder: String,
    pub project: String,
    pub trusted_at: i64,
}

/// A run PortPilot started. Active while `ended_at` is empty and its owner (the app or
/// `portctl run`) is alive; when the owner dies, the job ends the run with it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunRecord {
    pub id: String,
    pub name: String,
    pub project: String,
    pub folder: String,
    pub pid: u32,
    pub start: u64,
    pub started_at: i64,
    pub ended_at: Option<i64>,
    pub exit_code: Option<i64>,
    pub owner_pid: u32,
    pub owner_start: u64,
    pub ports: Vec<RunPort>,
    /// Masked command line, only when the user asked for command lines in history.
    pub command: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EventKind {
    RunStarted,
    RunExited,
    PortOpened,
    PortClosed,
}

impl EventKind {
    pub fn as_str(self) -> &'static str {
        match self {
            EventKind::RunStarted => "runStarted",
            EventKind::RunExited => "runExited",
            EventKind::PortOpened => "portOpened",
            EventKind::PortClosed => "portClosed",
        }
    }

    pub fn parse(s: &str) -> Option<EventKind> {
        [
            EventKind::RunStarted,
            EventKind::RunExited,
            EventKind::PortOpened,
            EventKind::PortClosed,
        ]
        .into_iter()
        .find(|k| k.as_str() == s)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NewEvent {
    pub port: Option<u16>,
    pub run_id: Option<String>,
    pub run_name: Option<String>,
    pub pid: Option<u32>,
    pub process: Option<String>,
    pub project: Option<String>,
    pub folder: Option<String>,
    pub runtime: Option<String>,
    pub exit_code: Option<i64>,
    pub detail: Option<String>,
    pub command: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEvent {
    pub id: i64,
    /// Unix milliseconds.
    pub at: i64,
    pub kind: EventKind,
    #[serde(flatten)]
    pub data: NewEvent,
}

#[derive(Debug, Clone, Default)]
pub struct EventQuery {
    pub kinds: Vec<EventKind>,
    pub port: Option<u16>,
    pub since: Option<i64>,
    /// Only events older than this `(at, id)`: the next page.
    pub before: Option<(i64, i64)>,
    pub limit: usize,
}

/// Folders the user ran something in, newest first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecentFolder {
    pub folder: String,
    pub project: String,
    pub last_used: i64,
}

/// What the user chose about history in Settings (`settings.json`). `portctl` reads the same
/// file so both record the same way.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HistoryPrefs {
    pub enabled: bool,
    pub command_lines: bool,
}

impl Default for HistoryPrefs {
    fn default() -> Self {
        HistoryPrefs {
            enabled: true,
            command_lines: false,
        }
    }
}

impl HistoryPrefs {
    pub fn load(data_dir: &Path) -> HistoryPrefs {
        let defaults = HistoryPrefs::default();
        let Ok(text) = std::fs::read_to_string(data_dir.join("settings.json")) else {
            return defaults;
        };
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else {
            return defaults;
        };
        HistoryPrefs {
            enabled: v["history"].as_bool().unwrap_or(defaults.enabled),
            command_lines: v["historyCommandLines"]
                .as_bool()
                .unwrap_or(defaults.command_lines),
        }
    }
}

pub struct Store {
    conn: Connection,
}

fn sql_err(e: rusqlite::Error) -> String {
    format!("history database: {e}")
}

fn lease_from(row: &Row) -> rusqlite::Result<Lease> {
    Ok(Lease {
        owner_key: row.get("owner_key")?,
        service: row.get("service")?,
        port: row.get::<_, i64>("port")? as u16,
        worktree: row.get("worktree")?,
        common_dir: row.get("common_dir")?,
        project: row.get("project")?,
        acquired_at: row.get("acquired_at")?,
    })
}

fn trust_from(row: &Row) -> rusqlite::Result<TrustEntry> {
    Ok(TrustEntry {
        project_key: row.get("project_key")?,
        run: row.get("run")?,
        digest: row.get("digest")?,
        folder: row.get("folder")?,
        project: row.get("project")?,
        trusted_at: row.get("trusted_at")?,
    })
}

fn run_from(row: &Row) -> rusqlite::Result<RunRecord> {
    let ports: String = row.get("ports")?;
    Ok(RunRecord {
        id: row.get("id")?,
        name: row.get("name")?,
        project: row.get("project")?,
        folder: row.get("folder")?,
        pid: row.get::<_, i64>("pid")? as u32,
        start: row.get::<_, i64>("start")? as u64,
        started_at: row.get("started_at")?,
        ended_at: row.get("ended_at")?,
        exit_code: row.get("exit_code")?,
        owner_pid: row.get::<_, i64>("owner_pid")? as u32,
        owner_start: row.get::<_, i64>("owner_start")? as u64,
        ports: serde_json::from_str(&ports).unwrap_or_default(),
        command: row.get("command")?,
    })
}

fn event_from(row: &Row) -> rusqlite::Result<HistoryEvent> {
    let kind: String = row.get("kind")?;
    Ok(HistoryEvent {
        id: row.get("id")?,
        at: row.get("at")?,
        kind: EventKind::parse(&kind).unwrap_or(EventKind::PortOpened),
        data: NewEvent {
            port: row.get::<_, Option<i64>>("port")?.map(|p| p as u16),
            run_id: row.get("run_id")?,
            run_name: row.get("run_name")?,
            pid: row.get::<_, Option<i64>>("pid")?.map(|p| p as u32),
            process: row.get("process")?,
            project: row.get("project")?,
            folder: row.get("folder")?,
            runtime: row.get("runtime")?,
            exit_code: row.get("exit_code")?,
            detail: row.get("detail")?,
            command: row.get("command")?,
        },
    })
}

impl Store {
    /// Opens (or creates) the database and deletes history past the retention period.
    pub fn open(path: &Path) -> Result<Store, String> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
        let conn = Connection::open(path).map_err(sql_err)?;
        conn.busy_timeout(Duration::from_secs(5)).map_err(sql_err)?;
        conn.pragma_update(None, "journal_mode", "WAL")
            .map_err(sql_err)?;
        conn.pragma_update(None, "foreign_keys", "ON")
            .map_err(sql_err)?;
        let store = Store { conn };
        store.migrate()?;
        store.prune(now_ms())?;
        Ok(store)
    }

    /// Opens the database only if it exists, so read-only commands never create it.
    pub fn open_existing(path: &Path) -> Option<Store> {
        path.is_file().then(|| Store::open(path).ok()).flatten()
    }

    /// An in-memory database for tests.
    pub fn in_memory() -> Result<Store, String> {
        let store = Store {
            conn: Connection::open_in_memory().map_err(sql_err)?,
        };
        store.migrate()?;
        Ok(store)
    }

    fn migrate(&self) -> Result<(), String> {
        let version: i32 = self
            .conn
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .map_err(sql_err)?;
        if version > SCHEMA {
            return Err(format!(
                "history database: written by a newer PortPilot (schema {version})"
            ));
        }
        if version < 1 {
            self.conn
                .execute_batch(
                    "BEGIN;
                    CREATE TABLE leases (
                        owner_key TEXT NOT NULL,
                        service TEXT NOT NULL,
                        port INTEGER NOT NULL UNIQUE,
                        worktree TEXT NOT NULL,
                        common_dir TEXT,
                        project TEXT NOT NULL,
                        acquired_at INTEGER NOT NULL,
                        PRIMARY KEY (owner_key, service)
                    );
                    CREATE TABLE trust (
                        project_key TEXT NOT NULL,
                        run TEXT NOT NULL,
                        digest TEXT NOT NULL,
                        folder TEXT NOT NULL,
                        project TEXT NOT NULL,
                        trusted_at INTEGER NOT NULL,
                        PRIMARY KEY (project_key, run)
                    );
                    CREATE TABLE runs (
                        id TEXT PRIMARY KEY,
                        name TEXT NOT NULL,
                        project TEXT NOT NULL,
                        folder TEXT NOT NULL,
                        pid INTEGER NOT NULL,
                        start INTEGER NOT NULL,
                        started_at INTEGER NOT NULL,
                        ended_at INTEGER,
                        exit_code INTEGER,
                        owner_pid INTEGER NOT NULL,
                        owner_start INTEGER NOT NULL,
                        ports TEXT NOT NULL,
                        command TEXT
                    );
                    CREATE INDEX runs_active ON runs(ended_at);
                    CREATE TABLE events (
                        id INTEGER PRIMARY KEY AUTOINCREMENT,
                        at INTEGER NOT NULL,
                        kind TEXT NOT NULL,
                        port INTEGER,
                        run_id TEXT,
                        run_name TEXT,
                        pid INTEGER,
                        process TEXT,
                        project TEXT,
                        folder TEXT,
                        runtime TEXT,
                        exit_code INTEGER,
                        detail TEXT,
                        command TEXT
                    );
                    CREATE INDEX events_at ON events(at);
                    PRAGMA user_version = 1;
                    COMMIT;",
                )
                .map_err(sql_err)?;
        }
        Ok(())
    }

    /// Deletes history entries and finished runs older than the retention period.
    pub fn prune(&self, now: i64) -> Result<(), String> {
        let cutoff = now - RETENTION_DAYS * DAY_MS;
        self.conn
            .execute("DELETE FROM events WHERE at < ?1", params![cutoff])
            .map_err(sql_err)?;
        self.conn
            .execute(
                "DELETE FROM runs WHERE ended_at IS NOT NULL AND ended_at < ?1",
                params![cutoff],
            )
            .map_err(sql_err)?;
        Ok(())
    }

    // Leases.

    pub fn leases(&self) -> Result<Vec<Lease>, String> {
        let mut stmt = self
            .conn
            .prepare("SELECT * FROM leases ORDER BY project, worktree, service")
            .map_err(sql_err)?;
        stmt.query_map([], lease_from)
            .map_err(sql_err)?
            .collect::<Result<_, _>>()
            .map_err(sql_err)
    }

    /// Runs `f` with every current lease inside an immediate transaction and stores the lease
    /// it returns. Two processes allocating at once are serialized here.
    pub fn lease_tx(
        &mut self,
        f: impl FnOnce(&[Lease]) -> Result<Lease, String>,
    ) -> Result<Lease, String> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sql_err)?;
        let all: Vec<Lease> = {
            let mut stmt = tx.prepare("SELECT * FROM leases").map_err(sql_err)?;
            stmt.query_map([], lease_from)
                .map_err(sql_err)?
                .collect::<Result<_, _>>()
                .map_err(sql_err)?
        };
        let lease = f(&all)?;
        tx.execute(
            "INSERT INTO leases (owner_key, service, port, worktree, common_dir, project, acquired_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT (owner_key, service) DO UPDATE SET
                port = excluded.port, worktree = excluded.worktree,
                common_dir = excluded.common_dir, project = excluded.project,
                acquired_at = excluded.acquired_at",
            params![
                lease.owner_key,
                lease.service,
                i64::from(lease.port),
                lease.worktree,
                lease.common_dir,
                lease.project,
                lease.acquired_at
            ],
        )
        .map_err(sql_err)?;
        tx.commit().map_err(sql_err)?;
        Ok(lease)
    }

    /// Releases one service's lease, or every lease of the owner when `service` is `None`.
    pub fn release_leases(&self, owner_key: &str, service: Option<&str>) -> Result<usize, String> {
        match service {
            Some(s) => self.conn.execute(
                "DELETE FROM leases WHERE owner_key = ?1 AND service = ?2",
                params![owner_key, s],
            ),
            None => self.conn.execute(
                "DELETE FROM leases WHERE owner_key = ?1",
                params![owner_key],
            ),
        }
        .map_err(sql_err)
    }

    // Trusted run commands.

    pub fn trust(&self, project_key: &str, run: &str) -> Result<Option<TrustEntry>, String> {
        self.conn
            .query_row(
                "SELECT * FROM trust WHERE project_key = ?1 AND run = ?2",
                params![project_key, run],
                trust_from,
            )
            .optional()
            .map_err(sql_err)
    }

    pub fn set_trust(&self, entry: &TrustEntry) -> Result<(), String> {
        self.conn
            .execute(
                "INSERT OR REPLACE INTO trust (project_key, run, digest, folder, project, trusted_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    entry.project_key,
                    entry.run,
                    entry.digest,
                    entry.folder,
                    entry.project,
                    entry.trusted_at
                ],
            )
            .map(drop)
            .map_err(sql_err)
    }

    pub fn trusted(&self) -> Result<Vec<TrustEntry>, String> {
        let mut stmt = self
            .conn
            .prepare("SELECT * FROM trust ORDER BY trusted_at DESC")
            .map_err(sql_err)?;
        stmt.query_map([], trust_from)
            .map_err(sql_err)?
            .collect::<Result<_, _>>()
            .map_err(sql_err)
    }

    pub fn revoke_trust(&self, project_key: &str, run: &str) -> Result<bool, String> {
        self.conn
            .execute(
                "DELETE FROM trust WHERE project_key = ?1 AND run = ?2",
                params![project_key, run],
            )
            .map(|n| n > 0)
            .map_err(sql_err)
    }

    // Runs.

    pub fn insert_run(&self, run: &RunRecord) -> Result<(), String> {
        self.conn
            .execute(
                "INSERT INTO runs (id, name, project, folder, pid, start, started_at, ended_at,
                    exit_code, owner_pid, owner_start, ports, command)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
                params![
                    run.id,
                    run.name,
                    run.project,
                    run.folder,
                    i64::from(run.pid),
                    run.start as i64,
                    run.started_at,
                    run.ended_at,
                    run.exit_code,
                    i64::from(run.owner_pid),
                    run.owner_start as i64,
                    serde_json::to_string(&run.ports).unwrap_or_else(|_| "[]".into()),
                    run.command
                ],
            )
            .map(drop)
            .map_err(sql_err)
    }

    pub fn run(&self, id: &str) -> Result<Option<RunRecord>, String> {
        self.conn
            .query_row("SELECT * FROM runs WHERE id = ?1", params![id], run_from)
            .optional()
            .map_err(sql_err)
    }

    /// Marks a run as ended. Returns false when something else already did, so the exit is
    /// recorded once.
    pub fn finish_run(
        &self,
        id: &str,
        ended_at: i64,
        exit_code: Option<i64>,
    ) -> Result<bool, String> {
        self.conn
            .execute(
                "UPDATE runs SET ended_at = ?2, exit_code = ?3 WHERE id = ?1 AND ended_at IS NULL",
                params![id, ended_at, exit_code],
            )
            .map(|n| n > 0)
            .map_err(sql_err)
    }

    /// Removes a run record, for users who turned history off.
    pub fn delete_run(&self, id: &str) -> Result<(), String> {
        self.conn
            .execute("DELETE FROM runs WHERE id = ?1", params![id])
            .map(drop)
            .map_err(sql_err)
    }

    pub fn unfinished_runs(&self) -> Result<Vec<RunRecord>, String> {
        let mut stmt = self
            .conn
            .prepare("SELECT * FROM runs WHERE ended_at IS NULL ORDER BY started_at")
            .map_err(sql_err)?;
        stmt.query_map([], run_from)
            .map_err(sql_err)?
            .collect::<Result<_, _>>()
            .map_err(sql_err)
    }

    /// Ends runs whose owner is gone: closing the owner's job ended them. `alive` checks a
    /// process identity. Returns the runs that were ended now.
    pub fn sweep_lost_runs(
        &self,
        alive: impl Fn(u32, u64) -> bool,
        record_events: bool,
    ) -> Result<Vec<RunRecord>, String> {
        let now = now_ms();
        let mut lost = Vec::new();
        for run in self.unfinished_runs()? {
            if alive(run.owner_pid, run.owner_start) {
                continue;
            }
            if self.finish_run(&run.id, now, None)? {
                if record_events {
                    self.record(
                        EventKind::RunExited,
                        now,
                        &NewEvent {
                            run_id: Some(run.id.clone()),
                            run_name: Some(run.name.clone()),
                            pid: Some(run.pid),
                            project: Some(run.project.clone()),
                            folder: Some(run.folder.clone()),
                            detail: Some("ownerExited".into()),
                            ..NewEvent::default()
                        },
                    )?;
                } else {
                    self.delete_run(&run.id)?;
                }
                lost.push(run);
            }
        }
        Ok(lost)
    }

    pub fn recent_folders(&self, limit: usize) -> Result<Vec<RecentFolder>, String> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT folder, project, MAX(t) AS last_used FROM (
                    SELECT folder, project, started_at AS t FROM runs
                    UNION ALL SELECT folder, project, trusted_at AS t FROM trust
                 ) GROUP BY lower(folder) ORDER BY last_used DESC LIMIT ?1",
            )
            .map_err(sql_err)?;
        stmt.query_map(params![limit as i64], |r| {
            Ok(RecentFolder {
                folder: r.get(0)?,
                project: r.get(1)?,
                last_used: r.get(2)?,
            })
        })
        .map_err(sql_err)?
        .collect::<Result<_, _>>()
        .map_err(sql_err)
    }

    // History events.

    pub fn record(&self, kind: EventKind, at: i64, e: &NewEvent) -> Result<(), String> {
        self.conn
            .execute(
                "INSERT INTO events (at, kind, port, run_id, run_name, pid, process, project,
                    folder, runtime, exit_code, detail, command)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
                params![
                    at,
                    kind.as_str(),
                    e.port.map(i64::from),
                    e.run_id,
                    e.run_name,
                    e.pid.map(i64::from),
                    e.process,
                    e.project,
                    e.folder,
                    e.runtime,
                    e.exit_code,
                    e.detail,
                    e.command
                ],
            )
            .map(drop)
            .map_err(sql_err)
    }

    pub fn events(&self, q: &EventQuery) -> Result<Vec<HistoryEvent>, String> {
        let mut sql = String::from("SELECT * FROM events WHERE 1 = 1");
        let mut args: Vec<rusqlite::types::Value> = Vec::new();
        if !q.kinds.is_empty() {
            let marks: Vec<String> = q
                .kinds
                .iter()
                .map(|k| {
                    args.push(k.as_str().to_string().into());
                    format!("?{}", args.len())
                })
                .collect();
            sql.push_str(&format!(" AND kind IN ({})", marks.join(", ")));
        }
        if let Some(p) = q.port {
            args.push(i64::from(p).into());
            sql.push_str(&format!(" AND port = ?{}", args.len()));
        }
        if let Some(s) = q.since {
            args.push(s.into());
            sql.push_str(&format!(" AND at >= ?{}", args.len()));
        }
        if let Some((at, id)) = q.before {
            args.push(at.into());
            let a = args.len();
            args.push(id.into());
            sql.push_str(&format!(
                " AND (at < ?{a} OR (at = ?{a} AND id < ?{}))",
                args.len()
            ));
        }
        args.push((q.limit.clamp(1, 5000) as i64).into());
        sql.push_str(&format!(" ORDER BY at DESC, id DESC LIMIT ?{}", args.len()));
        let mut stmt = self.conn.prepare(&sql).map_err(sql_err)?;
        stmt.query_map(rusqlite::params_from_iter(args), event_from)
            .map_err(sql_err)?
            .collect::<Result<_, _>>()
            .map_err(sql_err)
    }

    /// Deletes the timeline and finished runs. Leases, trusted commands and active runs stay.
    pub fn clear_history(&self) -> Result<(), String> {
        self.conn
            .execute_batch("DELETE FROM events; DELETE FROM runs WHERE ended_at IS NOT NULL;")
            .map_err(sql_err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lease(owner: &str, service: &str, port: u16) -> Lease {
        Lease {
            owner_key: owner.into(),
            service: service.into(),
            port,
            worktree: owner.into(),
            common_dir: None,
            project: "shop".into(),
            acquired_at: 1,
        }
    }

    fn run(id: &str, owner_pid: u32) -> RunRecord {
        RunRecord {
            id: id.into(),
            name: "dev".into(),
            project: "shop".into(),
            folder: r"C:\src\shop".into(),
            pid: 100,
            start: 132_000_000_000_000_000,
            started_at: now_ms(),
            ended_at: None,
            exit_code: None,
            owner_pid,
            owner_start: 5,
            ports: vec![RunPort {
                service: "web".into(),
                port: 5173,
            }],
            command: None,
        }
    }

    #[test]
    fn leases_are_unique_per_port_and_owner() {
        let mut s = Store::in_memory().unwrap();
        s.lease_tx(|_| Ok(lease("a", "web", 5173))).unwrap();
        s.lease_tx(|all| {
            assert_eq!(all.len(), 1);
            Ok(lease("a", "api", 8080))
        })
        .unwrap();
        // The same owner and service moves to a new port.
        s.lease_tx(|_| Ok(lease("a", "web", 5174))).unwrap();
        let all = s.leases().unwrap();
        assert_eq!(all.len(), 2);
        assert!(all.iter().any(|l| l.service == "web" && l.port == 5174));
        // Another owner cannot take a leased port.
        assert!(s.lease_tx(|_| Ok(lease("b", "web", 5174))).is_err());
        // A failing allocation stores nothing.
        assert!(s.lease_tx(|_| Err("full".into())).is_err());
        assert_eq!(s.leases().unwrap().len(), 2);
        assert_eq!(s.release_leases("a", Some("web")).unwrap(), 1);
        assert_eq!(s.release_leases("a", None).unwrap(), 1);
        assert!(s.leases().unwrap().is_empty());
    }

    #[test]
    fn trust_roundtrip() {
        let s = Store::in_memory().unwrap();
        assert_eq!(s.trust("k", "dev").unwrap(), None);
        let entry = TrustEntry {
            project_key: "k".into(),
            run: "dev".into(),
            digest: "abc".into(),
            folder: r"C:\src\shop".into(),
            project: "shop".into(),
            trusted_at: 7,
        };
        s.set_trust(&entry).unwrap();
        assert_eq!(s.trust("k", "dev").unwrap(), Some(entry.clone()));
        assert_eq!(s.trusted().unwrap(), vec![entry]);
        assert!(s.revoke_trust("k", "dev").unwrap());
        assert!(!s.revoke_trust("k", "dev").unwrap());
    }

    #[test]
    fn runs_end_once_and_lost_runs_are_swept() {
        let s = Store::in_memory().unwrap();
        s.insert_run(&run("r1", 11)).unwrap();
        s.insert_run(&run("r2", 22)).unwrap();
        assert_eq!(s.unfinished_runs().unwrap().len(), 2);
        assert_eq!(s.run("r1").unwrap().unwrap().ports[0].port, 5173);
        assert!(s.finish_run("r1", now_ms(), Some(0)).unwrap());
        assert!(!s.finish_run("r1", now_ms(), Some(1)).unwrap());
        assert_eq!(s.run("r1").unwrap().unwrap().exit_code, Some(0));
        // r2's owner is gone.
        let lost = s.sweep_lost_runs(|pid, _| pid != 22, true).unwrap();
        assert_eq!(lost.len(), 1);
        assert!(s.unfinished_runs().unwrap().is_empty());
        let events = s
            .events(&EventQuery {
                limit: 10,
                ..EventQuery::default()
            })
            .unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, EventKind::RunExited);
        assert_eq!(events[0].data.detail.as_deref(), Some("ownerExited"));
        assert_eq!(s.recent_folders(5).unwrap().len(), 1);
    }

    #[test]
    fn events_filter_page_and_expire() {
        let s = Store::in_memory().unwrap();
        let now = now_ms();
        for (i, kind) in [
            EventKind::PortOpened,
            EventKind::PortClosed,
            EventKind::RunStarted,
        ]
        .into_iter()
        .enumerate()
        {
            s.record(
                kind,
                now + i as i64,
                &NewEvent {
                    port: Some(3000 + i as u16),
                    ..NewEvent::default()
                },
            )
            .unwrap();
        }
        s.record(
            EventKind::PortOpened,
            now - 15 * DAY_MS,
            &NewEvent::default(),
        )
        .unwrap();
        let all = s
            .events(&EventQuery {
                limit: 100,
                ..EventQuery::default()
            })
            .unwrap();
        assert_eq!(all.len(), 4);
        assert_eq!(all[0].kind, EventKind::RunStarted, "newest first");
        let ports = s
            .events(&EventQuery {
                kinds: vec![EventKind::PortOpened, EventKind::PortClosed],
                since: Some(now - DAY_MS),
                limit: 100,
                ..EventQuery::default()
            })
            .unwrap();
        assert_eq!(ports.len(), 2);
        let one = s
            .events(&EventQuery {
                port: Some(3001),
                limit: 100,
                ..EventQuery::default()
            })
            .unwrap();
        assert_eq!(one.len(), 1);
        let page = s
            .events(&EventQuery {
                before: Some((all[1].at, all[1].id)),
                limit: 100,
                ..EventQuery::default()
            })
            .unwrap();
        assert_eq!(page.len(), 2);
        s.prune(now).unwrap();
        assert_eq!(
            s.events(&EventQuery {
                limit: 100,
                ..EventQuery::default()
            })
            .unwrap()
            .len(),
            3
        );
        s.clear_history().unwrap();
        assert!(
            s.events(&EventQuery {
                limit: 100,
                ..EventQuery::default()
            })
            .unwrap()
            .is_empty()
        );
    }

    #[test]
    fn file_database_survives_reopen() {
        let dir = std::env::temp_dir().join(format!("pp-store-{}", std::process::id()));
        let path = dir.join("portpilot.db");
        let _ = std::fs::remove_dir_all(&dir);
        assert!(Store::open_existing(&path).is_none());
        {
            let mut s = Store::open(&path).unwrap();
            s.lease_tx(|_| Ok(lease("a", "web", 5173))).unwrap();
        }
        let s = Store::open_existing(&path).expect("exists now");
        assert_eq!(s.leases().unwrap().len(), 1);
        drop(s);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn prefs_come_from_settings_json() {
        let dir = std::env::temp_dir().join(format!("pp-prefs-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(HistoryPrefs::load(&dir), HistoryPrefs::default());
        std::fs::write(
            dir.join("settings.json"),
            r#"{"history": false, "historyCommandLines": true}"#,
        )
        .unwrap();
        assert_eq!(
            HistoryPrefs::load(&dir),
            HistoryPrefs {
                enabled: false,
                command_lines: true
            }
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
