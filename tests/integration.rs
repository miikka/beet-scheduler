// SPDX-FileCopyrightText: 2026 Miikka Koskinen
//
// SPDX-License-Identifier: MIT

use beet_scheduler::{AppState, add_globals, build_app, db};
use minijinja::Environment;
use std::sync::Arc;
use tempfile::NamedTempFile;
use tokio::net::TcpListener;

/// Start the app on a random port and return the base URL.
/// The TempFile is kept alive for the test duration.
async fn spawn_app() -> (String, NamedTempFile) {
    spawn_app_with_snippet("").await
}

async fn spawn_app_with_snippet(snippet: &str) -> (String, NamedTempFile) {
    let tmp = NamedTempFile::new().expect("temp file");
    let db = db::open(tmp.path().to_str().unwrap()).expect("open db");

    let mut env = Environment::new();
    env.set_loader(minijinja::path_loader("templates"));
    add_globals(&mut env, snippet.to_string());

    let state = AppState {
        db,
        env: Arc::new(env),
    };

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base = format!("http://{}", addr);

    tokio::spawn(async move {
        axum::serve(listener, build_app(state)).await.unwrap();
    });

    (base, tmp)
}

/// HTTP client that does NOT follow redirects.
fn no_redirect_client() -> reqwest::Client {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap()
}

// ---------------------------------------------------------------------------
// Helper: create a meeting, return (base_url, meeting_id)
// ---------------------------------------------------------------------------

async fn create_meeting(base: &str, body: &str) -> String {
    let client = no_redirect_client();
    let resp = client
        .post(format!("{}/meetings", base))
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(body.to_string())
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        303,
        "create_meeting got non-303: {}",
        resp.status()
    );
    let location = resp
        .headers()
        .get("location")
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    location.trim_start_matches("/m/").to_string()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_home_returns_200() {
    let (base, _tmp) = spawn_app().await;
    let resp = reqwest::get(format!("{}/", base)).await.unwrap();
    assert_eq!(resp.status(), 200);
    let body = resp.text().await.unwrap();
    assert!(
        body.contains("Schedule a meeting"),
        "home page missing heading"
    );
    assert!(body.contains("calendar-grid"), "home page missing calendar");
}

#[tokio::test]
async fn test_new_slot_row_returns_fragment() {
    let (base, _tmp) = spawn_app().await;
    let resp = reqwest::get(format!("{}/slots/new-row", base))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body = resp.text().await.unwrap();
    assert!(body.contains(r#"class="slot-row""#));
    assert!(body.contains(r#"name="slot_label[]""#));
    assert!(body.contains(r#"name="slot_date[]""#));
    assert!(body.contains(r#"name="slot_time[]""#));
    assert!(body.contains("remove-slot"));
}

#[tokio::test]
async fn test_create_meeting_redirects() {
    let (base, _tmp) = spawn_app().await;
    let id = create_meeting(
        &base,
        "title=Team+Lunch&slot_label[]=Friday+7pm&slot_date[]=2026-02-27&slot_time[]=19:00\
         &slot_label[]=Saturday+2pm&slot_date[]=2026-02-28&slot_time[]=14:00",
    )
    .await;
    assert_eq!(id.len(), 8, "meeting ID should be 8 chars, got: {}", id);
}

#[tokio::test]
async fn test_meeting_page_shows_title_and_labels() {
    let (base, _tmp) = spawn_app().await;
    let id = create_meeting(
        &base,
        "title=My+Test+Meeting\
         &slot_label[]=Mon+9am&slot_date[]=2026-03-02&slot_time[]=09:00\
         &slot_label[]=Tue+3pm&slot_date[]=2026-03-03&slot_time[]=15:00",
    )
    .await;

    let resp = reqwest::get(format!("{}/m/{}", base, id)).await.unwrap();
    assert_eq!(resp.status(), 200);
    let body = resp.text().await.unwrap();
    assert!(
        body.contains("My Test Meeting"),
        "meeting title not in page"
    );
    assert!(body.contains("Mon 9am"), "explicit label not in page");
    assert!(body.contains("Tue 3pm"), "explicit label not in page");
}

#[tokio::test]
async fn test_auto_label_weekday_only() {
    let (base, _tmp) = spawn_app().await;
    // 2026-03-02 is a Monday; no label, no time provided
    let id = create_meeting(
        &base,
        "title=Auto+Label&slot_label[]=&slot_date[]=2026-03-02&slot_time[]=",
    )
    .await;

    let body = reqwest::get(format!("{}/m/{}", base, id))
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(body.contains("Monday"), "auto label should be weekday name");
}

#[tokio::test]
async fn test_auto_label_weekday_with_time() {
    let (base, _tmp) = spawn_app().await;
    // 2026-03-06 is a Friday
    let id = create_meeting(
        &base,
        "title=Auto+Label+Time&slot_label[]=&slot_date[]=2026-03-06&slot_time[]=19:00",
    )
    .await;

    let body = reqwest::get(format!("{}/m/{}", base, id))
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(
        body.contains("Friday 19:00"),
        "auto label should be 'Friday 19:00'"
    );
}

#[tokio::test]
async fn test_time_is_optional() {
    let (base, _tmp) = spawn_app().await;
    // Two slots: one with time, one without
    let id = create_meeting(
        &base,
        "title=Optional+Time\
         &slot_label[]=With+Time&slot_date[]=2026-03-02&slot_time[]=10:00\
         &slot_label[]=No+Time&slot_date[]=2026-03-03&slot_time[]=",
    )
    .await;

    let body = reqwest::get(format!("{}/m/{}", base, id))
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(body.contains("With Time"));
    assert!(body.contains("No Time"));
}

#[tokio::test]
async fn test_unknown_meeting_returns_404() {
    let (base, _tmp) = spawn_app().await;
    let resp = reqwest::get(format!("{}/m/XXXXXXXX", base)).await.unwrap();
    assert_eq!(resp.status(), 404);
}

#[tokio::test]
async fn test_submit_response_redirects() {
    let (base, _tmp) = spawn_app().await;
    let client = no_redirect_client();
    let id = create_meeting(
        &base,
        "title=Lunch&slot_label[]=Mon&slot_date[]=2026-03-02&slot_time[]=12:00\
         &slot_label[]=Tue&slot_date[]=2026-03-03&slot_time[]=12:00",
    )
    .await;

    let resp = client
        .post(format!("{}/m/{}/responses", base, id))
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body("name=Alice")
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 303);
    let redir = resp.headers().get("location").unwrap().to_str().unwrap();
    assert_eq!(redir, format!("/m/{}", id));
}

#[tokio::test]
async fn test_submit_response_with_slots() {
    let (base, _tmp) = spawn_app().await;
    let client = no_redirect_client();
    let id = create_meeting(
        &base,
        "title=Dinner\
         &slot_label[]=Fri&slot_date[]=2026-03-06&slot_time[]=19:00\
         &slot_label[]=Sat&slot_date[]=2026-03-07&slot_time[]=19:00",
    )
    .await;

    let page = reqwest::get(format!("{}/m/{}", base, id))
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    let slot_id = extract_first_slot_id(&page);

    let resp = client
        .post(format!("{}/m/{}/responses", base, id))
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(format!("name=Alice&slot_ids[]={}", slot_id))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 303);

    let page = reqwest::get(format!("{}/m/{}", base, id))
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(
        page.contains("Alice"),
        "Alice should appear in availability grid"
    );
}

#[tokio::test]
async fn test_htmx_response_returns_partial() {
    let (base, _tmp) = spawn_app().await;
    let client = no_redirect_client();
    let id = create_meeting(
        &base,
        "title=HTMX+Test&slot_label[]=Mon&slot_date[]=2026-03-02&slot_time[]=10:00",
    )
    .await;

    let resp = client
        .post(format!("{}/m/{}/responses", base, id))
        .header("Content-Type", "application/x-www-form-urlencoded")
        .header("hx-request", "true")
        .body("name=Bob")
        .send()
        .await
        .unwrap();

    assert_eq!(
        resp.status(),
        200,
        "HTMX response should be 200, not redirect"
    );
    let body = resp.text().await.unwrap();
    assert!(
        body.contains("availability-table"),
        "should return grid partial"
    );
    assert!(body.contains("Bob"), "Bob should be in the grid");
}

#[tokio::test]
async fn test_grid_totals_correct() {
    let (base, _tmp) = spawn_app().await;
    let client = no_redirect_client();
    let id = create_meeting(
        &base,
        "title=Totals+Test\
         &slot_label[]=A&slot_date[]=2026-03-01&slot_time[]=10:00\
         &slot_label[]=B&slot_date[]=2026-03-02&slot_time[]=10:00",
    )
    .await;

    let page = reqwest::get(format!("{}/m/{}", base, id))
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    let slot_ids = extract_all_slot_ids(&page);
    assert_eq!(slot_ids.len(), 2);
    let (s1, s2) = (slot_ids[0], slot_ids[1]);

    // Alice: slot 1 only
    client
        .post(format!("{}/m/{}/responses", base, id))
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(format!("name=Alice&slot_ids[]={}", s1))
        .send()
        .await
        .unwrap();

    // Bob: both slots
    client
        .post(format!("{}/m/{}/responses", base, id))
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(format!("name=Bob&slot_ids[]={}&slot_ids[]={}", s1, s2))
        .send()
        .await
        .unwrap();

    let page = reqwest::get(format!("{}/m/{}", base, id))
        .await
        .unwrap()
        .text()
        .await
        .unwrap();

    assert!(page.contains("Alice"));
    assert!(page.contains("Bob"));

    let counts: Vec<&str> = page
        .split(r#"class="total-count""#)
        .skip(1)
        .map(|s| {
            let inner = s.trim_start_matches('>');
            let end = inner.find('<').unwrap();
            inner[..end].trim()
        })
        .collect();
    assert_eq!(
        counts,
        vec!["2", "1"],
        "slot totals should be [2, 1], got {:?}",
        counts
    );
}

#[tokio::test]
async fn test_meeting_calendar_shows_slot_dates() {
    let (base, _tmp) = spawn_app().await;
    // 2026-03-06 = Friday, 2026-03-09 = Monday
    let id = create_meeting(
        &base,
        "title=Cal+View\
         &slot_label[]=Fri+eve&slot_date[]=2026-03-06&slot_time[]=19:00\
         &slot_label[]=Mon+morn&slot_date[]=2026-03-09&slot_time[]=09:00",
    )
    .await;

    let body = reqwest::get(format!("{}/m/{}", base, id))
        .await
        .unwrap()
        .text()
        .await
        .unwrap();

    // The template injects these ISO dates into the SLOT_DATES Set
    assert!(
        body.contains("\"2026-03-06\""),
        "slot date 2026-03-06 should be in JS"
    );
    assert!(
        body.contains("\"2026-03-09\""),
        "slot date 2026-03-09 should be in JS"
    );
    assert!(
        body.contains("meeting-calendar"),
        "calendar container should be present"
    );
}

#[tokio::test]
async fn test_edit_replaces_availability() {
    let (base, _tmp) = spawn_app().await;
    let id = create_meeting(
        &base,
        "title=Edit+Test\
         &slot_label[]=Slot+A&slot_date[]=2026-03-01&slot_time[]=10:00\
         &slot_label[]=Slot+B&slot_date[]=2026-03-02&slot_time[]=10:00",
    )
    .await;

    let page = reqwest::get(format!("{}/m/{}", base, id))
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    let slot_ids = extract_all_slot_ids(&page);
    let (s1, s2) = (slot_ids[0], slot_ids[1]);

    let client = no_redirect_client();

    // Alice first submits slot A only; capture the edit_tokens cookie
    let resp1 = client
        .post(format!("{}/m/{}/responses", base, id))
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(format!("name=Alice&slot_ids[]={}", s1))
        .send()
        .await
        .unwrap();
    let set_cookie = resp1
        .headers()
        .get("set-cookie")
        .expect("cookie missing after first submit")
        .to_str()
        .unwrap()
        .to_string();
    // Extract "edit_tokens=<value>" portion to send as Cookie header
    let cookie_value = set_cookie.split(';').next().unwrap().trim().to_string();

    // Alice edits: switches to slot B only, replaying her token cookie
    client
        .post(format!("{}/m/{}/responses", base, id))
        .header("Content-Type", "application/x-www-form-urlencoded")
        .header("Cookie", &cookie_value)
        .body(format!("name=Alice&slot_ids[]={}", s2))
        .send()
        .await
        .unwrap();

    let page = reqwest::get(format!("{}/m/{}", base, id))
        .await
        .unwrap()
        .text()
        .await
        .unwrap();

    // Alice should appear exactly once in the grid
    assert_eq!(
        count_participant_rows(&page, "Alice"),
        1,
        "Alice should appear exactly once after editing"
    );

    // Slot A=0, Slot B=1
    let counts: Vec<&str> = page
        .split(r#"class="total-count""#)
        .skip(1)
        .map(|s| {
            let inner = s.trim_start_matches('>');
            inner[..inner.find('<').unwrap()].trim()
        })
        .collect();
    assert_eq!(
        counts,
        vec!["0", "1"],
        "after edit totals should be [0, 1], got {:?}",
        counts
    );
}

#[tokio::test]
async fn test_edit_button_present_in_grid() {
    let (base, _tmp) = spawn_app().await;
    let id = create_meeting(
        &base,
        "title=Edit+Btn&slot_label[]=Mon&slot_date[]=2026-03-02&slot_time[]=",
    )
    .await;

    // Use a cookie-enabled client so Carol gets her edit token
    let carol = reqwest::Client::builder()
        .cookie_store(true)
        .build()
        .unwrap();

    carol
        .post(format!("{}/m/{}/responses", base, id))
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body("name=Carol")
        .send()
        .await
        .unwrap();

    let page = carol
        .get(format!("{}/m/{}", base, id))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(
        page.contains(r#"data-name="Carol""#),
        "Edit button with data-name should be present for Carol"
    );
    assert!(
        page.contains("edit-btn"),
        "edit-btn class should be present"
    );
}

#[tokio::test]
async fn test_edit_button_only_visible_to_token_owner() {
    let (base, _tmp) = spawn_app().await;
    let id = create_meeting(
        &base,
        "title=T&slot_label[]=Mon&slot_date[]=2026-03-02&slot_time[]=",
    )
    .await;

    // Alice's browser (cookie jar enabled)
    let alice = reqwest::Client::builder()
        .cookie_store(true)
        .build()
        .unwrap();

    alice
        .post(format!("{}/m/{}/responses", base, id))
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body("name=Alice")
        .send()
        .await
        .unwrap();

    // Alice visits the page — should see Edit button
    let alice_page = alice
        .get(format!("{}/m/{}", base, id))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(
        alice_page.contains(r#"class="edit-btn""#),
        "Alice should see Edit button for her row"
    );

    // Bob visits (fresh client, no cookies) — should NOT see Edit button
    let bob_page = reqwest::get(format!("{}/m/{}", base, id))
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(
        !bob_page.contains(r#"class="edit-btn""#),
        "Bob should not see any Edit buttons"
    );
}

#[tokio::test]
async fn test_submit_sets_edit_token_cookie() {
    let (base, _tmp) = spawn_app().await;
    let client = no_redirect_client();
    let id = create_meeting(
        &base,
        "title=T&slot_label[]=Mon&slot_date[]=2026-03-02&slot_time[]=",
    )
    .await;

    let resp = client
        .post(format!("{}/m/{}/responses", base, id))
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body("name=Alice")
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 303);
    let cookie = resp
        .headers()
        .get("set-cookie")
        .expect("Set-Cookie header missing")
        .to_str()
        .unwrap()
        .to_string();
    assert!(
        cookie.starts_with("edit_tokens="),
        "cookie should be named edit_tokens, got: {cookie}"
    );
    assert!(
        cookie.contains(&format!("Path=/m/{}", id)),
        "cookie should be path-scoped to the meeting"
    );
    assert!(
        cookie.contains("Max-Age=7776000"),
        "cookie should expire in 90 days"
    );
    assert!(cookie.contains("HttpOnly"), "cookie should be HttpOnly");
    let value = cookie
        .split("edit_tokens=")
        .nth(1)
        .unwrap()
        .split(';')
        .next()
        .unwrap();
    assert!(!value.is_empty(), "token value should be non-empty");
    assert!(
        value.contains('_'),
        "token value should contain pid_token separator"
    );
}

#[tokio::test]
async fn test_token_edit_allows_name_change() {
    let (base, _tmp) = spawn_app().await;
    let id = create_meeting(
        &base,
        "title=T&slot_label[]=Mon&slot_date[]=2026-03-02&slot_time[]=",
    )
    .await;

    // Alice's browser (cookie jar enabled)
    let alice = reqwest::Client::builder()
        .cookie_store(true)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();

    alice
        .post(format!("{}/m/{}/responses", base, id))
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body("name=Alice")
        .send()
        .await
        .unwrap();

    // Read participant_id from Alice's page view
    let page = alice
        .get(format!("{}/m/{}", base, id))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    let pid = extract_participant_id(&page, "Alice");

    // Edit: change name to Alicia
    alice
        .post(format!("{}/m/{}/responses", base, id))
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(format!("name=Alicia&participant_id={}", pid))
        .send()
        .await
        .unwrap();

    let page = alice
        .get(format!("{}/m/{}", base, id))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();

    assert_eq!(
        count_participant_rows(&page, "Alice"),
        0,
        "old name 'Alice' should not appear"
    );
    assert_eq!(
        count_participant_rows(&page, "Alicia"),
        1,
        "new name 'Alicia' should appear exactly once"
    );
}

#[tokio::test]
async fn test_forged_participant_id_creates_duplicate() {
    let (base, _tmp) = spawn_app().await;
    let id = create_meeting(
        &base,
        "title=T&slot_label[]=Mon&slot_date[]=2026-03-02&slot_time[]=",
    )
    .await;

    let client = no_redirect_client();

    // First submission — capture the Set-Cookie to find Alice's pid
    let resp = client
        .post(format!("{}/m/{}/responses", base, id))
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body("name=Alice")
        .send()
        .await
        .unwrap();
    let set_cookie = resp
        .headers()
        .get("set-cookie")
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    // Parse actual pid from cookie value "edit_tokens=<pid>_<token>; ..."
    let value = set_cookie
        .split("edit_tokens=")
        .nth(1)
        .unwrap()
        .split(';')
        .next()
        .unwrap();
    let alice_pid: i64 = value.split('_').next().unwrap().parse().unwrap();

    // Second submission: provide alice_pid but with a WRONG token
    let forged_cookie = format!("edit_tokens={}_{}", alice_pid, "0".repeat(32));
    client
        .post(format!("{}/m/{}/responses", base, id))
        .header("Content-Type", "application/x-www-form-urlencoded")
        .header("Cookie", forged_cookie)
        .body(format!("name=Alice&participant_id={}", alice_pid))
        .send()
        .await
        .unwrap();

    let page = reqwest::get(format!("{}/m/{}", base, id))
        .await
        .unwrap()
        .text()
        .await
        .unwrap();

    assert_eq!(
        count_participant_rows(&page, "Alice"),
        2,
        "forged token should create a duplicate row"
    );
}

#[tokio::test]
async fn test_same_name_with_valid_token_updates_no_duplicate() {
    let (base, _tmp) = spawn_app().await;
    let id = create_meeting(
        &base,
        "title=T\
         &slot_label[]=A&slot_date[]=2026-03-01&slot_time[]=10:00\
         &slot_label[]=B&slot_date[]=2026-03-02&slot_time[]=10:00",
    )
    .await;

    let alice = reqwest::Client::builder()
        .cookie_store(true)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();

    let page = reqwest::get(format!("{}/m/{}", base, id))
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    let slot_ids = extract_all_slot_ids(&page);
    let (s1, s2) = (slot_ids[0], slot_ids[1]);

    // Alice submits: slot A only
    alice
        .post(format!("{}/m/{}/responses", base, id))
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(format!("name=Alice&slot_ids[]={}", s1))
        .send()
        .await
        .unwrap();

    // Alice re-submits same name (cookie present): switches to slot B
    alice
        .post(format!("{}/m/{}/responses", base, id))
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(format!("name=Alice&slot_ids[]={}", s2))
        .send()
        .await
        .unwrap();

    let page = reqwest::get(format!("{}/m/{}", base, id))
        .await
        .unwrap()
        .text()
        .await
        .unwrap();

    // Exactly one Alice row
    assert_eq!(
        count_participant_rows(&page, "Alice"),
        1,
        "should be one Alice row after name-based re-submit with valid token"
    );
    // Totals: slot A = 0, slot B = 1
    let counts: Vec<&str> = page
        .split(r#"class="total-count""#)
        .skip(1)
        .map(|s| {
            let inner = s.trim_start_matches('>');
            inner[..inner.find('<').unwrap()].trim()
        })
        .collect();
    assert_eq!(counts, vec!["0", "1"], "totals should be [0, 1]");
}

#[tokio::test]
async fn test_same_name_without_token_creates_duplicate() {
    let (base, _tmp) = spawn_app().await;
    let id = create_meeting(
        &base,
        "title=T&slot_label[]=Mon&slot_date[]=2026-03-02&slot_time[]=",
    )
    .await;

    let client = no_redirect_client();

    // First submission (no cookie)
    client
        .post(format!("{}/m/{}/responses", base, id))
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body("name=Alice")
        .send()
        .await
        .unwrap();

    // Second submission: same name, no cookie
    client
        .post(format!("{}/m/{}/responses", base, id))
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body("name=Alice")
        .send()
        .await
        .unwrap();

    let page = reqwest::get(format!("{}/m/{}", base, id))
        .await
        .unwrap()
        .text()
        .await
        .unwrap();

    assert_eq!(
        count_participant_rows(&page, "Alice"),
        2,
        "without a token, re-submitting same name creates duplicate"
    );
}

#[tokio::test]
async fn html_snippet_appears_on_every_page() {
    let snippet = r#"<script id="test-snippet">/* analytics */</script>"#;
    let (base, _tmp) = spawn_app_with_snippet(snippet).await;

    let client = reqwest::Client::new();

    let body = client
        .get(format!("{}/", base))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(body.contains(snippet), "snippet missing from home page");

    // Also verify the snippet appears on a meeting page
    let meeting_id = create_meeting(
        &base,
        "title=Test+Meeting&slot_label%5B%5D=Mon&slot_date%5B%5D=2026-04-14&slot_time%5B%5D=09%3A00",
    )
    .await;

    let body = client
        .get(format!("{}/m/{}", base, meeting_id))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(body.contains(snippet), "snippet missing from meeting page");
}

#[tokio::test]
async fn test_robots_txt() {
    let (base, _tmp) = spawn_app().await;
    let resp = reqwest::get(format!("{}/robots.txt", base)).await.unwrap();
    assert_eq!(resp.status(), 200);
    let body = resp.text().await.unwrap();
    assert!(
        body.contains("User-agent: *"),
        "robots.txt missing User-agent"
    );
    assert!(body.contains("Disallow: /"), "robots.txt missing Disallow");
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Count how many participant-name cells in the grid contain `name`.
fn count_participant_rows(page: &str, name: &str) -> usize {
    page.split(r#"class="participant-name""#)
        .skip(1)
        .filter(|cell| cell.split("</td>").next().unwrap_or("").contains(name))
        .count()
}

fn extract_first_slot_id(page: &str) -> i64 {
    let marker = r#"name="slot_ids[]" value=""#;
    let pos = page.find(marker).expect("no slot checkbox found");
    let rest = &page[pos + marker.len()..];
    rest[..rest.find('"').unwrap()].parse().unwrap()
}

fn extract_all_slot_ids(page: &str) -> Vec<i64> {
    let marker = r#"name="slot_ids[]" value=""#;
    let mut ids = Vec::new();
    let mut search = page;
    while let Some(pos) = search.find(marker) {
        let rest = &search[pos + marker.len()..];
        let end = rest.find('"').unwrap();
        ids.push(rest[..end].parse().unwrap());
        search = &rest[end..];
    }
    ids
}

/// Extract `data-participant-id` for the participant with the given name from the Edit button.
fn extract_participant_id(page: &str, name: &str) -> i64 {
    // Looks for data-participant-id="NNN" before data-name="Alice" on the same button
    let needle = format!(r#"data-name="{}""#, name);
    let pos = page
        .find(&needle)
        .unwrap_or_else(|| panic!("name {name} not found in page"));
    let before = &page[..pos];
    let attr = r#"data-participant-id=""#;
    let start = before
        .rfind(attr)
        .unwrap_or_else(|| panic!("data-participant-id not found before {name}"));
    let rest = &before[start + attr.len()..];
    rest[..rest.find('"').unwrap()].parse().unwrap()
}
