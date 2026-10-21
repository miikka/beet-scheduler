<!--
SPDX-FileCopyrightText: 2026 Miikka Koskinen

SPDX-License-Identifier: MIT
-->

# Security Issues

Source review for a deployment reachable from the open internet. Severity assumes a public service without protective reverse-proxy limits. Findings are based on code paths; no live penetration test was performed.

## HIGH: Unauthenticated database growth and application-wide denial of service

Anyone can call `POST /meetings` and `POST /m/{id}/responses` without authentication or rate limits (`src/lib.rs:54-66`). The server does not limit field lengths, numbers of slots or responses, or the number of submitted slot IDs (`src/handlers/meetings.rs:16-71`, `src/handlers/respond.rs:144-235`). The framework's request-body limit does not bound total database growth across requests.

Rendering a meeting loads every slot and participant, then checks each slot against that participant's availability with a linear search (`src/handlers/respond.rs:267-355`). This work holds the application's single SQLite connection mutex, so a large attacker-created meeting can delay requests for unrelated meetings. Repeated writes also grow the database and WAL.

**Fix:** Set server-side limits for field lengths, slots per meeting, participants per meeting, and submitted slot IDs. Rate limit write routes and bound database growth. Replace the nested availability search with a keyed lookup or a bounded SQL query; avoid holding the shared connection lock during expensive computation.

## MEDIUM: Duplicate names allow participant impersonation

Edit tokens protect existing response rows, but a caller without a matching token can submit the same name and create a second row (`src/handlers/respond.rs:205-220`). Anyone with a meeting link can make a response appear to come from another participant and change the displayed totals. This does not overwrite the token-holder's original row.

**Fix:** If participant identity is a requirement, use invitations or authentication. Otherwise make duplicate names visibly distinct and explain that names are self-reported.

## LOW: Edit-token cookie lacks `Secure`

The 90-day `edit_tokens` cookie has `HttpOnly` and explicit `SameSite=Lax`, but no `Secure` attribute (`src/handlers/respond.rs:76-81`). If the public service is reachable over HTTP, a network observer can capture the edit credential. The app itself listens on plain HTTP on all interfaces (`src/main.rs:45`); production safety depends on deployment configuration.

**Fix:** Serve the public endpoint over HTTPS, redirect HTTP to HTTPS, and set `Secure` on the cookie. Make the bind address configurable if the app may run without a reverse proxy.

## LOW: Cross-meeting slot IDs can be stored

`submit_response` inserts each submitted `slot_id` without checking that it belongs to the meeting (`src/handlers/respond.rs:224-233`). A valid slot ID from another meeting satisfies the foreign key and creates cross-meeting availability data. The current grid does not display it, but it wastes storage and violates the intended relationship.

**Fix:** Validate each slot ID against the target meeting, or insert from a query constrained by `meeting_id`.

## LOW: Blank participant names are accepted

The handler trims `name` but does not reject an empty result (`src/handlers/respond.rs:195-220`). Browser `required` validation is bypassable by a direct request.

**Reproduce:** `curl -X POST http://localhost:3000/m/<id>/responses --data-urlencode 'name=   '`

**Fix:** Reject blank names with a client-error response after trimming.

## LOW: Unvalidated dates can break the meeting calendar

`POST /meetings` accepts arbitrary `slot_date[]` text. A failed date parse only changes the displayed label; the original text is stored in `slot_dt` (`src/handlers/meetings.rs:39-69`). The first ten characters are later placed in the meeting page's JavaScript (`templates/meeting.html:63-65`). Malformed values can cause script syntax errors or unusable calendar ranges. The line breaks in the current template prevent the previously suspected backslash-based script-execution example from working.

**Fix:** Reject invalid or out-of-range dates server-side, and serialize dates as JSON for the script.

## LOW: Missing defensive response headers

The router adds no security response headers (`src/lib.rs:54-67`). A Content Security Policy would provide defense against future script injection once the inline scripts are accommodated. `frame-ancestors` can prevent framing, and `X-Content-Type-Options: nosniff` can prevent MIME sniffing. An explicit `Referrer-Policy` can document the intended handling of meeting URLs; modern browsers generally send only the origin, rather than the full meeting URL, on cross-origin HTTPS requests by default.

**Fix:** Add appropriate headers at the app or public reverse proxy and verify that the policy allows required scripts and styles.

## CSRF assessment

The POST routes do not validate `Origin` or use CSRF tokens. This matters if a deployment allows a hostile page on the same *site* (for example, an untrusted sibling subdomain), or if browser cookie behavior changes. However, the edit cookie explicitly uses `SameSite=Lax` (`src/handlers/respond.rs:78`), which generally prevents it from accompanying a cross-site HTML form POST. A cross-site form can still create an unauthenticated response or meeting, but a remote attacker can make those requests directly. The previous claim that an ordinary cross-site form can overwrite a token-holder's row was incorrect.

For stronger protection, validate the `Origin` header on state-changing routes or use CSRF tokens. `SameSite=Strict` can be considered for the edit cookie, but it does not replace input validation or abuse controls.
