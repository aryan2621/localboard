//! Loopback HTTP server for live datasites, reports, automation webhooks and the local MCP endpoint.

use std::net::SocketAddr;

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;

use axum::extract::{Path, Request, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::oneshot;

use crate::commands::artifacts::{refresh_datasite_chart, LOCAL_PORT};
use crate::engine::artifacts::{html, BRAND_HEX, BRAND_NAME};
use crate::engine::artifacts::html::SiteMode;
use crate::error::{blocking, AppError};
use crate::state::{AppState, LocalServerHandle};

pub fn start(app: AppHandle) -> Result<LocalServerHandle, AppError> {
    let addr = SocketAddr::from(([127, 0, 0, 1], LOCAL_PORT));
    let listener = std::net::TcpListener::bind(addr).map_err(|error| {
        AppError::Conflict(format!("LocalBoard's site server needs 127.0.0.1:{LOCAL_PORT}. Something else is using that port. {error}"))
    })?;
    listener.set_nonblocking(true).map_err(|error| AppError::Internal(error.to_string()))?;
    let (tx, rx) = oneshot::channel();
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        let listener = match tokio::net::TcpListener::from_std(listener) {
            Ok(listener) => listener,
            Err(error) => {
                tracing::error!("local site server: {error}");
                return;
            }
        };
        let router = Router::new()
            .route("/d/{id}/data/{chart}", get(chart_data))
            .route("/d/{id}", get(datasite_overview))
            .route("/d/{id}/{slug}", get(datasite_section))
            .route("/r/{id}", get(report_page))
            .route("/hooks/{id}", post(webhook_fire))
            .route("/mcp", post(mcp_rpc))
            .layer(middleware::from_fn(guard))
            .with_state(handle);
        if let Err(error) = axum::serve(listener, router).with_graceful_shutdown(async { rx.await.ok(); }).await {
            tracing::error!("local site server stopped: {error}");
        }
    });
    Ok(LocalServerHandle { port: LOCAL_PORT, shutdown: Some(tx) })
}

/// Header the live pages send on data requests.
pub const SITE_TOKEN_HEADER: &str = "x-lb-site-token";

/// Random per-launch token embedded in live pages. It authenticates chart-data requests, including those from sandboxed (null-origin) previews; other pages or processes cannot guess it.
pub fn site_token() -> &'static str {
    static TOKEN: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    TOKEN.get_or_init(|| format!("{}{}", uuid::Uuid::new_v4().simple(), uuid::Uuid::new_v4().simple()))
}

fn is_query_route(path: &str) -> bool {
    path == "/mcp" || path.starts_with("/hooks/") || is_chart_route(path) || ((path.starts_with("/a/") || path.starts_with("/e/")) && path.ends_with("/q"))
}

fn is_chart_route(path: &str) -> bool {
    path.starts_with("/d/") && path.contains("/data/")
}

/// Blocks DNS rebinding (the Host must be this server) and cross-site requests (a foreign Origin) on every
/// route that runs queries.
async fn guard(request: Request, next: Next) -> Response {
    let headers = request.headers();
    let host = headers.get(header::HOST).and_then(|value| value.to_str().ok()).unwrap_or("").to_ascii_lowercase();
    let port = LOCAL_PORT;
    if host != format!("127.0.0.1:{port}") && host != format!("localhost:{port}") {
        return (StatusCode::FORBIDDEN, "Unexpected Host header").into_response();
    }
    let path = request.uri().path().to_owned();
    let origin = headers.get(header::ORIGIN).and_then(|value| value.to_str().ok()).map(str::to_owned);
    if is_query_route(&path) {
        // Chart data may also come from the sandboxed (opaque "null" origin) in-app preview iframe: the site token authenticates it.
        if let Some(origin) = &origin {
            let same = origin == &format!("http://127.0.0.1:{port}") || origin == &format!("http://localhost:{port}");
            if !same && !(is_chart_route(&path) && origin == "null") {
                return (StatusCode::FORBIDDEN, "Cross-origin requests are not allowed").into_response();
            }
        }
    }
    if is_chart_route(&path) && request.method() == axum::http::Method::OPTIONS {
        // Preflight for the x-lb-site-token header; the real request is still token-checked.
        let mut response = StatusCode::NO_CONTENT.into_response();
        let out = response.headers_mut();
        let allow = origin.as_deref().and_then(|value| HeaderValue::from_str(value).ok()).unwrap_or(HeaderValue::from_static("null"));
        out.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, allow);
        out.insert(header::ACCESS_CONTROL_ALLOW_HEADERS, HeaderValue::from_static(SITE_TOKEN_HEADER));
        out.insert(header::ACCESS_CONTROL_ALLOW_METHODS, HeaderValue::from_static("GET"));
        return response;
    }
    if is_chart_route(&path) {
        let provided = headers.get(SITE_TOKEN_HEADER).and_then(|value| value.to_str().ok()).unwrap_or("");
        if !crate::storage::security::tokens_match(site_token(), provided) {
            return (StatusCode::UNAUTHORIZED, "A site token is required").into_response();
        }
    }
    let mut response = next.run(request).await;
    if is_chart_route(&path) {
        if let Some(origin) = origin.as_deref().and_then(|value| HeaderValue::from_str(value).ok()) {
            response.headers_mut().insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, origin);
        }
    }
    response.headers_mut().insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    response
}

/// Token buckets, 30 a minute per webhook id: one for failed (bad-token) attempts, one for valid calls.
static BAD_BUCKETS: Mutex<Option<HashMap<String, (f64, Instant)>>> = Mutex::new(None);
static GOOD_BUCKETS: Mutex<Option<HashMap<String, (f64, Instant)>>> = Mutex::new(None);
const BAD_BURST: f64 = 30.0;
const GOOD_BURST: f64 = 12.0;
const BUCKET_REFILL_PER_SEC: f64 = 0.5;
const MAX_BUCKETS: usize = 2000;

type Buckets = HashMap<String, (f64, Instant)>;

/// Refills the bucket for `id`. `spend` takes one token if available; returns whether one was (or is) available.
fn bucket_take(buckets: &mut Buckets, id: &str, burst: f64, now: Instant, spend: bool) -> bool {
    if buckets.len() >= MAX_BUCKETS && !buckets.contains_key(id) {
        // Evict full (idle) buckets first, else the oldest one; never wholesale-clear.
        buckets.retain(|_, (tokens, at)| *tokens + now.duration_since(*at).as_secs_f64() * BUCKET_REFILL_PER_SEC < burst);
        if buckets.len() >= MAX_BUCKETS {
            if let Some(oldest) = buckets.iter().min_by_key(|(_, (_, at))| *at).map(|(key, _)| key.clone()) {
                buckets.remove(&oldest);
            }
        }
    }
    let entry = buckets.entry(id.to_owned()).or_insert((burst, now));
    entry.0 = (entry.0 + now.duration_since(entry.1).as_secs_f64() * BUCKET_REFILL_PER_SEC).min(burst);
    entry.1 = now;
    if entry.0 < 1.0 {
        return false;
    }
    if spend {
        entry.0 -= 1.0;
    }
    true
}

fn with_bucket(store: &Mutex<Option<Buckets>>, id: &str, burst: f64, spend: bool) -> bool {
    let Ok(mut guard) = store.lock() else { return false };
    bucket_take(guard.get_or_insert_with(HashMap::new), id, burst, Instant::now(), spend)
}

async fn datasite_overview(State(app): State<AppHandle>, Path(id): Path<String>) -> Response {
    site_page(app, id, None).await
}

async fn datasite_section(State(app): State<AppHandle>, Path((id, slug)): Path<(String, String)>) -> Response {
    site_page(app, id, Some(slug)).await
}

async fn site_page(app: AppHandle, id: String, slug: Option<String>) -> Response {
    let Some(state) = app.try_state::<AppState>() else {
        return (StatusCode::SERVICE_UNAVAILABLE, "LocalBoard is shutting down").into_response();
    };
    let workspace = match state.workspace() {
        Ok(workspace) => workspace,
        Err(error) => return fail(error),
    };
    let loaded = match blocking({
        let workspace = workspace.clone();
        let id = id.clone();
        move || {
            let db = workspace.db()?;
            Ok((db.datasite(&id)?, db.theme().unwrap_or(crate::storage::theme::WorkspaceTheme {
                brand: BRAND_HEX.into(),
                name: BRAND_NAME.into(),
            })))
        }
    })
    .await
    {
        Ok(loaded) => loaded,
        Err(error) => return fail(error),
    };
    let (site, theme) = loaded;
    let page = match slug {
        None => site.pages.iter().find(|page| page.slug == "overview").or_else(|| site.pages.first()),
        Some(slug) => site.pages.iter().find(|page| page.slug == slug),
    };
    let Some(page) = page else {
        return (StatusCode::NOT_FOUND, "Page not found").into_response();
    };
    html_ok(html::datasite_page(&site, page, SiteMode::Live { token: site_token() }, &theme.brand, &theme.name))
}

async fn chart_data(State(app): State<AppHandle>, Path((id, chart)): Path<(String, String)>) -> Response {
    match refresh_datasite_chart(&app, &id, &chart).await {
        Ok(result) => Json(result).into_response(),
        Err(error) => fail(error),
    }
}

async fn report_page(State(app): State<AppHandle>, Path(id): Path<String>) -> Response {
    let Some(state) = app.try_state::<AppState>() else {
        return (StatusCode::SERVICE_UNAVAILABLE, "LocalBoard is shutting down").into_response();
    };
    let workspace = match state.workspace() {
        Ok(workspace) => workspace,
        Err(error) => return fail(error),
    };
    match blocking(move || workspace.db()?.report(&id)).await {
        Ok(report) => html_ok(html::report_html(&report)),
        Err(error) => fail(error),
    }
}

async fn mcp_rpc(State(app): State<AppHandle>, headers: HeaderMap, Json(request): Json<crate::engine::mcp::JsonRpcRequest>) -> Response {
    let Some(state) = app.try_state::<AppState>() else {
        return (StatusCode::SERVICE_UNAVAILABLE, "LocalBoard is shutting down").into_response();
    };
    let provided = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer ").or_else(|| value.strip_prefix("bearer ")))
        .unwrap_or("");
    let expected = match crate::engine::mcp::local_token() {
        Ok(token) => token,
        Err(error) => return fail(error),
    };
    if !crate::storage::security::tokens_match(&expected, provided) {
        return (StatusCode::UNAUTHORIZED, "A bearer token is required. Copy it from LocalBoard Settings.").into_response();
    }
    if let Err(error) = state.ensure_unlocked() {
        return (StatusCode::LOCKED, error.to_string()).into_response();
    }
    drop(state);
    let handle: crate::engine::mcp::ToolHandler = {
        let app = app.clone();
        std::sync::Arc::new(move |name: &str, arguments: serde_json::Value| {
            let state = app.state::<AppState>();
            crate::commands::mcp::run_local_tool(&state, name, arguments)
        })
    };
    Json(crate::engine::mcp::respond(request, &handle)).into_response()
}

async fn webhook_fire(State(app): State<AppHandle>, Path(id): Path<String>, headers: HeaderMap) -> Response {
    // Refuse before any token check once the bad-attempt bucket is dry.
    if !with_bucket(&BAD_BUCKETS, &id, BAD_BURST, false) {
        return (StatusCode::TOO_MANY_REQUESTS, "Too many requests for this webhook. Try again in a minute.").into_response();
    }
    let Some(token) = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer ").or_else(|| value.strip_prefix("bearer ")))
    else {
        return (StatusCode::UNAUTHORIZED, "Bearer token required").into_response();
    };
    let Some(state) = app.try_state::<AppState>() else {
        return (StatusCode::SERVICE_UNAVAILABLE, "LocalBoard is shutting down").into_response();
    };
    let workspace = match state.workspace() {
        Ok(workspace) => workspace,
        Err(error) => return fail(error),
    };
    let allowed = match blocking({
        let workspace = workspace.clone();
        let id = id.clone();
        let token = token.to_owned();
        move || workspace.db()?.webhook_matches(&id, &token)
    })
    .await
    {
        Ok(allowed) => allowed,
        Err(error) => return fail(error),
    };
    if !allowed {
        // Only failed attempts spend the bucket, so a flood of bad tokens cannot starve the real caller.
        with_bucket(&BAD_BUCKETS, &id, BAD_BURST, true);
        return (StatusCode::UNAUTHORIZED, "That webhook token is not valid").into_response();
    }
    if !with_bucket(&GOOD_BUCKETS, &id, GOOD_BURST, true) {
        return (StatusCode::TOO_MANY_REQUESTS, "Too many runs for this webhook. Try again in a minute.").into_response();
    }
    match crate::commands::automations::run_one(&app, &state, &id, "webhook").await {
        Ok(()) => {
            let _ = app.emit(crate::commands::automations::AUTOMATION_EVENT, serde_json::json!({ "id": id }));
            StatusCode::NO_CONTENT.into_response()
        }
        Err(error) => fail(error),
    }
}

/// Pages are self-contained: inline script/style only, data over same-origin fetch, nothing else.
const PAGE_CSP: &str = "default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; img-src data:; connect-src 'self' http://127.0.0.1:17890; base-uri 'none'; form-action 'none'; frame-ancestors 'self' tauri://localhost http://tauri.localhost https://tauri.localhost http://localhost:* http://127.0.0.1:*";

fn html_ok(body: String) -> Response {
    (
        [
            (header::CONTENT_TYPE, "text/html; charset=utf-8"),
            (header::CONTENT_SECURITY_POLICY, PAGE_CSP),
        ],
        body,
    )
        .into_response()
}

fn fail(error: AppError) -> Response {
    let status = match error {
        AppError::NotFound(_) => StatusCode::NOT_FOUND,
        AppError::Invalid(_) => StatusCode::BAD_REQUEST,
        AppError::Conflict(_) => StatusCode::CONFLICT,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    };
    (status, error.to_string()).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chart_routes_need_the_token() {
        assert!(is_chart_route("/d/abc/data/c1"));
        assert!(!is_chart_route("/d/abc"));
        assert!(!site_token().is_empty());
    }

    #[test]
    fn bucket_limits_and_evicts_without_clearing() {
        let mut buckets = Buckets::new();
        let now = Instant::now();
        for _ in 0..3 {
            assert!(bucket_take(&mut buckets, "a", 3.0, now, true));
        }
        assert!(!bucket_take(&mut buckets, "a", 3.0, now, true));
        assert!(!bucket_take(&mut buckets, "a", 3.0, now, false));
        for index in 0..MAX_BUCKETS + 10 {
            bucket_take(&mut buckets, &format!("id{index}"), 3.0, now, true);
        }
        assert!(buckets.len() <= MAX_BUCKETS);
        assert!(buckets.contains_key(&format!("id{}", MAX_BUCKETS + 9)));
    }
}
