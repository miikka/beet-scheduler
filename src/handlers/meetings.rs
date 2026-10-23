// SPDX-FileCopyrightText: 2026 Miikka Koskinen
//
// SPDX-License-Identifier: MIT

use crate::{
    db::Db,
    error::AppError,
    limits::{
        MAX_DATE_CHARS, MAX_LABEL_CHARS, MAX_SLOT_DAYS_AHEAD, MAX_SLOT_SPAN_DAYS,
        MAX_SLOTS_PER_MEETING, MAX_TIME_CHARS, MAX_TITLE_CHARS, check_all, check_len,
    },
};
use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Redirect, Response},
};
use chrono::{Days, NaiveDate, NaiveTime, Utc};
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

/// A validated time slot, ready to insert.
#[derive(Debug, PartialEq)]
struct NewSlot {
    label: String,
    /// `YYYY-MM-DDTHH:MM`
    slot_dt: String,
}

impl CreateMeetingForm {
    /// Check limits and parse the slots. Rows with a blank date are skipped.
    fn validate(&self, today: NaiveDate) -> Result<Vec<NewSlot>, String> {
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

        let latest_allowed = today + Days::new(MAX_SLOT_DAYS_AHEAD);
        let mut slots = Vec::new();
        let mut dates = Vec::new();
        for (i, date_str) in self.slot_date.iter().enumerate() {
            let date_str = date_str.trim();
            if date_str.is_empty() {
                continue;
            }
            let date = parse_date(date_str)?;
            if date > latest_allowed {
                return Err(format!(
                    "slot_date must be at most {MAX_SLOT_DAYS_AHEAD} days in the future"
                ));
            }

            let time_str = self.slot_time.get(i).map(|s| s.trim()).unwrap_or("");
            let time = if time_str.is_empty() {
                None
            } else {
                Some(parse_time(time_str)?)
            };
            let custom_label = self.slot_label.get(i).map(|s| s.trim()).unwrap_or("");

            let label = if !custom_label.is_empty() {
                custom_label.to_string()
            } else if let Some(time) = time {
                format!("{} {}", date.format("%A"), time.format("%H:%M"))
            } else {
                date.format("%A").to_string()
            };
            let slot_dt = date
                .and_time(time.unwrap_or(NaiveTime::MIN))
                .format("%Y-%m-%dT%H:%M")
                .to_string();

            dates.push(date);
            slots.push(NewSlot { label, slot_dt });
        }

        if let (Some(first), Some(last)) = (dates.iter().min(), dates.iter().max())
            && (*last - *first).num_days() > MAX_SLOT_SPAN_DAYS
        {
            return Err(format!(
                "slot dates must be within {MAX_SLOT_SPAN_DAYS} days of each other"
            ));
        }
        Ok(slots)
    }
}

/// Parse a strict, zero-padded `YYYY-MM-DD` date.
fn parse_date(s: &str) -> Result<NaiveDate, String> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d")
        .ok()
        .filter(|d| d.format("%Y-%m-%d").to_string() == s)
        .ok_or_else(|| format!("invalid slot_date {s:?}, expected YYYY-MM-DD"))
}

/// Parse `HH:MM` or `HH:MM:SS`. Callers format it as `HH:MM`, dropping seconds.
fn parse_time(s: &str) -> Result<NaiveTime, String> {
    NaiveTime::parse_from_str(s, "%H:%M")
        .or_else(|_| NaiveTime::parse_from_str(s, "%H:%M:%S"))
        .map_err(|_| format!("invalid slot_time {s:?}, expected HH:MM"))
}

pub async fn create(
    State(db): State<Db>,
    QsForm(form): QsForm<CreateMeetingForm>,
) -> Result<Response, AppError> {
    let now = Utc::now();
    let slots = match form.validate(now.date_naive()) {
        Ok(slots) => slots,
        Err(msg) => return Ok((StatusCode::UNPROCESSABLE_ENTITY, msg).into_response()),
    };

    let id = generate_id();

    let mut conn = db.lock().unwrap();
    let conn = conn.transaction()?;
    conn.execute(
        "INSERT INTO meetings (id, title, created_at) VALUES (?1, ?2, ?3)",
        rusqlite::params![id, form.title, now.to_rfc3339()],
    )?;
    for slot in slots {
        conn.execute(
            "INSERT INTO time_slots (meeting_id, label, slot_dt) VALUES (?1, ?2, ?3)",
            rusqlite::params![id, slot.label, slot.slot_dt],
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

#[cfg(test)]
mod tests {
    use super::*;

    fn form(dates: &[&str], times: &[&str]) -> CreateMeetingForm {
        CreateMeetingForm {
            title: "T".to_string(),
            slot_label: vec![],
            slot_date: dates.iter().map(|s| s.to_string()).collect(),
            slot_time: times.iter().map(|s| s.to_string()).collect(),
        }
    }

    fn today() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 10, 23).unwrap()
    }

    #[test]
    fn validate_normalizes_slots() {
        let slots = form(&["2026-10-23", " ", "2026-10-24"], &["09:30:15", "", ""])
            .validate(today())
            .unwrap();
        assert_eq!(
            slots,
            vec![
                NewSlot {
                    label: "Friday 09:30".to_string(),
                    slot_dt: "2026-10-23T09:30".to_string(),
                },
                NewSlot {
                    label: "Saturday".to_string(),
                    slot_dt: "2026-10-24T00:00".to_string(),
                },
            ]
        );
    }

    #[test]
    fn validate_rejects_malformed_dates_and_times() {
        for date in ["2026-02-30", "2026-3-02", "02/03/2026", "\\\"); x"] {
            assert!(form(&[date], &[]).validate(today()).is_err(), "{date}");
        }
        for time in ["25:00", "9", "12:00pm"] {
            assert!(
                form(&["2026-10-23"], &[time]).validate(today()).is_err(),
                "{time}"
            );
        }
    }

    #[test]
    fn validate_bounds_date_range() {
        let latest = today() + Days::new(MAX_SLOT_DAYS_AHEAD);
        let latest_str = latest.format("%Y-%m-%d").to_string();
        let too_late = (latest + Days::new(1)).format("%Y-%m-%d").to_string();
        assert!(form(&[&latest_str], &[]).validate(today()).is_ok());
        assert!(form(&[&too_late], &[]).validate(today()).is_err());

        // Past dates are fine, but slots must not be spread too far apart.
        assert!(form(&["1999-01-01"], &[]).validate(today()).is_ok());
        assert!(
            form(&["2025-10-22", "2026-10-23"], &[])
                .validate(today())
                .is_ok()
        );
        assert!(
            form(&["2025-10-21", "2026-10-23"], &[])
                .validate(today())
                .is_err()
        );
    }
}
