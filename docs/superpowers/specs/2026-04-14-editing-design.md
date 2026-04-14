# Editing Availability — Design

Date: 2026-04-14

## Overview

Users can edit their previously submitted availability (name and time slots) using a randomly-generated edit token. The token is stored in the browser via a path-scoped cookie and in the database alongside the participant row. Only the browser that submitted a response can edit it.

## Database

Add a migration appending `edit_token TEXT` (nullable) to the `participants` table.

- New participants receive a randomly-generated 32-char hex token (16 bytes via `rand`).
- Existing rows keep `NULL`, meaning they are not editable by anyone.

## Cookie Format

Cookie name: `edit_tokens`  
Cookie path: `/m/{id}` (path-scoped to the specific meeting)  
Cookie value: JSON object mapping participant ID (as string key) to token string, e.g. `{"42": "a1b2c3..."}`.  
Max-Age: 7776000 seconds (90 days).  
Flags: `HttpOnly; SameSite=Lax`.

On each submission, the handler reads the existing `edit_tokens` cookie, merges the new `{participant_id: token}` entry, and sets the updated cookie on the response. This applies to both the redirect response and the HTMX partial response.

## Submit Flow (`POST /m/{id}/responses`)

The form gains an optional hidden field `participant_id`.

1. **Token-based edit path** — if `participant_id` is present in the form and the cookie holds a matching token for that ID in the DB: update the participant's name and availabilities. This is the primary edit path and supports name changes.
2. **Name-based new submission** — otherwise, look up any existing participant by `meeting_id + name`:
   - Not found → insert new participant with fresh token.
   - Found, cookie has matching token for that ID → delete old availabilities and re-insert (secondary edit path, no `participant_id` field needed).
   - Found, no matching token → insert new participant row with the same name and a fresh token (duplicate row allowed).

In all cases where a new participant is inserted, merge their `{id: token}` into the cookie and set `Set-Cookie`.

## Page Load (`GET /m/{id}`)

1. Parse the `edit_tokens` cookie from the `Cookie` header.
2. Build a `HashSet<i64>` of participant IDs whose cookie token matches the DB token.
3. Add `editable: bool` to `GridRow`. Set to `true` only for IDs in that set.
4. Template renders the Edit button only when `row.editable` is true.

## Edit Button (JavaScript)

The existing Edit button click handler additionally sets a hidden `<input name="participant_id">` field in the form with the participant's ID. The participant ID is stored in a `data-participant-id` attribute on the button (alongside the existing `data-name`).

## Model Changes

- `GridRow` gains `pub editable: bool`.
- `participants` DB table gains `edit_token TEXT` (nullable).

## Testing

Integration tests use `reqwest::ClientBuilder::new().cookie_store(true)` so cookies persist across requests within a test client.

1. After submitting, only the submitter's client sees Edit buttons for their row.
2. A different client (no cookie) sees no Edit buttons.
3. Editing with a valid token updates the row (name and availability).
4. Editing changes the name correctly (old name removed, new name shown).
5. Submitting the same name without a token creates a duplicate row instead of replacing.
6. Submitting with a forged/invalid token cookie is treated as a new submission, creating a duplicate row.
