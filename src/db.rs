use anyhow::Result;
use rusqlite::Connection;
use std::sync::{Arc, Mutex};

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
];

pub fn open(path: &str) -> Result<Db> {
    let conn = Connection::open(path)?;
    conn.execute_batch(
        "PRAGMA journal_mode=WAL;
         PRAGMA foreign_keys=ON;",
    )?;
    run_migrations(&conn)?;
    Ok(Arc::new(Mutex::new(conn)))
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
}
