<!--
SPDX-FileCopyrightText: 2026 Miikka Koskinen

SPDX-License-Identifier: MIT
-->

# Security Issues

Found during a penetration test of the local dev instance.

---

## MEDIUM: No CSRF protection (severity elevated by edit token feature)

Both `POST /meetings` and `POST /m/{id}/responses` accept requests from any origin without a CSRF token. An attacker who can get a meeting participant to visit a malicious page can forge requests on their behalf — silently overwriting their availability or creating spam meetings.

With the edit token feature in place, the severity of this issue has increased: `SameSite=Lax` blocks cross-site `fetch`/XHR but not a regular HTML form POST. A third-party page can therefore forge an authenticated edit (overwriting a token-holder's response) without needing to know the token.

**Reproduce:**
```
curl -X POST http://localhost:3000/m/<id>/responses \
  -H "Origin: http://evil.example.com" \
  --data-urlencode "name=Alice" \
  --data-urlencode "slot_ids[]=1"
```

**Fix:** Add CSRF tokens to all state-mutating forms, or upgrade the edit-token cookie to `SameSite=Strict`.

---

## MEDIUM: Edit token cookie missing `Secure` flag

The `Set-Cookie` header for the edit token does not include the `Secure` attribute (`src/handlers/respond.rs`). A passive network observer on the same network can capture the token from plain HTTP traffic. The token is the only authentication credential in the system.

**Fix:** Add `; Secure` to the `Set-Cookie` format string in `build_set_cookie_header`. This requires TLS termination (reverse proxy or direct TLS) in production.

---


## LOW: Cross-meeting slot_id injection

`submit_response` inserts into `availabilities` without verifying that the submitted `slot_ids` belong to the target meeting (`src/handlers/respond.rs:87-92`). An attacker submitting to Meeting B can include slot IDs from Meeting A. The orphaned rows are never surfaced in any grid view, so there is no information disclosure, but unconstrained rows accumulate in the DB.

**Fix:** Validate that each incoming `slot_id` belongs to the current meeting before inserting:
```sql
SELECT id FROM time_slots WHERE id = ?1 AND meeting_id = ?2
```

---

## LOW: No server-side input length limits

Meeting titles, slot labels, participant names, and slot dates have no maximum length enforced server-side. A 100 000-character title is accepted (HTTP 303). This allows DB bloat and potential memory pressure from large payloads. The `required` / `type="date"` constraints on form inputs are client-side only and trivially bypassed with curl.

**Fix:** Reject requests where any string field exceeds a reasonable limit (e.g. 500 characters for titles/names, 20 characters for dates/times) and return HTTP 422.

---

## LOW: Blank participant names accepted

A name consisting entirely of whitespace is trimmed to an empty string and stored successfully, creating a nameless participant row. The `required` attribute on the `<input>` only enforces this in the browser.

**Reproduce:**
```
curl -X POST http://localhost:3000/m/<id>/responses --data-urlencode "name=   "
```

**Fix:** After trimming, reject (HTTP 422) if `name` is empty.

---

## LOW: No security response headers

None of the standard defensive headers are set on any response:

| Header | Risk |
|---|---|
| `Content-Security-Policy` | No XSS mitigation; HTMX and inline `<script>` blocks are present |
| `X-Frame-Options` / `frame-ancestors 'none'` | Meeting pages can be embedded in iframes (clickjacking) |
| `X-Content-Type-Options: nosniff` | MIME-type sniffing by browsers |
| `Referrer-Policy: same-origin` | Meeting IDs leak to third-party resources via `Referer` |

**Fix:** Add a `tower_http::set_header` or `tower_http::sensitive_headers` layer (or a custom middleware) that appends these headers to every response.

---

## LOW: Server binds on 0.0.0.0

`src/main.rs` binds to `0.0.0.0`, exposing the service on all network interfaces. In development this is usually harmless, but if deployed directly it serves unencrypted HTTP to the network with no reverse proxy in front.

**Fix:** Default the bind address to `127.0.0.1` and require an explicit flag or environment variable to expose it more broadly.
