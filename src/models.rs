// SPDX-FileCopyrightText: 2026 Miikka Koskinen
//
// SPDX-License-Identifier: MIT

use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct Meeting {
    pub id: String,
    pub title: String,
    pub created_at: String,
}

#[derive(Debug, Serialize)]
pub struct TimeSlot {
    pub id: i64,
    pub meeting_id: String,
    pub label: String,
    pub slot_dt: String,
    /// True when multiple slots share the same weekday and the date number should be shown.
    pub show_date: bool,
}

#[derive(Debug, Serialize)]
#[allow(dead_code)]
pub struct Participant {
    pub id: i64,
    pub meeting_id: String,
    pub name: String,
}

#[derive(Debug, Serialize)]
pub struct GridRow {
    pub participant_name: String,
    pub availability: Vec<bool>,
}

#[derive(Debug, Serialize)]
pub struct MeetingView {
    pub meeting: Meeting,
    pub slots: Vec<TimeSlot>,
    pub grid: Vec<GridRow>,
    pub slot_counts: Vec<usize>,
}
