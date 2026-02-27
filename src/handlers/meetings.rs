use crate::{db::Db, error::AppError};
use axum::{
    extract::State,
    response::{IntoResponse, Redirect},
};
use chrono::{NaiveDate, Utc};
use rand::{distributions::Alphanumeric, thread_rng, Rng};
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

pub async fn create(
    State(db): State<Db>,
    QsForm(form): QsForm<CreateMeetingForm>,
) -> Result<impl IntoResponse, AppError> {
    let id = generate_id();
    let now = Utc::now().to_rfc3339();

    let conn = db.lock().unwrap();
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

    Ok(Redirect::to(&format!("/m/{}", id)))
}

fn generate_id() -> String {
    thread_rng()
        .sample_iter(Alphanumeric)
        .take(8)
        .map(char::from)
        .collect()
}
