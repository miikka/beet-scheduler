# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Commands

- **Run**: `cargo run` (serves on `0.0.0.0:3000`, pass port as first arg: `cargo run 8080`)
- **Test**: `just check` (runs `cargo fmt --check`, `cargo clippy`, and `cargo llvm-cov nextest` with a coverage gate)
- **Single test**: `cargo nextest run test_name`
- **Logging**: `RUST_LOG="beet_scheduler=debug,tower_http=debug"` is the default; override with e.g. `RUST_LOG=trace cargo run`

## Architecture

Rust web app using **Axum** + **SQLite** (rusqlite, bundled) + **minijinja** templates + **HTMX** on the frontend.

**AppState** (`src/lib.rs`) holds `Db` (`Arc<Mutex<Connection>>`) and the minijinja template environment. `build_app()` constructs the full Axum router. `src/main.rs` is minimal: init logging, open DB, serve.

### Routes

```
GET  /                  → home page with calendar widget
GET  /slots/new-row     → HTMX fragment for adding a time slot row
POST /meetings          → create meeting (generates 8-char ID), redirect to /m/{id}
GET  /m/{id}            → meeting detail with availability grid
POST /m/{id}/responses  → submit availability, returns HTMX grid partial or redirect
```

Handlers live in `src/handlers/` with one file per route group. `src/handlers/mod.rs` defines a custom `QsForm<T>` extractor for form parsing.

### Form handling with serde_qs

HTML forms use `name="foo[]"` for array fields. `serde_qs` automatically strips the `[]` suffix, so Rust struct fields are just `foo: Vec<T>` — no `#[serde(rename)]` needed.

### Database

SQLite with WAL mode and foreign keys enabled. Migrations use `PRAGMA user_version` for versioning — new migrations are appended to the `MIGRATIONS` array in `src/db.rs`. DB file is `beet-scheduler.db` in the working directory.

### Templates

minijinja with `path_loader` from the project root. Templates in `templates/`, partials in `templates/partials/`. Jinja2-compatible syntax.

### Testing

Integration tests in `tests/integration.rs` use a `spawn_app()` helper that creates a temp DB and starts the server on a random port. Tests use reqwest (with `rustls-tls`, no OpenSSL dependency).

## Known issues

See `ISSUES.md` for a security audit covering CSRF, input validation, and other concerns.
