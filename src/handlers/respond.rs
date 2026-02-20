use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{Html, IntoResponse, Redirect, Response},
};
use minijinja::Environment;
use rusqlite::OptionalExtension;
use serde::Deserialize;
use std::sync::Arc;

use crate::{
    db::Db,
    error::AppError,
    models::{GridRow, Meeting, MeetingView, TimeSlot},
};

use super::QsForm;

pub async fn show_meeting(
    State(db): State<Db>,
    State(env): State<Arc<Environment<'static>>>,
    Path(id): Path<String>,
) -> Result<Response, AppError> {
    match load_meeting_view(&db, &id)? {
        None => Ok((StatusCode::NOT_FOUND, "Meeting not found").into_response()),
        Some(view) => {
            let tmpl = env.get_template("meeting.html")?;
            let rendered = tmpl.render(minijinja::context! { view => view })?;
            Ok(Html(rendered).into_response())
        }
    }
}

#[derive(Deserialize)]
pub struct SubmitResponseForm {
    pub name: String,
    #[serde(default)]
    pub slot_ids: Vec<i64>,
}

pub async fn submit_response(
    State(db): State<Db>,
    State(env): State<Arc<Environment<'static>>>,
    Path(id): Path<String>,
    headers: HeaderMap,
    QsForm(form): QsForm<SubmitResponseForm>,
) -> Result<Response, AppError> {
    {
        let conn = db.lock().unwrap();

        // Verify meeting exists
        let exists: bool = conn
            .query_row(
                "SELECT COUNT(*) FROM meetings WHERE id = ?1",
                rusqlite::params![id],
                |row| row.get::<_, i64>(0),
            )
            .map(|c| c > 0)
            .unwrap_or(false);

        if !exists {
            return Ok((StatusCode::NOT_FOUND, "Meeting not found").into_response());
        }

        let existing_id: Option<i64> = conn
            .query_row(
                "SELECT id FROM participants WHERE meeting_id = ?1 AND name = ?2",
                rusqlite::params![id, form.name.trim()],
                |row| row.get(0),
            )
            .optional()?;

        let participant_id = if let Some(pid) = existing_id {
            conn.execute(
                "DELETE FROM availabilities WHERE participant_id = ?1",
                rusqlite::params![pid],
            )?;
            pid
        } else {
            conn.query_row(
                "INSERT INTO participants (meeting_id, name) VALUES (?1, ?2) RETURNING id",
                rusqlite::params![id, form.name.trim()],
                |row| row.get(0),
            )?
        };

        for slot_id in &form.slot_ids {
            conn.execute(
                "INSERT OR IGNORE INTO availabilities (participant_id, slot_id) VALUES (?1, ?2)",
                rusqlite::params![participant_id, slot_id],
            )?;
        }
    }

    let is_htmx = headers.contains_key("hx-request");

    if is_htmx {
        let view = load_meeting_view(&db, &id)?.unwrap_or_else(|| panic!("meeting vanished"));
        let tmpl = env.get_template("partials/grid.html")?;
        let rendered = tmpl.render(minijinja::context! { view => view })?;
        Ok(Html(rendered).into_response())
    } else {
        Ok(Redirect::to(&format!("/m/{}", id)).into_response())
    }
}

pub fn load_meeting_view(db: &Db, id: &str) -> Result<Option<MeetingView>, AppError> {
    let conn = db.lock().unwrap();

    let Some(meeting) = conn
        .query_row(
            "SELECT id, title, created_at FROM meetings WHERE id = ?1",
            rusqlite::params![id],
            |row| {
                Ok(Meeting {
                    id: row.get(0)?,
                    title: row.get(1)?,
                    created_at: row.get(2)?,
                })
            },
        )
        .optional()?
    else {
        return Ok(None);
    };

    let mut stmt = conn.prepare(
        "SELECT id, meeting_id, label, slot_dt FROM time_slots WHERE meeting_id = ?1 ORDER BY slot_dt",
    )?;
    let slots: Vec<TimeSlot> = stmt
        .query_map(rusqlite::params![id], |row| {
            Ok(TimeSlot {
                id: row.get(0)?,
                meeting_id: row.get(1)?,
                label: row.get(2)?,
                slot_dt: row.get(3)?,
            })
        })?
        .collect::<Result<_, _>>()?;

    let mut stmt = conn.prepare(
        "SELECT id, name FROM participants WHERE meeting_id = ?1 ORDER BY id",
    )?;
    let participants: Vec<(i64, String)> = stmt
        .query_map(rusqlite::params![id], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<Result<_, _>>()?;

    let mut grid = Vec::new();
    for (pid, name) in &participants {
        let mut availability = vec![false; slots.len()];
        let mut stmt = conn.prepare(
            "SELECT slot_id FROM availabilities WHERE participant_id = ?1",
        )?;
        let avail_slots: Vec<i64> = stmt
            .query_map(rusqlite::params![pid], |row| row.get(0))?
            .collect::<Result<_, _>>()?;

        for (i, slot) in slots.iter().enumerate() {
            if avail_slots.contains(&slot.id) {
                availability[i] = true;
            }
        }
        grid.push(GridRow {
            participant_name: name.clone(),
            availability,
        });
    }

    let mut slot_counts = vec![0usize; slots.len()];
    for row in &grid {
        for (i, &avail) in row.availability.iter().enumerate() {
            if avail {
                slot_counts[i] += 1;
            }
        }
    }

    Ok(Some(MeetingView {
        meeting,
        slots,
        grid,
        slot_counts,
    }))
}
