// SPDX-FileCopyrightText: 2026 Miikka Koskinen
//
// SPDX-License-Identifier: MIT

use anyhow::Result;
use chrono::{Days, NaiveDate, Utc};
use rusqlite::Connection;
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

pub type Db = Arc<Mutex<Connection>>;

/// Ordered list of migrations. Each entry is a SQL batch to run.
/// Index 0 is the initial schema. Append new migrations at the end.
const MIGRATIONS: &[&str] = &[
    // 0: initial schema
    "CREATE TABLE IF NOT EXISTS meetings (
         id         TEXT    PRIMARY KEY,
         title      TEXT    NOT NULL,
         created_at TEXT    NOT NULL
     );

     CREATE TABLE IF NOT EXISTS time_slots (
         id         INTEGER PRIMARY KEY AUTOINCREMENT,
         meeting_id TEXT    NOT NULL REFERENCES meetings(id) ON DELETE CASCADE,
         label      TEXT    NOT NULL,
         slot_dt    TEXT    NOT NULL
     );

     CREATE TABLE IF NOT EXISTS participants (
         id         INTEGER PRIMARY KEY AUTOINCREMENT,
         meeting_id TEXT    NOT NULL REFERENCES meetings(id) ON DELETE CASCADE,
         name       TEXT    NOT NULL
     );

     CREATE TABLE IF NOT EXISTS availabilities (
         participant_id INTEGER NOT NULL REFERENCES participants(id) ON DELETE CASCADE,
         slot_id        INTEGER NOT NULL REFERENCES time_slots(id) ON DELETE CASCADE,
         PRIMARY KEY (participant_id, slot_id)
     );",
    // 1: add edit_token to participants
    "ALTER TABLE participants ADD COLUMN edit_token TEXT;",
    // 2: index lookups by meeting
    "CREATE INDEX IF NOT EXISTS time_slots_meeting_id ON time_slots(meeting_id);
     CREATE INDEX IF NOT EXISTS participants_meeting_id ON participants(meeting_id);",
];

/// The WAL file is truncated to this size after checkpoints.
const JOURNAL_SIZE_LIMIT_BYTES: u64 = 64 * 1024 * 1024;

pub fn open(path: &str) -> Result<Db> {
    let conn = Connection::open(path)?;
    conn.execute_batch(&format!(
        "PRAGMA journal_mode=WAL;
             PRAGMA foreign_keys=ON;
             PRAGMA journal_size_limit={JOURNAL_SIZE_LIMIT_BYTES};"
    ))?;
    run_migrations(&conn)?;
    Ok(Arc::new(Mutex::new(conn)))
}

/// Cap the database file size. Writes beyond it fail with `SQLITE_FULL`.
pub fn set_max_size(db: &Db, max_bytes: u64) -> Result<()> {
    let conn = db.lock().unwrap();
    let page_size: u64 = conn.pragma_query_value(None, "page_size", |row| row.get(0))?;
    conn.pragma_update(None, "max_page_count", max_bytes / page_size)?;
    Ok(())
}

/// True if `err` was caused by the database reaching its size limit.
pub fn is_full(err: &anyhow::Error) -> bool {
    matches!(
        err.downcast_ref::<rusqlite::Error>(),
        Some(rusqlite::Error::SqliteFailure(e, _)) if e.code == rusqlite::ErrorCode::DiskFull
    )
}

/// Delete meetings that were created before `cutoff` and have no time slot
/// on or after it. Slots with unparseable dates are ignored. Returns the
/// number of meetings deleted; their slots and responses cascade.
pub fn delete_expired(db: &Db, cutoff: NaiveDate) -> Result<usize> {
    let cutoff = cutoff.format("%Y-%m-%d").to_string();
    let conn = db.lock().unwrap();
    let deleted = conn.execute(
        "DELETE FROM meetings
         WHERE substr(created_at, 1, 10) < ?1
           AND NOT EXISTS (
               SELECT 1 FROM time_slots
               WHERE time_slots.meeting_id = meetings.id
                 AND date(substr(slot_dt, 1, 10)) >= ?1
           )",
        [cutoff],
    )?;
    Ok(deleted)
}

/// How often `expire_periodically` checks for expired meetings.
const EXPIRY_INTERVAL: Duration = Duration::from_secs(60 * 60);

/// Every hour, delete meetings that have been inactive for `retention_days`,
/// measured from the later of their creation and their last time slot.
pub async fn expire_periodically(db: Db, retention_days: u32) {
    let mut interval = tokio::time::interval(EXPIRY_INTERVAL);
    loop {
        interval.tick().await;
        let cutoff = Utc::now().date_naive() - Days::new(retention_days.into());
        match delete_expired(&db, cutoff) {
            Ok(0) => {}
            Ok(n) => tracing::info!("Deleted {n} expired meetings"),
            Err(e) => tracing::error!("Failed to delete expired meetings: {e:#}"),
        }
    }
}

fn run_migrations(conn: &Connection) -> Result<()> {
    apply_migrations(conn, MIGRATIONS)
}

fn apply_migrations(conn: &Connection, migrations: &[&str]) -> Result<()> {
    let current_version: u32 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;

    for (i, migration) in migrations.iter().enumerate() {
        let version = i as u32;
        if version >= current_version {
            conn.execute_batch(&format!(
                "BEGIN;\n{migration}\nPRAGMA user_version = {new_version};\nCOMMIT;",
                new_version = version + 1,
            ))?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user_version(conn: &Connection) -> u32 {
        conn.pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap()
    }

    #[test]
    fn fresh_db_applies_all_migrations() {
        let conn = Connection::open_in_memory().unwrap();
        apply_migrations(
            &conn,
            &[
                "CREATE TABLE t1 (id INTEGER PRIMARY KEY);",
                "CREATE TABLE t2 (id INTEGER PRIMARY KEY);",
            ],
        )
        .unwrap();

        assert_eq!(user_version(&conn), 2);
        // Both tables exist
        let tables: Vec<String> = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<std::result::Result<_, _>>()
            .unwrap();
        assert_eq!(tables, vec!["t1", "t2"]);
    }

    #[test]
    fn skips_already_applied_migrations() {
        let conn = Connection::open_in_memory().unwrap();

        // Run first migration only
        apply_migrations(&conn, &["CREATE TABLE t1 (id INTEGER PRIMARY KEY);"]).unwrap();
        assert_eq!(user_version(&conn), 1);

        // Now run with two migrations — only the second should execute
        apply_migrations(
            &conn,
            &[
                "CREATE TABLE t1 (id INTEGER PRIMARY KEY);",
                "CREATE TABLE t2 (id INTEGER PRIMARY KEY);",
            ],
        )
        .unwrap();
        assert_eq!(user_version(&conn), 2);

        // t1 still exists (wasn't dropped/recreated), t2 was added
        let count: u32 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name IN ('t1','t2')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 2);
    }

    #[test]
    fn real_migrations_apply_to_fresh_db() {
        let conn = Connection::open_in_memory().unwrap();
        apply_migrations(&conn, MIGRATIONS).unwrap();

        assert_eq!(user_version(&conn), MIGRATIONS.len() as u32);

        // Spot-check that the schema is usable
        conn.execute(
            "INSERT INTO meetings (id, title, created_at) VALUES ('m1', 'Test', '2025-01-01')",
            [],
        )
        .unwrap();
        let title: String = conn
            .query_row("SELECT title FROM meetings WHERE id = 'm1'", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(title, "Test");
    }

    #[test]
    fn set_max_size_rejects_writes_past_limit() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let db = open(tmp.path().to_str().unwrap()).unwrap();
        set_max_size(&db, 64 * 1024).unwrap();
        let conn = db.lock().unwrap();
        conn.execute(
            "INSERT INTO meetings (id, title, created_at) VALUES ('m1', 'T', '2026-01-01')",
            [],
        )
        .unwrap();
        let big = "x".repeat(1024 * 1024);
        let err = conn
            .execute(
                "INSERT INTO time_slots (meeting_id, label, slot_dt) VALUES ('m1', ?1, '')",
                [&big],
            )
            .unwrap_err();
        assert!(is_full(&anyhow::Error::from(err)));
    }

    fn date(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    fn insert_meeting(db: &Db, id: &str, created_at: &str, slots: &[&str]) {
        let conn = db.lock().unwrap();
        conn.execute(
            "INSERT INTO meetings (id, title, created_at) VALUES (?1, 'T', ?2)",
            [id, created_at],
        )
        .unwrap();
        for slot in slots {
            conn.execute(
                "INSERT INTO time_slots (meeting_id, label, slot_dt) VALUES (?1, 'L', ?2)",
                [id, slot],
            )
            .unwrap();
        }
    }

    fn meeting_ids(db: &Db) -> Vec<String> {
        db.lock()
            .unwrap()
            .prepare("SELECT id FROM meetings ORDER BY id")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<std::result::Result<_, _>>()
            .unwrap()
    }

    #[test]
    fn delete_expired_removes_only_inactive_meetings() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let db = open(tmp.path().to_str().unwrap()).unwrap();
        let old = "2026-01-01T12:00:00+00:00";
        insert_meeting(&db, "old", old, &["2026-01-10T10:00"]);
        insert_meeting(&db, "noslots", old, &[]);
        insert_meeting(&db, "garbage", old, &["not-a-dateT00:00"]);
        insert_meeting(
            &db,
            "future",
            old,
            &["2026-01-10T10:00", "2026-06-01T00:00"],
        );
        insert_meeting(&db, "recent", "2026-06-01T00:00:00+00:00", &[]);

        let conn = db.lock().unwrap();
        conn.execute(
            "INSERT INTO participants (meeting_id, name) VALUES ('old', 'Alice')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO availabilities (participant_id, slot_id)
             SELECT p.id, s.id FROM participants p, time_slots s WHERE s.meeting_id = 'old'",
            [],
        )
        .unwrap();
        drop(conn);

        let deleted = delete_expired(&db, date("2026-06-01")).unwrap();

        assert_eq!(deleted, 3);
        assert_eq!(meeting_ids(&db), vec!["future", "recent"]);
        let conn = db.lock().unwrap();
        let orphans: u32 = conn
            .query_row(
                "SELECT (SELECT COUNT(*) FROM participants WHERE meeting_id = 'old')
                      + (SELECT COUNT(*) FROM availabilities)
                      + (SELECT COUNT(*) FROM time_slots WHERE meeting_id = 'old')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(orphans, 0);
    }

    #[tokio::test]
    async fn expire_periodically_runs_immediately() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let db = open(tmp.path().to_str().unwrap()).unwrap();
        insert_meeting(&db, "old", "2000-01-01T00:00:00+00:00", &[]);
        insert_meeting(&db, "new", &Utc::now().to_rfc3339(), &[]);

        let task = tokio::spawn(expire_periodically(db.clone(), 90));
        for _ in 0..100 {
            if meeting_ids(&db) == vec!["new"] {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        task.abort();

        assert_eq!(meeting_ids(&db), vec!["new"]);
    }

    #[test]
    fn participants_have_edit_token_column() {
        let conn = Connection::open_in_memory().unwrap();
        apply_migrations(&conn, MIGRATIONS).unwrap();

        // Should be able to insert with an edit_token and retrieve it
        conn.execute(
            "INSERT INTO meetings (id, title, created_at) VALUES ('m1', 'T', '2026-01-01')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO participants (meeting_id, name, edit_token) VALUES ('m1', 'Alice', 'tok')",
            [],
        )
        .unwrap();
        let token: Option<String> = conn
            .query_row(
                "SELECT edit_token FROM participants WHERE name = 'Alice'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(token.as_deref(), Some("tok"));
    }
}
