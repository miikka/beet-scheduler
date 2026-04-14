use beet_scheduler::{add_globals, build_app, db, AppState};
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
    let client = no_redirect_client();
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

    // Alice first submits slot A only
    client
        .post(format!("{}/m/{}/responses", base, id))
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(format!("name=Alice&slot_ids[]={}", s1))
        .send()
        .await
        .unwrap();

    // Alice edits: switches to slot B only
    client
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

    // Alice should appear exactly once in the grid
    let alice_rows = page.matches(r#"data-name="Alice""#).count();
    assert_eq!(
        alice_rows, 1,
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
    let client = no_redirect_client();
    let id = create_meeting(
        &base,
        "title=Edit+Btn&slot_label[]=Mon&slot_date[]=2026-03-02&slot_time[]=",
    )
    .await;

    client
        .post(format!("{}/m/{}/responses", base, id))
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body("name=Carol")
        .send()
        .await
        .unwrap();

    let page = reqwest::get(format!("{}/m/{}", base, id))
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(
        page.contains(r#"data-name="Carol""#),
        "Edit button with data-name should be present"
    );
    assert!(
        page.contains("edit-btn"),
        "edit-btn class should be present"
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

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

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
