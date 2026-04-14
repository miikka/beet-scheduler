// SPDX-FileCopyrightText: 2026 Miikka Koskinen
//
// SPDX-License-Identifier: MIT

use axum::response::{Html, IntoResponse};
use minijinja::Environment;
use std::sync::Arc;

use crate::error::AppError;

pub async fn show(
    axum::extract::State(env): axum::extract::State<Arc<Environment<'static>>>,
) -> Result<impl IntoResponse, AppError> {
    let tmpl = env.get_template("home.html")?;
    let rendered = tmpl.render(minijinja::context! {})?;
    Ok(Html(rendered))
}

pub async fn new_slot_row() -> impl IntoResponse {
    Html(
        r#"<div class="slot-row">
  <input type="text" name="slot_label[]" placeholder="Label (optional)">
  <input type="date" name="slot_date[]" required>
  <input type="time" name="slot_time[]">
  <button type="button" class="remove-slot btn-secondary">Remove</button>
</div>"#,
    )
}
