// SPDX-FileCopyrightText: 2026 Miikka Koskinen
//
// SPDX-License-Identifier: MIT

use subtle::ConstantTimeEq;

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{Html, IntoResponse, Redirect, Response},
};
use minijinja::Environment;
use rusqlite::OptionalExtension;
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::{
    db::Db,
    error::AppError,
    models::{GridRow, Meeting, MeetingView, TimeSlot},
};

use super::QsForm;

fn generate_token() -> String {
    use rand::RngExt;
    let bytes: [u8; 16] = rand::rng().random();
    bytes
        .iter()
        .fold(String::with_capacity(32), |mut s: String, b| {
            use std::fmt::Write;
            write!(s, "{:02x}", b).unwrap();
            s
        })
}

/// Cookie value format: `<pid>_<token>.<pid>_<token>...`
/// All characters (digits, a-f, underscore, dot) are RFC 6265 cookie-safe.
fn parse_edit_tokens_value(value: &str) -> HashMap<i64, String> {
    if value.is_empty() {
        return HashMap::new();
    }
    value
        .split('.')
        .filter_map(|entry| {
            let (pid_str, token) = entry.split_once('_')?;
            let pid = pid_str.parse::<i64>().ok()?;
            Some((pid, token.to_string()))
        })
        .collect()
}

fn build_edit_tokens_value(tokens: &HashMap<i64, String>) -> String {
    let mut pairs: Vec<_> = tokens
        .iter()
        .map(|(pid, tok)| format!("{}_{}", pid, tok))
        .collect();
    pairs.sort(); // deterministic for tests
    pairs.join(".")
}

fn parse_edit_tokens(headers: &HeaderMap) -> HashMap<i64, String> {
    let cookie_header = headers
        .get(axum::http::header::COOKIE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    for cookie in cookie_header.split(';') {
        if let Some(value) = cookie.trim().strip_prefix("edit_tokens=") {
            return parse_edit_tokens_value(value);
        }
    }
    HashMap::new()
}

fn build_set_cookie_header(meeting_id: &str, tokens: &HashMap<i64, String>) -> String {
    format!(
        "edit_tokens={}; Path=/m/{}; Max-Age=7776000; HttpOnly; SameSite=Lax",
        build_edit_tokens_value(tokens),
        meeting_id,
    )
}

fn insert_participant(
    conn: &rusqlite::Connection,
    meeting_id: &str,
    name: &str,
    edit_tokens: &mut HashMap<i64, String>,
) -> Result<i64, rusqlite::Error> {
    let token = generate_token();
    let pid: i64 = conn.query_row(
        "INSERT INTO participants (meeting_id, name, edit_token) VALUES (?1, ?2, ?3) RETURNING id",
        rusqlite::params![meeting_id, name, &token],
        |row| row.get(0),
    )?;
    edit_tokens.insert(pid, token);
    Ok(pid)
}

fn verify_edit_tokens(
    conn: &rusqlite::Connection,
    meeting_id: &str,
    edit_tokens: &HashMap<i64, String>,
) -> Result<HashSet<i64>, rusqlite::Error> {
    let mut editable = HashSet::new();
    for (pid, cookie_token) in edit_tokens {
        let stored: Option<Option<String>> = conn
            .query_row(
                "SELECT edit_token FROM participants WHERE id = ?1 AND meeting_id = ?2",
                rusqlite::params![pid, meeting_id],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(Some(db_token)) = stored
            && db_token.as_bytes().ct_eq(cookie_token.as_bytes()).into()
        {
            editable.insert(*pid);
        }
    }
    Ok(editable)
}

pub async fn show_meeting(
    State(db): State<Db>,
    State(env): State<Arc<Environment<'static>>>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    let edit_tokens = parse_edit_tokens(&headers);
    let editable_ids = {
        let conn = db.lock().unwrap();
        verify_edit_tokens(&conn, &id, &edit_tokens)?
    };
    match load_meeting_view(&db, &id, &editable_ids)? {
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
    pub participant_id: Option<i64>,
}

pub async fn submit_response(
    State(db): State<Db>,
    State(env): State<Arc<Environment<'static>>>,
    Path(id): Path<String>,
    headers: HeaderMap,
    QsForm(form): QsForm<SubmitResponseForm>,
) -> Result<Response, AppError> {
    let mut edit_tokens = parse_edit_tokens(&headers);

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

        let pid: i64 = if let Some(form_pid) = form.participant_id {
            // Token-based edit: participant_id explicitly provided in form
            let stored: Option<Option<String>> = conn
                .query_row(
                    "SELECT edit_token FROM participants WHERE id = ?1 AND meeting_id = ?2",
                    rusqlite::params![form_pid, id],
                    |row| row.get(0),
                )
                .optional()?;
            let token_valid = stored
                .flatten()
                .map(|db_tok| {
                    edit_tokens
                        .get(&form_pid)
                        .is_some_and(|ct| ct.as_bytes().ct_eq(db_tok.as_bytes()).into())
                })
                .unwrap_or(false);
            if token_valid {
                conn.execute(
                    "UPDATE participants SET name = ?1 WHERE id = ?2",
                    rusqlite::params![form.name.trim(), form_pid],
                )?;
                form_pid
            } else {
                insert_participant(&conn, &id, form.name.trim(), &mut edit_tokens)?
            }
        } else {
            // Name-based: look up existing participant
            let existing: Option<(i64, Option<String>)> = conn
                .query_row(
                    "SELECT id, edit_token FROM participants WHERE meeting_id = ?1 AND name = ?2",
                    rusqlite::params![id, form.name.trim()],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()?;
            match existing {
                None => insert_participant(&conn, &id, form.name.trim(), &mut edit_tokens)?,
                Some((existing_pid, Some(ref db_tok)))
                    if edit_tokens.get(&existing_pid) == Some(db_tok) =>
                {
                    existing_pid
                }
                Some(_) => insert_participant(&conn, &id, form.name.trim(), &mut edit_tokens)?,
            }
        };

        // Reset and re-insert availabilities
        conn.execute(
            "DELETE FROM availabilities WHERE participant_id = ?1",
            rusqlite::params![pid],
        )?;
        for slot_id in &form.slot_ids {
            conn.execute(
                "INSERT OR IGNORE INTO availabilities (participant_id, slot_id) VALUES (?1, ?2)",
                rusqlite::params![pid, slot_id],
            )?;
        }
    }

    let cookie_header = build_set_cookie_header(&id, &edit_tokens);
    let is_htmx = headers.contains_key("hx-request");

    if is_htmx {
        let editable_ids = {
            let conn = db.lock().unwrap();
            verify_edit_tokens(&conn, &id, &edit_tokens)?
        };
        let view = load_meeting_view(&db, &id, &editable_ids)?
            .unwrap_or_else(|| panic!("meeting vanished"));
        let tmpl = env.get_template("partials/grid.html")?;
        let rendered = tmpl.render(minijinja::context! { view => view })?;
        let mut response = Html(rendered).into_response();
        response.headers_mut().insert(
            axum::http::header::SET_COOKIE,
            axum::http::HeaderValue::from_str(&cookie_header)
                .expect("cookie header is valid ASCII"),
        );
        Ok(response)
    } else {
        let mut response = Redirect::to(&format!("/m/{}", id)).into_response();
        response.headers_mut().insert(
            axum::http::header::SET_COOKIE,
            axum::http::HeaderValue::from_str(&cookie_header)
                .expect("cookie header is valid ASCII"),
        );
        Ok(response)
    }
}

pub fn load_meeting_view(
    db: &Db,
    id: &str,
    editable_ids: &HashSet<i64>,
) -> Result<Option<MeetingView>, AppError> {
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
    let mut slots: Vec<TimeSlot> = stmt
        .query_map(rusqlite::params![id], |row| {
            Ok(TimeSlot {
                id: row.get(0)?,
                meeting_id: row.get(1)?,
                label: row.get(2)?,
                slot_dt: row.get(3)?,
                show_date: false,
            })
        })?
        .collect::<Result<_, _>>()?;

    // Mark slots that share a label with another slot so the date number can be shown.
    // If any slot needs disambiguation, show dates on all slots for consistency.
    let mut label_counts: HashMap<String, usize> = HashMap::new();
    for slot in &slots {
        *label_counts.entry(slot.label.clone()).or_default() += 1;
    }
    let any_duplicate = label_counts.values().any(|&c| c > 1);
    for slot in &mut slots {
        slot.show_date = any_duplicate;
    }

    let mut stmt =
        conn.prepare("SELECT id, name FROM participants WHERE meeting_id = ?1 ORDER BY id")?;
    let participants: Vec<(i64, String)> = stmt
        .query_map(rusqlite::params![id], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<Result<_, _>>()?;

    let mut grid = Vec::new();
    for (pid, name) in &participants {
        let mut availability = vec![false; slots.len()];
        let mut stmt =
            conn.prepare("SELECT slot_id FROM availabilities WHERE participant_id = ?1")?;
        let avail_slots: Vec<i64> = stmt
            .query_map(rusqlite::params![pid], |row| row.get(0))?
            .collect::<Result<_, _>>()?;

        for (i, slot) in slots.iter().enumerate() {
            if avail_slots.contains(&slot.id) {
                availability[i] = true;
            }
        }
        grid.push(GridRow {
            participant_id: *pid,
            participant_name: name.clone(),
            availability,
            editable: editable_ids.contains(pid),
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

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderMap;

    #[test]
    fn generate_token_is_32_hex_chars() {
        let tok = generate_token();
        assert_eq!(tok.len(), 32);
        assert!(tok.chars().all(|c| c.is_ascii_hexdigit()), "all hex: {tok}");
    }

    #[test]
    fn generate_token_is_unique() {
        let a = generate_token();
        let b = generate_token();
        assert_ne!(a, b);
    }

    #[test]
    fn roundtrip_empty_cookie() {
        let map: HashMap<i64, String> = HashMap::new();
        let val = build_edit_tokens_value(&map);
        assert_eq!(val, "");
        let parsed = parse_edit_tokens_value(&val);
        assert!(parsed.is_empty());
    }

    #[test]
    fn roundtrip_single_token() {
        let mut map = HashMap::new();
        map.insert(42i64, "abc123def456abc123def456abc123ef".to_string());
        let val = build_edit_tokens_value(&map);
        let parsed = parse_edit_tokens_value(&val);
        assert_eq!(
            parsed.get(&42).map(String::as_str),
            Some("abc123def456abc123def456abc123ef")
        );
    }

    #[test]
    fn roundtrip_multiple_tokens() {
        let mut map = HashMap::new();
        map.insert(1i64, "a".repeat(32));
        map.insert(2i64, "b".repeat(32));
        let val = build_edit_tokens_value(&map);
        let parsed = parse_edit_tokens_value(&val);
        assert_eq!(parsed.len(), 2);
        assert_eq!(
            parsed.get(&1).map(String::as_str),
            Some("a".repeat(32).as_str())
        );
        assert_eq!(
            parsed.get(&2).map(String::as_str),
            Some("b".repeat(32).as_str())
        );
    }

    #[test]
    fn parse_edit_tokens_from_header() {
        let mut headers = HeaderMap::new();
        let pid = 7i64;
        let tok = "c".repeat(32);
        headers.insert(
            axum::http::header::COOKIE,
            format!("other=x; edit_tokens={}_{}", pid, tok)
                .parse()
                .unwrap(),
        );
        let map = parse_edit_tokens(&headers);
        assert_eq!(map.get(&pid).map(String::as_str), Some(tok.as_str()));
    }

    #[test]
    fn parse_edit_tokens_missing_cookie() {
        let headers = HeaderMap::new();
        let map = parse_edit_tokens(&headers);
        assert!(map.is_empty());
    }

    #[test]
    fn build_set_cookie_header_format() {
        let mut map = HashMap::new();
        map.insert(5i64, "d".repeat(32));
        let header = build_set_cookie_header("abc12345", &map);
        assert!(
            header.starts_with("edit_tokens="),
            "should start with cookie name"
        );
        assert!(
            header.contains("Path=/m/abc12345"),
            "should have meeting path"
        );
        assert!(header.contains("Max-Age=7776000"), "90 days");
        assert!(header.contains("HttpOnly"));
        assert!(header.contains("SameSite=Lax"));
    }
}
