//! Loopback HTTP server for live datasites, reports, automation webhooks and the local MCP endpoint.

use std::net::SocketAddr;

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;

use axum::extract::{Path, Request, State};
use axum::http::{header, HeaderMap, HeaderValue, Method, StatusCode};
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

fn is_query_route(path: &str) -> bool {
    path == "/mcp" || path.starts_with("/hooks/") || is_chart_route(path) || ((path.starts_with("/a/") || path.starts_with("/e/")) && path.ends_with("/q"))
}

/// Pages that hand their own query path to a sandboxed iframe, which has an opaque ("null") origin.
fn is_page_query_route(path: &str) -> bool {
    is_chart_route(path) || ((path.starts_with("/a/") || path.starts_with("/e/")) && path.ends_with("/q"))
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
    let mut null_origin = false;
    if is_query_route(&path) {
        if let Some(origin) = &origin {
            let same = origin == &format!("http://127.0.0.1:{port}") || origin == &format!("http://localhost:{port}");
            null_origin = origin == "null" && is_page_query_route(&path);
            if !same && !null_origin {
                return (StatusCode::FORBIDDEN, "Cross-origin requests are not allowed").into_response();
            }
        }
    }
    if null_origin && request.method() == Method::OPTIONS {
        let mut response = StatusCode::NO_CONTENT.into_response();
        let out = response.headers_mut();
        out.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, HeaderValue::from_static("null"));
        out.insert(header::ACCESS_CONTROL_ALLOW_METHODS, HeaderValue::from_static("POST"));
        out.insert(header::ACCESS_CONTROL_ALLOW_HEADERS, HeaderValue::from_static("content-type"));
        out.insert(header::ACCESS_CONTROL_MAX_AGE, HeaderValue::from_static("600"));
        return response;
    }
    let mut response = next.run(request).await;
    if null_origin {
        response.headers_mut().insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, HeaderValue::from_static("null"));
        response.headers_mut().insert(header::VARY, HeaderValue::from_static("Origin"));
    }
    response.headers_mut().insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    response
}

/// Per-hook token bucket: 30 requests a minute.
static HOOK_BUCKETS: Mutex<Option<HashMap<String, (f64, Instant)>>> = Mutex::new(None);
const HOOK_BURST: f64 = 30.0;
const HOOK_REFILL_PER_SEC: f64 = 30.0 / 60.0;

fn hook_allowed(id: &str) -> bool {
    let Ok(mut guard) = HOOK_BUCKETS.lock() else { return false };
    let buckets = guard.get_or_insert_with(HashMap::new);
    let now = Instant::now();
    if buckets.len() > 2000 {
        buckets.clear();
    }
    let entry = buckets.entry(id.to_owned()).or_insert((HOOK_BURST, now));
    entry.0 = (entry.0 + now.duration_since(entry.1).as_secs_f64() * HOOK_REFILL_PER_SEC).min(HOOK_BURST);
    entry.1 = now;
    if entry.0 >= 1.0 {
        entry.0 -= 1.0;
        true
    } else {
        false
    }
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
    html_ok(html::datasite_page(&site, page, SiteMode::Live, &theme.brand, &theme.name))
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
    let Some(token) = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer ").or_else(|| value.strip_prefix("bearer ")))
    else {
        return (StatusCode::UNAUTHORIZED, "Bearer token required").into_response();
    };
    if !hook_allowed(&id) {
        return (StatusCode::TOO_MANY_REQUESTS, "Too many requests for this webhook. Try again in a minute.").into_response();
    }
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
        return (StatusCode::UNAUTHORIZED, "That webhook token is not valid").into_response();
    }
    match crate::commands::automations::run_one(&app, &state, &id, "webhook").await {
        Ok(()) => {
            let _ = app.emit(crate::commands::automations::AUTOMATION_EVENT, serde_json::json!({ "id": id }));
            StatusCode::NO_CONTENT.into_response()
        }
        Err(error) => fail(error),
    }
}

fn html_ok(body: String) -> Response {
    ([(header::CONTENT_TYPE, "text/html; charset=utf-8")], body).into_response()
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
