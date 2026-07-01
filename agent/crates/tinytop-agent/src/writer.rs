use std::{
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use axum::{
    Json, Router,
    extract::{Query, Request, State},
    http::{HeaderValue, StatusCode, Uri, header},
    middleware::{self, Next},
    response::{IntoResponse, Redirect, Response},
    routing::get,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tinytop_collectors::NativeCollector;
use tinytop_store::{
    DashboardSettings, HistoryMarker, HistoryMarkerType, HistoryPoint, HistoryPointMode,
    HistoryPointsQuery, HistoryQuery, HistorySample, SqliteHistoryStore,
};
use tokio::{net::TcpListener, sync::Mutex, task::JoinHandle};

const DEFAULT_WINDOW_SECONDS: i64 = 300;
const DEFAULT_HISTORY_LIMIT: i64 = 120;

#[derive(Debug, Clone)]
pub struct ServeOptions {
    pub host: String,
    pub port: u16,
    pub sqlite_url: String,
    pub poll_ms: u64,
    pub dashboard_assets: DashboardAssets,
    pub embed_frame_ancestors: String,
    /// Reverse-proxy mount prefix (e.g. "/mon"); empty means root-mounted.
    pub base_path: String,
}

#[derive(Debug, Clone)]
pub enum DashboardAssets {
    Embedded,
    Directory(PathBuf),
    Disabled,
}

#[derive(Clone)]
struct AppState {
    collector: Arc<Mutex<NativeCollector>>,
    store: SqliteHistoryStore,
    dashboard_assets: DashboardAssets,
    daemon: DaemonMetadata,
    embed_frame_ancestors: String,
}

#[derive(Debug, Clone, Copy, Deserialize)]
struct HistoryParams {
    limit: Option<i64>,
    window_seconds: Option<i64>,
    since_ms: Option<i64>,
    until_ms: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
struct HistoryPointsParams {
    limit: Option<i64>,
    window_seconds: Option<i64>,
    since_ms: Option<i64>,
    until_ms: Option<i64>,
    source: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct HistoryMarkersParams {
    limit: Option<i64>,
    window_seconds: Option<i64>,
    since_ms: Option<i64>,
    until_ms: Option<i64>,
    expected_gap_ms: Option<i64>,
}

#[derive(Debug, Serialize)]
struct HistoryResponse {
    samples: Vec<HistorySample>,
}

#[derive(Debug, Serialize)]
struct HistoryPointsResponse {
    points: Vec<HistoryPoint>,
}

#[derive(Debug, Serialize)]
struct HistoryMarkersResponse {
    markers: Vec<HistoryMarker>,
}

#[derive(Debug, Serialize)]
struct VersionResponse {
    status: &'static str,
    app: &'static str,
    version: String,
    runtime: &'static str,
    component: &'static str,
    dashboard: &'static str,
    capabilities: Vec<&'static str>,
    daemon: DaemonMetadata,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct HealthResponse {
    status: &'static str,
    app: &'static str,
    version: String,
    capabilities: Vec<&'static str>,
    daemon: DaemonMetadata,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct DaemonMetadata {
    os: String,
    arch: String,
    install: InstallMetadata,
    bind: BindMetadata,
    storage: StorageMetadata,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct InstallMetadata {
    executable: String,
    working_directory: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct BindMetadata {
    host: String,
    port: u16,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct StorageMetadata {
    sqlite_url: String,
    sqlite_path: String,
}

#[derive(Debug, Serialize)]
struct ErrorResponse {
    error: String,
}

pub async fn serve(options: ServeOptions) -> Result<(), ServeError> {
    let store = SqliteHistoryStore::connect(&options.sqlite_url).await?;
    let state = AppState {
        collector: Arc::new(Mutex::new(NativeCollector::default())),
        store,
        dashboard_assets: options.dashboard_assets.clone(),
        daemon: daemon_metadata(&options),
        embed_frame_ancestors: options.embed_frame_ancestors.clone(),
    };

    collect_and_store(&state).await?;
    state
        .store
        .record_event(
            now_ms()?,
            HistoryMarkerType::DaemonStart,
            "Daemon started",
            json!({
                "runtime": "rust",
                "component": "collector-dashboard-daemon",
                "version": product_version(),
            }),
        )
        .await?;
    let _collection_task = spawn_collection_loop(state.clone(), options.poll_ms);

    let app = router(state, normalize_base_path(&options.base_path));
    let address: SocketAddr = format!("{}:{}", options.host, options.port).parse()?;
    let listener = TcpListener::bind(address).await?;
    let local_address = listener.local_addr()?;

    println!("TinyTop Rust daemon listening on http://{local_address}");
    println!("History database: {}", options.sqlite_url);

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    Ok(())
}

fn router(state: AppState, base_path: String) -> Router {
    let app = Router::new()
        .route("/health", get(health))
        .route("/snapshot/latest", get(latest_snapshot))
        .route("/snapshot/collect", get(collect_snapshot))
        .route("/history", get(history))
        .route("/version", get(version))
        .route("/api/version", get(version))
        .route("/api/settings", get(get_settings).put(update_settings))
        .route("/api/snapshot", get(latest_snapshot))
        .route("/api/history/coverage", get(history_coverage))
        .route("/api/history/points", get(history_points))
        .route("/api/history/markers", get(history_markers))
        .route("/api/history", get(history))
        .route("/", get(static_file))
        .route("/embed", get(embed_file))
        .route("/index.html", get(static_file))
        .route("/favicon.svg", get(static_file))
        .route("/styles.css", get(static_file))
        .route("/app.js", get(static_file))
        .route("/vendor/echarts.min.js", get(static_file))
        .with_state(state);

    if base_path.is_empty() {
        return app;
    }

    // Strip the reverse-proxy prefix *before* routing so a proxy that forwards
    // "/mon/..." unchanged reaches the same handlers as a root-mounted daemon.
    // `Router::layer` runs after route matching, so the rewrite has to happen in
    // an outer router that delegates to `app` as its fallback: the middleware
    // rewrites the URI, then `app` re-routes on the stripped path. Root routes
    // stay live for backwards-compatible deployments.
    Router::new()
        .fallback_service(app)
        .layer(middleware::from_fn_with_state(
            Arc::new(base_path),
            strip_base_path,
        ))
}

/// Normalize a mount prefix into a leading-slash, no-trailing-slash form
/// ("/mon"). An empty or "/" value means the dashboard is root-mounted.
pub fn normalize_base_path(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed == "/" {
        return String::new();
    }
    let with_leading = if trimmed.starts_with('/') {
        trimmed.to_string()
    } else {
        format!("/{trimmed}")
    };
    with_leading.trim_end_matches('/').to_string()
}

enum BasePathAction {
    /// Redirect the bare mount to its trailing-slash form.
    Redirect(String),
    /// Rewrite the request path (prefix stripped) before routing.
    Rewrite(String),
    /// Leave the request untouched (root routes / unrelated paths).
    PassThrough,
}

fn base_path_action(base: &str, path: &str, query: Option<&str>) -> BasePathAction {
    if base.is_empty() {
        return BasePathAction::PassThrough;
    }
    if path == base {
        let mut location = format!("{base}/");
        if let Some(query) = query {
            location.push('?');
            location.push_str(query);
        }
        return BasePathAction::Redirect(location);
    }
    let prefix = format!("{base}/");
    if let Some(rest) = path.strip_prefix(prefix.as_str()) {
        let mut new_path = format!("/{rest}");
        if let Some(query) = query {
            new_path.push('?');
            new_path.push_str(query);
        }
        return BasePathAction::Rewrite(new_path);
    }
    BasePathAction::PassThrough
}

async fn strip_base_path(
    State(base): State<Arc<String>>,
    mut request: Request,
    next: Next,
) -> Response {
    match base_path_action(base.as_str(), request.uri().path(), request.uri().query()) {
        BasePathAction::Redirect(location) => Redirect::permanent(&location).into_response(),
        BasePathAction::Rewrite(new_path) => {
            if let Ok(new_uri) = new_path.parse::<Uri>() {
                *request.uri_mut() = new_uri;
            }
            next.run(request).await
        }
        BasePathAction::PassThrough => next.run(request).await,
    }
}

async fn health(State(state): State<AppState>) -> Response {
    no_store(Json(HealthResponse {
        status: "ok",
        app: "tinytop",
        version: product_version(),
        capabilities: capabilities_for_dashboard(&state.dashboard_assets),
        daemon: state.daemon,
    }))
}

async fn version(State(state): State<AppState>) -> Response {
    no_store(Json(VersionResponse {
        status: "ok",
        app: "tinytop",
        version: product_version(),
        runtime: "rust",
        component: "collector-dashboard-daemon",
        dashboard: dashboard_asset_label(&state.dashboard_assets),
        capabilities: capabilities_for_dashboard(&state.dashboard_assets),
        daemon: state.daemon,
    }))
}

async fn get_settings(State(state): State<AppState>) -> Result<Response, ServeError> {
    Ok(no_store(Json(state.store.get_settings().await?)).into_response())
}

async fn update_settings(
    State(state): State<AppState>,
    Json(settings): Json<DashboardSettings>,
) -> Result<Response, ServeError> {
    let previous = state.store.get_settings().await?;
    let saved = state.store.put_settings(&settings).await?;
    maintain_history(&state, &saved).await?;
    state
        .store
        .record_event(
            now_ms()?,
            HistoryMarkerType::SettingsChange,
            "Settings changed",
            json!({
                "changed": changed_setting_keys(&previous, &saved),
            }),
        )
        .await?;
    Ok(no_store(Json(saved)).into_response())
}

async fn latest_snapshot(State(state): State<AppState>) -> Result<Response, ServeError> {
    let sample = match state.store.latest_snapshot().await? {
        Some(sample) => sample,
        None => collect_and_store(&state).await?,
    };

    Ok(no_store(Json(sample.snapshot)).into_response())
}

async fn collect_snapshot(State(state): State<AppState>) -> Result<Response, ServeError> {
    let sample = collect_and_store(&state).await?;
    Ok(no_store(Json(sample.snapshot)).into_response())
}

async fn history(
    State(state): State<AppState>,
    Query(params): Query<HistoryParams>,
) -> Result<Response, ServeError> {
    let samples = read_history_with_backfill(&state, params).await?;
    Ok(no_store(Json(HistoryResponse { samples })).into_response())
}

async fn history_coverage(State(state): State<AppState>) -> Result<Response, ServeError> {
    let settings = state.store.get_settings().await?;
    let coverage = state.store.history_coverage(&settings).await?;
    Ok(no_store(Json(coverage)).into_response())
}

async fn history_points(
    State(state): State<AppState>,
    Query(params): Query<HistoryPointsParams>,
) -> Result<Response, ServeError> {
    let points = state
        .store
        .read_history_points(history_points_query(params)?)
        .await?;
    Ok(no_store(Json(HistoryPointsResponse { points })).into_response())
}

async fn history_markers(
    State(state): State<AppState>,
    Query(params): Query<HistoryMarkersParams>,
) -> Result<Response, ServeError> {
    let expected_gap_ms = params.expected_gap_ms.unwrap_or(60_000).max(1);
    let markers = state
        .store
        .read_history_markers(history_query(params.into()), expected_gap_ms)
        .await?;
    Ok(no_store(Json(HistoryMarkersResponse { markers })).into_response())
}

async fn static_file(State(state): State<AppState>, uri: Uri) -> Result<Response, ServeError> {
    // Use the (possibly base-path-stripped) request URI rather than OriginalUri
    // so assets resolve correctly when the daemon is mounted under a subpath.
    let Some(relative_path) = static_relative_path(uri.path()) else {
        return Err(ServeError::not_found("asset not found"));
    };

    dashboard_asset_response(&state.dashboard_assets, relative_path)
}

async fn embed_file(State(state): State<AppState>) -> Result<Response, ServeError> {
    let mut response = dashboard_asset_response(&state.dashboard_assets, Path::new("index.html"))?;
    insert_embed_frame_ancestors(&mut response, &state.embed_frame_ancestors);
    Ok(response)
}

fn dashboard_asset_response(
    dashboard_assets: &DashboardAssets,
    relative_path: &Path,
) -> Result<Response, ServeError> {
    match dashboard_assets {
        DashboardAssets::Disabled => Err(ServeError::not_found("dashboard assets are disabled")),
        DashboardAssets::Embedded => embedded_response(relative_path),
        DashboardAssets::Directory(public_dir) => {
            let path = public_dir.join(relative_path);
            let bytes = std::fs::read(&path).map_err(|error| match error.kind() {
                std::io::ErrorKind::NotFound => {
                    ServeError::not_found(format!("{} is missing", path.display()))
                }
                _ => ServeError::Io(error),
            })?;
            Ok(bytes_response(bytes, content_type(relative_path)))
        }
    }
}

fn embedded_response(path: &Path) -> Result<Response, ServeError> {
    let bytes = match path.to_str() {
        Some("index.html") => include_bytes!("../../../assets/dashboard/index.html").as_slice(),
        Some("favicon.svg") => include_bytes!("../../../assets/dashboard/favicon.svg").as_slice(),
        Some("styles.css") => include_bytes!("../../../assets/dashboard/styles.css").as_slice(),
        Some("app.js") => include_bytes!("../../../assets/dashboard/app.js").as_slice(),
        Some("vendor/echarts.min.js") => {
            include_bytes!("../../../assets/dashboard/vendor/echarts.min.js").as_slice()
        }
        _ => return Err(ServeError::not_found("embedded asset not found")),
    };

    Ok(bytes_response(bytes, content_type(path)))
}

fn insert_embed_frame_ancestors(response: &mut Response, configured_ancestors: &str) {
    let ancestors = normalized_frame_ancestors(configured_ancestors);
    let policy = format!("frame-ancestors {ancestors}");
    if let Ok(value) = HeaderValue::from_str(&policy) {
        response
            .headers_mut()
            .insert(header::CONTENT_SECURITY_POLICY, value);
    }
}

fn normalized_frame_ancestors(configured_ancestors: &str) -> &str {
    let trimmed = configured_ancestors.trim();
    if trimmed.is_empty() || trimmed.contains('\n') || trimmed.contains('\r') {
        "'self'"
    } else {
        trimmed
    }
}

fn bytes_response(bytes: impl Into<axum::body::Body>, content_type: &'static str) -> Response {
    let mut response = Response::new(bytes.into());
    response
        .headers_mut()
        .insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

async fn read_history_with_backfill(
    state: &AppState,
    params: HistoryParams,
) -> Result<Vec<HistorySample>, ServeError> {
    let should_backfill = should_backfill_empty_history(&params);
    let mut samples = state.store.read_history(history_query(params)).await?;
    if samples.is_empty() && should_backfill {
        collect_and_store(state).await?;
        samples = state.store.read_history(history_query(params)).await?;
    }
    Ok(samples)
}

fn should_backfill_empty_history(params: &HistoryParams) -> bool {
    params.since_ms.is_none() && params.until_ms.is_none()
}

async fn collect_and_store(state: &AppState) -> Result<HistorySample, ServeError> {
    let snapshot = {
        let mut collector = state.collector.lock().await;
        collector.collect()?
    };
    let sample = state
        .store
        .insert_snapshot(now_ms()?, &snapshot)
        .await
        .map_err(ServeError::from)?;
    let settings = state.store.get_settings().await?;
    maintain_history(state, &settings).await?;
    Ok(sample)
}

async fn maintain_history(
    state: &AppState,
    settings: &DashboardSettings,
) -> Result<(), ServeError> {
    let now = now_ms()?;
    let raw_cutoff = now.saturating_sub(settings.retention_hours.saturating_mul(60 * 60 * 1000));
    let rollup_cutoff = now.saturating_sub(
        settings
            .rollup_retention_days
            .saturating_mul(24 * 60 * 60 * 1000),
    );
    state.store.prune_raw_history(raw_cutoff).await?;
    state.store.prune_rollups(rollup_cutoff).await?;
    Ok(())
}

fn spawn_collection_loop(state: AppState, poll_ms: u64) -> JoinHandle<()> {
    let interval_ms = poll_ms.max(250);
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_millis(interval_ms));
        loop {
            interval.tick().await;
            if let Err(error) = collect_and_store(&state).await {
                eprintln!("scheduled collection failed: {error}");
            }
        }
    })
}

fn history_query(params: HistoryParams) -> HistoryQuery {
    let now = now_ms().unwrap_or_default();
    let window_seconds = params
        .window_seconds
        .unwrap_or(DEFAULT_WINDOW_SECONDS)
        .max(1);
    let since_ms = params
        .since_ms
        .or_else(|| Some(now.saturating_sub(window_seconds.saturating_mul(1000))));

    HistoryQuery {
        since_ms,
        until_ms: params.until_ms,
        limit: Some(
            params
                .limit
                .unwrap_or(DEFAULT_HISTORY_LIMIT)
                .clamp(1, 10_000),
        ),
    }
}

fn history_points_query(params: HistoryPointsParams) -> Result<HistoryPointsQuery, ServeError> {
    let now = now_ms().unwrap_or_default();
    let window_seconds = params
        .window_seconds
        .unwrap_or(DEFAULT_WINDOW_SECONDS)
        .max(1);
    let since_ms = params
        .since_ms
        .or_else(|| Some(now.saturating_sub(window_seconds.saturating_mul(1000))));
    let source = params
        .source
        .as_deref()
        .unwrap_or("auto")
        .parse::<HistoryPointMode>()?;

    Ok(HistoryPointsQuery {
        since_ms,
        until_ms: params.until_ms,
        limit: Some(
            params
                .limit
                .unwrap_or(DEFAULT_HISTORY_LIMIT)
                .clamp(1, 10_000),
        ),
        source,
    })
}

impl From<HistoryMarkersParams> for HistoryParams {
    fn from(params: HistoryMarkersParams) -> Self {
        Self {
            limit: params.limit,
            window_seconds: params.window_seconds,
            since_ms: params.since_ms,
            until_ms: params.until_ms,
        }
    }
}

fn changed_setting_keys(
    previous: &DashboardSettings,
    saved: &DashboardSettings,
) -> Vec<&'static str> {
    let mut changed = Vec::new();
    if previous.default_theme != saved.default_theme {
        changed.push("defaultTheme");
    }
    if previous.default_graph_mode != saved.default_graph_mode {
        changed.push("defaultGraphMode");
    }
    if previous.poll_interval_ms != saved.poll_interval_ms {
        changed.push("pollIntervalMs");
    }
    if previous.default_history_window != saved.default_history_window {
        changed.push("defaultHistoryWindow");
    }
    if previous.retention_hours != saved.retention_hours {
        changed.push("retentionHours");
    }
    if previous.rollup_retention_days != saved.rollup_retention_days {
        changed.push("rollupRetentionDays");
    }
    if previous.target_database_bytes != saved.target_database_bytes {
        changed.push("targetDatabaseBytes");
    }
    if previous.top_process_count != saved.top_process_count {
        changed.push("topProcessCount");
    }
    if previous.redaction_default != saved.redaction_default {
        changed.push("redactionDefault");
    }
    if previous.thresholds != saved.thresholds {
        changed.push("thresholds");
    }
    if previous.enabled_sections != saved.enabled_sections {
        changed.push("enabledSections");
    }
    changed
}

fn static_relative_path(path: &str) -> Option<&'static Path> {
    match path {
        "/" | "/index.html" => Some(Path::new("index.html")),
        "/favicon.svg" => Some(Path::new("favicon.svg")),
        "/styles.css" => Some(Path::new("styles.css")),
        "/app.js" => Some(Path::new("app.js")),
        "/vendor/echarts.min.js" => Some(Path::new("vendor/echarts.min.js")),
        _ => None,
    }
}

fn content_type(path: &Path) -> &'static str {
    match path.extension().and_then(|extension| extension.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("js") => "text/javascript; charset=utf-8",
        Some("svg") => "image/svg+xml; charset=utf-8",
        _ => "application/octet-stream",
    }
}

fn product_version() -> String {
    include_str!("../../../../VERSION").trim().to_string()
}

fn dashboard_asset_label(assets: &DashboardAssets) -> &'static str {
    match assets {
        DashboardAssets::Embedded => "embedded",
        DashboardAssets::Directory(_) => "directory",
        DashboardAssets::Disabled => "disabled",
    }
}

fn capabilities_for_dashboard(assets: &DashboardAssets) -> Vec<&'static str> {
    let mut capabilities = vec!["snapshot", "history"];
    if !matches!(assets, DashboardAssets::Disabled) {
        capabilities.push("embed");
    }
    capabilities
}

fn daemon_metadata(options: &ServeOptions) -> DaemonMetadata {
    DaemonMetadata {
        os: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
        install: InstallMetadata {
            executable: path_or_unavailable(std::env::current_exe()),
            working_directory: path_or_unavailable(std::env::current_dir()),
        },
        bind: BindMetadata {
            host: options.host.clone(),
            port: options.port,
        },
        storage: StorageMetadata {
            sqlite_url: options.sqlite_url.clone(),
            sqlite_path: sqlite_path_label(&options.sqlite_url),
        },
    }
}

fn path_or_unavailable(path: Result<PathBuf, std::io::Error>) -> String {
    path.map(|path| path.display().to_string())
        .unwrap_or_else(|error| format!("unavailable: {error}"))
}

fn sqlite_path_label(sqlite_url: &str) -> String {
    if sqlite_url == "sqlite::memory:" || sqlite_url == ":memory:" {
        return "memory".to_string();
    }

    if let Some(path) = sqlite_url.strip_prefix("sqlite://") {
        return if path == ":memory:" {
            "memory".to_string()
        } else {
            path.to_string()
        };
    }

    if let Some(path) = sqlite_url.strip_prefix("sqlite:") {
        return if path == ":memory:" {
            "memory".to_string()
        } else {
            path.to_string()
        };
    }

    sqlite_url.to_string()
}

fn no_store<T: IntoResponse>(response: T) -> Response {
    let mut response = response.into_response();
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

fn now_ms() -> Result<i64, ServeError> {
    let duration = SystemTime::now().duration_since(UNIX_EPOCH)?;
    i64::try_from(duration.as_millis()).map_err(|_| ServeError::TimeOverflow)
}

async fn shutdown_signal() {
    if let Err(error) = tokio::signal::ctrl_c().await {
        eprintln!("failed to listen for shutdown signal: {error}");
    }
}

impl Default for HistoryParams {
    fn default() -> Self {
        Self {
            limit: Some(DEFAULT_HISTORY_LIMIT),
            window_seconds: Some(DEFAULT_WINDOW_SECONDS),
            since_ms: None,
            until_ms: None,
        }
    }
}

#[derive(Debug)]
pub enum ServeError {
    Collector(tinytop_collectors::CollectorError),
    Store(tinytop_store::StoreError),
    Io(std::io::Error),
    AddrParse(std::net::AddrParseError),
    Time(std::time::SystemTimeError),
    TimeOverflow,
    NotFound(String),
}

impl ServeError {
    fn not_found(message: impl Into<String>) -> Self {
        Self::NotFound(message.into())
    }

    fn status_code(&self) -> StatusCode {
        match self {
            Self::NotFound(_) => StatusCode::NOT_FOUND,
            Self::Store(tinytop_store::StoreError::Validation(_)) => StatusCode::BAD_REQUEST,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

impl std::fmt::Display for ServeError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Collector(error) => write!(formatter, "{error}"),
            Self::Store(error) => write!(formatter, "{error}"),
            Self::Io(error) => write!(formatter, "{error}"),
            Self::AddrParse(error) => write!(formatter, "{error}"),
            Self::Time(error) => write!(formatter, "{error}"),
            Self::TimeOverflow => write!(formatter, "current time does not fit in milliseconds"),
            Self::NotFound(message) => write!(formatter, "{message}"),
        }
    }
}

impl std::error::Error for ServeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Collector(error) => Some(error),
            Self::Store(error) => Some(error),
            Self::Io(error) => Some(error),
            Self::AddrParse(error) => Some(error),
            Self::Time(error) => Some(error),
            Self::TimeOverflow | Self::NotFound(_) => None,
        }
    }
}

impl IntoResponse for ServeError {
    fn into_response(self) -> Response {
        let status = self.status_code();
        let body = Json(ErrorResponse {
            error: self.to_string(),
        });
        (status, body).into_response()
    }
}

impl From<tinytop_collectors::CollectorError> for ServeError {
    fn from(error: tinytop_collectors::CollectorError) -> Self {
        Self::Collector(error)
    }
}

impl From<tinytop_store::StoreError> for ServeError {
    fn from(error: tinytop_store::StoreError) -> Self {
        Self::Store(error)
    }
}

impl From<std::io::Error> for ServeError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<std::net::AddrParseError> for ServeError {
    fn from(error: std::net::AddrParseError) -> Self {
        Self::AddrParse(error)
    }
}

impl From<std::time::SystemTimeError> for ServeError {
    fn from(error: std::time::SystemTimeError) -> Self {
        Self::Time(error)
    }
}

#[cfg(test)]
mod base_path_tests {
    use super::{BasePathAction, base_path_action, normalize_base_path};

    #[test]
    fn normalizes_mount_prefix_variants() {
        assert_eq!(normalize_base_path(""), "");
        assert_eq!(normalize_base_path("/"), "");
        assert_eq!(normalize_base_path("  "), "");
        assert_eq!(normalize_base_path("mon"), "/mon");
        assert_eq!(normalize_base_path("/mon"), "/mon");
        assert_eq!(normalize_base_path("/mon/"), "/mon");
        assert_eq!(normalize_base_path("/mon/status/"), "/mon/status");
    }

    #[test]
    fn passes_through_when_root_mounted() {
        assert!(matches!(
            base_path_action("", "/api/version", None),
            BasePathAction::PassThrough
        ));
    }

    #[test]
    fn redirects_bare_mount_to_trailing_slash() {
        match base_path_action("/mon", "/mon", None) {
            BasePathAction::Redirect(location) => assert_eq!(location, "/mon/"),
            _ => panic!("expected redirect"),
        }
        match base_path_action("/mon", "/mon", Some("theme=dark")) {
            BasePathAction::Redirect(location) => assert_eq!(location, "/mon/?theme=dark"),
            _ => panic!("expected redirect with query"),
        }
    }

    #[test]
    fn rewrites_prefixed_paths() {
        match base_path_action("/mon", "/mon/", None) {
            BasePathAction::Rewrite(path) => assert_eq!(path, "/"),
            _ => panic!("expected rewrite of mount root"),
        }
        match base_path_action("/mon", "/mon/app.js", None) {
            BasePathAction::Rewrite(path) => assert_eq!(path, "/app.js"),
            _ => panic!("expected asset rewrite"),
        }
        match base_path_action("/mon", "/mon/api/history", Some("limit=20")) {
            BasePathAction::Rewrite(path) => assert_eq!(path, "/api/history?limit=20"),
            _ => panic!("expected api rewrite with query"),
        }
    }

    #[test]
    fn leaves_unrelated_and_root_paths_untouched() {
        assert!(matches!(
            base_path_action("/mon", "/api/version", None),
            BasePathAction::PassThrough
        ));
        // A sibling path that merely shares the prefix string is not stripped.
        assert!(matches!(
            base_path_action("/mon", "/monitoring", None),
            BasePathAction::PassThrough
        ));
    }
}
