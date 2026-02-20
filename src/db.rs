use anyhow::Result;
use rusqlite::Connection;
use std::sync::{Arc, Mutex};

pub type Db = Arc<Mutex<Connection>>;

pub fn open(path: &str) -> Result<Db> {
    let conn = Connection::open(path)?;
    init_schema(&conn)?;
    Ok(Arc::new(Mutex::new(conn)))
}

fn init_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "PRAGMA journal_mode=WAL;
         PRAGMA foreign_keys=ON;

         CREATE TABLE IF NOT EXISTS meetings (
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
    )?;
    Ok(())
}
