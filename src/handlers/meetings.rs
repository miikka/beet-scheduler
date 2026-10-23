// SPDX-FileCopyrightText: 2026 Miikka Koskinen
//
// SPDX-License-Identifier: MIT

use crate::{
    db::Db,
    error::AppError,
    limits::{
        MAX_DATE_CHARS, MAX_LABEL_CHARS, MAX_SLOTS_PER_MEETING, MAX_TIME_CHARS, MAX_TITLE_CHARS,
        check_all, check_len,
    },
};
use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Redirect, Response},
};
use chrono::{NaiveDate, Utc};
use rand::{RngExt, distr::Alphanumeric, rng};
use serde::Deserialize;

use super::QsForm;

#[derive(Deserialize)]
pub struct CreateMeetingForm {
    pub title: String,
    #[serde(default)]
    pub slot_label: Vec<String>,
    pub slot_date: Vec<String>,
    #[serde(default)]
    pub slot_time: Vec<String>,
}

impl CreateMeetingForm {
    fn validate(&self) -> Result<(), String> {
        check_len("title", &self.title, MAX_TITLE_CHARS)?;
        check_all(
            "slot_label",
            &self.slot_label,
            MAX_SLOTS_PER_MEETING,
            MAX_LABEL_CHARS,
        )?;
        check_all(
            "slot_date",
            &self.slot_date,
            MAX_SLOTS_PER_MEETING,
            MAX_DATE_CHARS,
        )?;
        check_all(
            "slot_time",
            &self.slot_time,
            MAX_SLOTS_PER_MEETING,
            MAX_TIME_CHARS,
        )?;
        Ok(())
    }
}

pub async fn create(
    State(db): State<Db>,
    QsForm(form): QsForm<CreateMeetingForm>,
) -> Result<Response, AppError> {
    if let Err(msg) = form.validate() {
        return Ok((StatusCode::UNPROCESSABLE_ENTITY, msg).into_response());
    }

    let id = generate_id();
    let now = Utc::now().to_rfc3339();

    let mut conn = db.lock().unwrap();
    let conn = conn.transaction()?;
    conn.execute(
        "INSERT INTO meetings (id, title, created_at) VALUES (?1, ?2, ?3)",
        rusqlite::params![id, form.title, now],
    )?;

    for (i, date_str) in form.slot_date.iter().enumerate() {
        let date_str = date_str.trim();
        if date_str.is_empty() {
            continue;
        }

        let time_str = form.slot_time.get(i).map(|s| s.trim()).unwrap_or("");
        let custom_label = form.slot_label.get(i).map(|s| s.trim()).unwrap_or("");

        let label = if custom_label.is_empty() {
            let weekday = NaiveDate::parse_from_str(date_str, "%Y-%m-%d")
                .map(|d| d.format("%A").to_string())
                .unwrap_or_else(|_| date_str.to_string());
            if time_str.is_empty() {
                weekday
            } else {
                format!("{} {}", weekday, time_str)
            }
        } else {
            custom_label.to_string()
        };

        let slot_dt = if time_str.is_empty() {
            format!("{}T00:00", date_str)
        } else {
            format!("{}T{}", date_str, time_str)
        };

        conn.execute(
            "INSERT INTO time_slots (meeting_id, label, slot_dt) VALUES (?1, ?2, ?3)",
            rusqlite::params![id, label, slot_dt],
        )?;
    }
    conn.commit()?;

    Ok(Redirect::to(&format!("/m/{}", id)).into_response())
}

fn generate_id() -> String {
    rng()
        .sample_iter(Alphanumeric)
        .take(8)
        .map(char::from)
        .collect()
}
