<!--
SPDX-FileCopyrightText: 2026 Miikka Koskinen

SPDX-License-Identifier: MIT
-->

# Security Issues

Source review for a deployment reachable from the open internet. Severity assumes a public service without protective reverse-proxy limits. Findings are based on code paths; no live penetration test was performed.

## MEDIUM: Duplicate names allow participant impersonation

Edit tokens protect existing response rows, but a caller without a matching token can submit the same name and create a second row (`src/handlers/respond.rs:219-258`). Anyone with a meeting link can make a response appear to come from another participant and change the displayed totals. This does not overwrite the token-holder's original row.

**Fix:** If participant identity is a requirement, use invitations or authentication. Otherwise make duplicate names visibly distinct and explain that names are self-reported.

## LOW: Database size cap can be exhausted

Write routes are rate limited per client and the database has a configurable size cap (`max_db_size_mb`), so growth is bounded. Nothing expires old meetings, though. An attacker with many IP addresses, or a slow attacker over time, can fill the database to the cap, after which all writes fail with 503 until an operator intervenes.

**Fix:** Add a retention policy (for example, delete meetings whose slots are all in the past by some margin) and alert when the database approaches the cap.

## LOW: Edit-token cookie lacks `Secure`

The 90-day `edit_tokens` cookie has `HttpOnly` and explicit `SameSite=Lax`, but no `Secure` attribute (`src/handlers/respond.rs:77-83`). If the public service is reachable over HTTP, a network observer can capture the edit credential. The app itself listens on plain HTTP on all interfaces (`src/main.rs:50`); production safety depends on deployment configuration.

**Fix:** Serve the public endpoint over HTTPS, redirect HTTP to HTTPS, and set `Secure` on the cookie. Make the bind address configurable if the app may run without a reverse proxy.

## LOW: Unvalidated dates can break the meeting calendar

`POST /meetings` accepts arbitrary `slot_date[]` text. A failed date parse only changes the displayed label; the original text is stored in `slot_dt` (`src/handlers/meetings.rs:77-109`). The first ten characters are later placed in the meeting page's JavaScript (`templates/meeting.html:63-65`). Malformed values can cause script syntax errors or unusable calendar ranges. The line breaks in the current template prevent the previously suspected backslash-based script-execution example from working.

**Fix:** Reject invalid or out-of-range dates server-side, and serialize dates as JSON for the script.

## LOW: Missing defensive response headers

The router adds no security response headers (`src/lib.rs:60-78`). A Content Security Policy would provide defense against future script injection once the inline scripts are accommodated. `frame-ancestors` can prevent framing, and `X-Content-Type-Options: nosniff` can prevent MIME sniffing. An explicit `Referrer-Policy` can document the intended handling of meeting URLs; modern browsers generally send only the origin, rather than the full meeting URL, on cross-origin HTTPS requests by default.

**Fix:** Add appropriate headers at the app or public reverse proxy and verify that the policy allows required scripts and styles.

## CSRF assessment

The POST routes do not validate `Origin` or use CSRF tokens. This matters if a deployment allows a hostile page on the same *site* (for example, an untrusted sibling subdomain), or if browser cookie behavior changes. However, the edit cookie explicitly uses `SameSite=Lax` (`src/handlers/respond.rs:79`), which generally prevents it from accompanying a cross-site HTML form POST. A cross-site form can still create an unauthenticated response or meeting, but a remote attacker can make those requests directly. The previous claim that an ordinary cross-site form can overwrite a token-holder's row was incorrect.

For stronger protection, validate the `Origin` header on state-changing routes or use CSRF tokens. `SameSite=Strict` can be considered for the edit cookie, but it does not replace input validation or abuse controls.
