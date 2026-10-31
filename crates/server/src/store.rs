//! SQLite persistence: readings for the history chart, the activity log, settings and runtime.

use std::path::Path;
use std::sync::Mutex;

use anyhow::Context;
use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;
use serde::de::DeserializeOwned;
use ts_rs::TS;

use crate::models::{ActivityKind, Decision, Runtime, Settings, Snapshot};

const SCHEMA: &str = "
create table if not exists readings (
  ts real not null,
  zone integer not null,          -- -1 is the AC itself
  on_ integer not null,
  temperature real,
  target real,
  damper real,
  fan text,
  setpoint real
);
create index if not exists readings_ts on readings (ts);
create table if not exists activity (
  id integer primary key autoincrement,
  ts real not null,
  kind text not null,             -- decision | info | external | error
  title text not null,
  reason text not null default '',
  dry_run integer not null default 0
);
create index if not exists activity_ts on activity (ts);
create table if not exists kv (
  key text primary key,
  value text not null
);
";

pub const AC_ROW: i64 = -1;

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct ActivityItem {
    #[ts(type = "number")]
    pub id: i64,
    pub ts: f64,
    pub kind: ActivityKind,
    pub title: String,
    pub reason: String,
    pub dry_run: bool,
}

pub struct Reading {
    pub ts: f64,
    pub zone: i64,
    pub on: bool,
    pub temperature: Option<f64>,
    pub target: Option<f64>,
    pub damper: Option<f64>,
    pub fan: Option<String>,
    pub setpoint: Option<f64>,
}

pub struct Store {
    db: Mutex<Connection>,
}

fn kind_str(kind: ActivityKind) -> &'static str {
    match kind {
        ActivityKind::Decision => "decision",
        ActivityKind::Info => "info",
        ActivityKind::External => "external",
        ActivityKind::Error => "error",
    }
}

fn kind_from(s: &str) -> ActivityKind {
    match s {
        "decision" => ActivityKind::Decision,
        "external" => ActivityKind::External,
        "error" => ActivityKind::Error,
        _ => ActivityKind::Info,
    }
}

impl Store {
    pub fn open(path: &Path) -> anyhow::Result<Self> {
        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        }
        Self::init(Connection::open(path).with_context(|| format!("opening {}", path.display()))?)
    }

    pub fn memory() -> anyhow::Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> anyhow::Result<Self> {
        conn.pragma_update(None, "journal_mode", "wal")?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self {
            db: Mutex::new(conn),
        })
    }

    fn db(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.db.lock().unwrap_or_else(|e| e.into_inner())
    }

    // readings

    pub fn record(&self, ts: f64, snap: &Snapshot, decision: &Decision) -> anyhow::Result<()> {
        let mut db = self.db();
        let tx = db.transaction()?;
        {
            let mut insert = tx.prepare_cached("insert into readings values (?,?,?,?,?,?,?,?)")?;
            let ac = &snap.ac;
            let fan = serde_json::to_value(ac.fan)?;
            insert.execute(params![
                ts,
                AC_ROW,
                ac.on,
                ac.temperature,
                None::<f64>,
                None::<f64>,
                fan.as_str(),
                ac.setpoint
            ])?;
            for z in &snap.zones {
                let target = decision
                    .zones
                    .iter()
                    .find(|e| e.number == z.number)
                    .and_then(|e| e.target);
                insert.execute(params![
                    ts,
                    i64::from(z.number),
                    z.on,
                    z.temperature,
                    target,
                    z.damper,
                    None::<String>,
                    None::<f64>
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn readings(&self, since: f64, until: f64) -> anyhow::Result<Vec<Reading>> {
        let db = self.db();
        let mut stmt = db.prepare_cached(
            "select ts, zone, on_, temperature, target, damper, fan, setpoint
             from readings where ts >= ? and ts <= ? order by ts",
        )?;
        let rows = stmt.query_map(params![since, until], |r| {
            Ok(Reading {
                ts: r.get(0)?,
                zone: r.get(1)?,
                on: r.get(2)?,
                temperature: r.get(3)?,
                target: r.get(4)?,
                damper: r.get(5)?,
                fan: r.get(6)?,
                setpoint: r.get(7)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn prune(&self, before: f64) -> anyhow::Result<()> {
        let db = self.db();
        db.execute("delete from readings where ts < ?", [before])?;
        db.execute("delete from activity where ts < ?", [before])?;
        Ok(())
    }

    // activity

    pub fn log(&self, ts: f64, kind: ActivityKind, title: &str, reason: &str, dry_run: bool) {
        let result = self.db().execute(
            "insert into activity (ts, kind, title, reason, dry_run) values (?,?,?,?,?)",
            params![ts, kind_str(kind), title, reason, dry_run],
        );
        if let Err(e) = result {
            tracing::error!("writing activity failed: {e}");
        }
    }

    pub fn activity(
        &self,
        limit: u32,
        before: Option<i64>,
        kind: Option<ActivityKind>,
    ) -> anyhow::Result<Vec<ActivityItem>> {
        let db = self.db();
        let mut stmt = db.prepare_cached(
            "select id, ts, kind, title, reason, dry_run from activity
             where (?1 is null or id < ?1) and (?2 is null or kind = ?2)
             order by id desc limit ?3",
        )?;
        let rows = stmt.query_map(params![before, kind.map(kind_str), limit], |r| {
            Ok(ActivityItem {
                id: r.get(0)?,
                ts: r.get(1)?,
                kind: kind_from(&r.get::<_, String>(2)?),
                title: r.get(3)?,
                reason: r.get(4)?,
                dry_run: r.get(5)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn activity_counts(&self) -> anyhow::Result<Vec<(String, u32)>> {
        let db = self.db();
        let mut stmt = db.prepare_cached("select kind, count(*) from activity group by kind")?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    // settings + runtime

    fn get<T: DeserializeOwned>(&self, key: &str) -> Option<T> {
        let raw: Option<String> = self
            .db()
            .query_row("select value from kv where key = ?", [key], |r| r.get(0))
            .optional()
            .unwrap_or_else(|e| {
                tracing::error!("reading {key} failed: {e}");
                None
            });
        raw.and_then(|v| {
            serde_json::from_str(&v)
                .inspect_err(|e| tracing::warn!("ignoring unreadable {key}: {e}"))
                .ok()
        })
    }

    fn put<T: Serialize>(&self, key: &str, value: &T) {
        let result = serde_json::to_string(value).map_err(anyhow::Error::from).and_then(|v| {
            self.db().execute(
                "insert into kv values (?, ?) on conflict(key) do update set value = excluded.value",
                params![key, v],
            )?;
            Ok(())
        });
        if let Err(e) = result {
            tracing::error!("saving {key} failed: {e}");
        }
    }

    pub fn load_settings(&self) -> Settings {
        self.get("settings").unwrap_or_default()
    }

    pub fn save_settings(&self, settings: &Settings) {
        self.put("settings", settings);
    }

    pub fn load_runtime(&self) -> Runtime {
        self.get("runtime").unwrap_or_default()
    }

    pub fn save_runtime(&self, runtime: &Runtime) {
        self.put("runtime", runtime);
    }
}
