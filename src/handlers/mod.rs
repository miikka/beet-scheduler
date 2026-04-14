// SPDX-FileCopyrightText: 2026 Miikka Koskinen
//
// SPDX-License-Identifier: MIT

use axum::{
    body::Bytes,
    extract::{FromRequest, Request},
    http::StatusCode,
};
use serde::de::DeserializeOwned;

pub mod home;
pub mod meetings;
pub mod respond;

pub struct QsForm<T>(pub T);

impl<T, S> FromRequest<S> for QsForm<T>
where
    T: DeserializeOwned,
    S: Send + Sync,
{
    type Rejection = (StatusCode, String);

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        let bytes = Bytes::from_request(req, state)
            .await
            .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
        serde_qs::Config::new(5, false)
            .deserialize_bytes(&bytes)
            .map(QsForm)
            .map_err(|e| (StatusCode::UNPROCESSABLE_ENTITY, e.to_string()))
    }
}
