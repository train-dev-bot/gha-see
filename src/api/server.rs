//! Local Axum server: JSON API + embedded SPA.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::extract::{Path as AxumPath, Query, State};
use axum::http::{header, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use rust_embed::Embed;
use serde::Deserialize;
use tokio::sync::RwLock;
use tokio::task;
use tower_http::cors::{Any, CorsLayer};

use crate::analysis::{
    analyze_path, analyze_yaml_source, refetch_all_remotes, refetch_workflow_remotes, revaluate,
    AnalysisView, AnalyzeError,
};
use crate::eval::EvalContext;

use super::dto::{EvalContextDto, WebView};
use super::fs::{list_directory, FsListResponse};

#[derive(Embed)]
#[folder = "web/dist"]
struct Assets;

pub struct AppState {
    root: RwLock<PathBuf>,
    view: RwLock<AnalysisView>,
    context: RwLock<EvalContext>,
}

impl AppState {
    pub fn from_analysis(root: PathBuf, view: AnalysisView) -> Arc<Self> {
        let context = EvalContext::default_mock_for(&view.workflows);
        Arc::new(Self {
            root: RwLock::new(root),
            view: RwLock::new(view),
            context: RwLock::new(context),
        })
    }
}

pub fn build_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/api/view", get(get_view))
        .route("/api/analyze", post(post_analyze))
        .route("/api/analyze-source", post(post_analyze_source))
        .route("/api/fs", get(get_fs))
        .route("/api/save", post(post_save))
        .route("/api/revaluate", post(post_revaluate))
        .route("/api/fetch", post(post_fetch))
        .route("/api/fetch-all", post(post_fetch_all))
        .route("/", get(index_handler))
        .route("/{*path}", get(static_handler))
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any),
        )
        .with_state(state)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AnalyzeBody {
    path: String,
}

#[derive(Deserialize)]
struct AnalyzeSourceBody {
    name: String,
    source: String,
}

#[derive(Deserialize)]
struct SaveBody {
    path: String,
    content: String,
}

#[derive(Deserialize)]
struct FsQuery {
    path: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FetchBody {
    workflow_index: usize,
}

/// Bind on `127.0.0.1:0`, serve the SPA + API, optionally open a browser.
pub async fn run_server(root: PathBuf, open_browser: bool) -> Result<(), String> {
    // Cold analyze (and any later fetch) uses blocking I/O / reqwest::blocking.
    // Keep that off the async worker threads.
    let root_for_analyze = root.clone();
    let view = task::spawn_blocking(move || analyze_path(&root_for_analyze))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;
    let state = AppState::from_analysis(root, view);
    let app = build_router(state);

    let listener = tokio::net::TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .map_err(|e| e.to_string())?;
    let addr = listener.local_addr().map_err(|e| e.to_string())?;
    let url = format!("http://{addr}/");

    eprintln!("gha-see web UI → {url}");
    if open_browser {
        let open_url = url.clone();
        task::spawn_blocking(move || {
            if let Err(e) = open::that(&open_url) {
                eprintln!("could not open browser: {e} (open {open_url} manually)");
            }
        });
    }

    axum::serve(listener, app).await.map_err(|e| e.to_string())
}

async fn snapshot(state: &AppState) -> WebView {
    let view = state.view.read().await;
    let ctx = state.context.read().await;
    let root = state.root.read().await;
    WebView::from_analysis(&view, &root, &ctx)
}

async fn get_view(State(state): State<Arc<AppState>>) -> Json<WebView> {
    Json(snapshot(&state).await)
}

async fn post_analyze(
    State(state): State<Arc<AppState>>,
    Json(body): Json<AnalyzeBody>,
) -> Result<Json<WebView>, (StatusCode, String)> {
    let path = PathBuf::from(&body.path);
    let path_for_task = path.clone();
    let view = task::spawn_blocking(move || analyze_path(&path_for_task))
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
    let context = EvalContext::default_mock_for(&view.workflows);
    {
        let mut root = state.root.write().await;
        *root = path;
    }
    {
        let mut v = state.view.write().await;
        *v = view;
    }
    {
        let mut c = state.context.write().await;
        *c = context;
    }
    Ok(Json(snapshot(&state).await))
}

async fn post_analyze_source(
    State(state): State<Arc<AppState>>,
    Json(body): Json<AnalyzeSourceBody>,
) -> Result<Json<WebView>, (StatusCode, String)> {
    let view = task::spawn_blocking(move || analyze_yaml_source(&body.name, &body.source))
        .await
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    let context = EvalContext::default_mock_for(&view.workflows);
    // Keep the previous disk root so Browse / Reload still open the folder the
    // user was exploring — scratch analysis must not clobber it with "(scratch)".
    {
        let mut current = state.view.write().await;
        *current = view;
    }
    {
        let mut current = state.context.write().await;
        *current = context;
    }
    Ok(Json(snapshot(&state).await))
}

async fn get_fs(
    State(state): State<Arc<AppState>>,
    Query(query): Query<FsQuery>,
) -> Result<Json<FsListResponse>, (StatusCode, String)> {
    let requested_path = query.path.map(PathBuf::from);
    let root = state.root.read().await.clone();
    let response = task::spawn_blocking(move || {
        let path = requested_path.unwrap_or_else(|| {
            let candidate = if root.is_dir() {
                root
            } else {
                root.parent()
                    .map(Path::to_path_buf)
                    .unwrap_or_else(|| PathBuf::from("."))
            };
            // Never default the browser into a synthetic scratch path.
            let display = candidate.display().to_string();
            if display.starts_with("(scratch)") {
                PathBuf::from(".")
            } else {
                candidate
            }
        });
        list_directory(&path)
    })
    .await
    .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
    .map_err(|error| (StatusCode::BAD_REQUEST, error.to_string()))?;
    Ok(Json(response))
}

async fn post_save(
    State(state): State<Arc<AppState>>,
    Json(body): Json<SaveBody>,
) -> Result<Json<WebView>, (StatusCode, String)> {
    let path = PathBuf::from(body.path);
    let path_for_task = path.clone();
    let view = task::spawn_blocking(move || {
        if let Some(parent) = path_for_task.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path_for_task, body.content)?;
        analyze_path(&path_for_task).map_err(std::io::Error::other)
    })
    .await
    .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
    .map_err(|error| (StatusCode::BAD_REQUEST, error.to_string()))?;
    let context = EvalContext::default_mock_for(&view.workflows);
    {
        let mut root = state.root.write().await;
        *root = path;
    }
    {
        let mut current = state.view.write().await;
        *current = view;
    }
    {
        let mut current = state.context.write().await;
        *current = context;
    }
    Ok(Json(snapshot(&state).await))
}

async fn post_revaluate(
    State(state): State<Arc<AppState>>,
    Json(body): Json<EvalContextDto>,
) -> Result<Json<WebView>, (StatusCode, String)> {
    let ctx: EvalContext = body.into();
    let view = state.view.read().await.clone();
    let ctx_for_task = ctx.clone();
    let next = task::spawn_blocking(move || revaluate(&view, &ctx_for_task))
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    {
        let mut v = state.view.write().await;
        *v = next;
    }
    {
        let mut c = state.context.write().await;
        *c = ctx;
    }
    Ok(Json(snapshot(&state).await))
}

async fn post_fetch(
    State(state): State<Arc<AppState>>,
    Json(body): Json<FetchBody>,
) -> Result<Json<WebView>, (StatusCode, String)> {
    let view = state.view.read().await.clone();
    let workflow_index = body.workflow_index;
    let next = task::spawn_blocking(move || refetch_workflow_remotes(&view, workflow_index))
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .map_err(|e: AnalyzeError| (StatusCode::BAD_REQUEST, e.to_string()))?;
    {
        let mut v = state.view.write().await;
        *v = next;
    }
    Ok(Json(snapshot(&state).await))
}

async fn post_fetch_all(
    State(state): State<Arc<AppState>>,
) -> Result<Json<WebView>, (StatusCode, String)> {
    let view = state.view.read().await.clone();
    let next = task::spawn_blocking(move || refetch_all_remotes(&view))
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .map_err(|e: AnalyzeError| (StatusCode::BAD_REQUEST, e.to_string()))?;
    {
        let mut v = state.view.write().await;
        *v = next;
    }
    Ok(Json(snapshot(&state).await))
}

async fn index_handler() -> Response {
    serve_asset("index.html")
}

async fn static_handler(AxumPath(path): AxumPath<String>) -> Response {
    serve_asset(&path)
}

fn serve_asset(path: &str) -> Response {
    let path = path.trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };

    match Assets::get(path) {
        Some(file) => {
            let mime = mime_guess(path);
            (
                StatusCode::OK,
                [(header::CONTENT_TYPE, mime)],
                file.data.into_owned(),
            )
                .into_response()
        }
        None => match Assets::get("index.html") {
            Some(file) => Html(String::from_utf8_lossy(&file.data).into_owned()).into_response(),
            None => (
                StatusCode::NOT_FOUND,
                "web UI assets missing — run `npm run build` in web/",
            )
                .into_response(),
        },
    }
}

fn mime_guess(path: &str) -> &'static str {
    match Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
    {
        "html" => "text/html; charset=utf-8",
        "js" => "application/javascript",
        "css" => "text/css",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "ico" => "image/x-icon",
        "json" => "application/json",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        "map" => "application/json",
        _ => "application/octet-stream",
    }
}
