use std::{
    io,
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use axum::{
    Json, Router,
    extract::{Query, RawQuery, State},
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::get,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value as JsonValue, json};
use tinytop_collectors::NativeCollector;
use tinytop_store::{
    DashboardSettings, DiskTransition, FreeBytesProvider, HistoryFilesystemSample, HistoryMarker,
    HistoryMarkerType, HistoryPoint, HistoryPointMode, HistoryPointsQuery, HistoryProcessCapture,
    HistoryQuery, HistorySample, SqliteHistoryStore, SysinfoFreeBytes, apply_disk_measurement,
    resolve_history_point_source_with_poll,
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
#[serde(rename_all = "camelCase")]
struct HistoryParams {
    limit: Option<i64>,
    #[serde(alias = "window_seconds")]
    window_seconds: Option<i64>,
    #[serde(alias = "since_ms")]
    since_ms: Option<i64>,
    #[serde(alias = "until_ms")]
    until_ms: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct HistoryPointsParams {
    limit: Option<i64>,
    #[serde(alias = "window_seconds")]
    window_seconds: Option<i64>,
    #[serde(alias = "since_ms")]
    since_ms: Option<i64>,
    #[serde(alias = "until_ms")]
    until_ms: Option<i64>,
    source: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct HistoryMarkersParams {
    limit: Option<i64>,
    #[serde(alias = "window_seconds")]
    window_seconds: Option<i64>,
    #[serde(alias = "since_ms")]
    since_ms: Option<i64>,
    #[serde(alias = "until_ms")]
    until_ms: Option<i64>,
    #[serde(alias = "expected_gap_ms")]
    expected_gap_ms: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct HistoryFilesystemsParams {
    #[serde(alias = "since_ms")]
    since_ms: Option<i64>,
    #[serde(alias = "until_ms")]
    until_ms: Option<i64>,
    mount: Option<String>,
    limit: Option<i64>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
struct HistoryProcessesParams {
    #[serde(alias = "since_ms")]
    since_ms: Option<i64>,
    #[serde(alias = "until_ms")]
    until_ms: Option<i64>,
    limit: Option<i64>,
}

#[derive(Debug, Serialize)]
struct HistoryResponse {
    samples: Vec<HistorySample>,
}

#[derive(Debug, Serialize)]
struct HistoryPointsResponse {
    points: Vec<HistoryPoint>,
    source: &'static str,
    #[serde(rename = "resolutionMs")]
    resolution_ms: i64,
    available: bool,
}

#[derive(Debug, Serialize)]
struct HistoryFilesystemsResponse {
    filesystems: Vec<HistoryFilesystemSample>,
}

#[derive(Debug, Serialize)]
struct HistoryProcessesResponse {
    captures: Vec<HistoryProcessCapture>,
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
    let _cold_export_task = spawn_cold_export_loop(state.clone());
    let _disk_check_task = spawn_disk_check_loop(state.clone());

    let app = router(state);
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

fn router(state: AppState) -> Router {
    Router::new()
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
        .route("/api/history/filesystems", get(history_filesystems))
        .route("/api/history/processes", get(history_processes))
        .route("/api/history", get(history))
        .route("/", get(static_file))
        .route("/embed", get(embed_file))
        .route("/index.html", get(static_file))
        .route("/favicon.svg", get(static_file))
        .route("/styles.css", get(static_file))
        .route("/app.js", get(static_file))
        .route("/vendor/echarts.min.js", get(static_file))
        .route("/ladder-rules.js", get(static_file))
        .with_state(state)
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
    Json(payload): Json<JsonValue>,
) -> Result<Response, ServeError> {
    let previous = state.store.get_settings().await?;
    let settings = DashboardSettings::from_document(payload, Some(&previous.retention_ladder))?;
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
    RawQuery(raw_query): RawQuery,
) -> Result<Response, ServeError> {
    let params = parse_history_points_params(raw_query.as_deref())?;
    let query = history_points_query(params)?;
    let settings = state.store.get_settings().await?;
    let source = resolve_history_point_source_with_poll(
        &settings.retention_ladder,
        settings.poll_interval_ms,
        now_ms()?,
        query,
    );
    let points = state
        .store
        .read_history_points(HistoryPointsQuery { source, ..query })
        .await?;
    Ok(no_store(Json(HistoryPointsResponse {
        points,
        source: source.as_str(),
        resolution_ms: source.resolution_ms(settings.poll_interval_ms),
        available: source != HistoryPointMode::Archive
            || settings.retention_ladder.archive.queryable,
    }))
    .into_response())
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

async fn history_filesystems(
    State(state): State<AppState>,
    RawQuery(raw_query): RawQuery,
) -> Result<Response, ServeError> {
    let params = parse_history_filesystems_params(raw_query.as_deref())?;
    let query = detail_history_query(params.since_ms, params.until_ms, params.limit);
    let filesystems = state
        .store
        .read_history_filesystems(query, params.mount.as_deref())
        .await?;
    Ok(no_store(Json(HistoryFilesystemsResponse { filesystems })).into_response())
}

async fn history_processes(
    State(state): State<AppState>,
    RawQuery(raw_query): RawQuery,
) -> Result<Response, ServeError> {
    let params = parse_history_processes_params(raw_query.as_deref())?;
    let query = detail_history_query(params.since_ms, params.until_ms, params.limit);
    let captures = state.store.read_history_processes(query).await?;
    Ok(no_store(Json(HistoryProcessesResponse { captures })).into_response())
}

async fn static_file(
    State(state): State<AppState>,
    axum::extract::OriginalUri(uri): axum::extract::OriginalUri,
) -> Result<Response, ServeError> {
    let Some(relative_path) = static_relative_path(uri.path()) else {
        return Err(ServeError::not_found("asset not found"));
    };

    let mut response = dashboard_asset_response(&state.dashboard_assets, relative_path)?;
    if is_dashboard_html_path(uri.path()) {
        insert_static_frame_ancestors(&mut response);
    }
    Ok(response)
}

/// The top-level dashboard HTML routes. These get a fixed `frame-ancestors 'self'`
/// CSP so the dashboard cannot be framed by another origin (D1). `/embed` is the
/// deliberate exception: it keeps its operator-configurable ancestors.
fn is_dashboard_html_path(path: &str) -> bool {
    matches!(path, "/" | "/index.html")
}

fn insert_static_frame_ancestors(response: &mut Response) {
    response.headers_mut().insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static("frame-ancestors 'self'"),
    );
}

async fn embed_file(
    State(state): State<AppState>,
    axum::extract::OriginalUri(_uri): axum::extract::OriginalUri,
) -> Result<Response, ServeError> {
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
        Some("ladder-rules.js") => {
            include_bytes!("../../../assets/dashboard/ladder-rules.js").as_slice()
        }
        _ => return Err(ServeError::not_found("embedded asset not found")),
    };

    Ok(bytes_response(bytes, content_type(path)))
}

fn insert_embed_frame_ancestors(response: &mut Response, configured_ancestors: &str) {
    let ancestors = normalized_frame_ancestors(configured_ancestors);
    let policy = format!("frame-ancestors {ancestors}");
    // Fail closed (D2): if the configured value contains bytes that are illegal in
    // an HTTP header (control chars that slipped past the newline/CR check), fall
    // back to a static `'self'` policy rather than omitting the header entirely.
    // `'self'` is always a valid header value, so the CSP is never dropped. This
    // mirrors the Bun runtime, which rejects the same characters up front.
    let value = HeaderValue::from_str(&policy)
        .unwrap_or_else(|_| HeaderValue::from_static("frame-ancestors 'self'"));
    response
        .headers_mut()
        .insert(header::CONTENT_SECURITY_POLICY, value);
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
    let report = match tinytop_store::maintenance::maintain(&state.store, settings, now).await {
        Ok(report) => report,
        Err(error) => {
            eprintln!(
                "history maintenance completed with an error: {error}; partial report: {:?}",
                error.report
            );
            return Ok(());
        }
    };
    if report != tinytop_store::maintenance::MaintenanceReport::default() {
        eprintln!("history maintenance debug: {report:?}");
    }
    if report.pruned.iter().any(|count| *count > 0) || report.expired_l4 > 0 {
        eprintln!(
            "history maintenance info: deleted tier rows {:?}, expired L4 rows {}, archived L4 rows {}",
            report.pruned, report.expired_l4, report.archived_l4
        );
    }
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

fn spawn_disk_check_loop(state: AppState) -> JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            let settings = match state.store.get_settings().await {
                Ok(settings) => settings,
                Err(error) => {
                    eprintln!("disk check skipped: cannot read settings: {error}");
                    tokio::time::sleep(Duration::from_secs(60)).await;
                    continue;
                }
            };
            let dir = match state.store.database_path().parent() {
                Some(parent) if !parent.as_os_str().is_empty() => parent.to_path_buf(),
                _ => PathBuf::from("."),
            };
            let measurement_dir = dir.clone();
            let measurement = match tokio::task::spawn_blocking(move || {
                SysinfoFreeBytes.free_bytes(&measurement_dir)
            })
            .await
            {
                Ok(measurement) => measurement,
                Err(error) => Err(io::Error::other(format!(
                    "disk measurement task failed: {error}"
                ))),
            };
            match now_ms() {
                Ok(now) => {
                    match apply_disk_measurement(
                        &state.store,
                        &dir,
                        measurement,
                        &settings.retention_ladder,
                        now,
                    )
                    .await
                    {
                        Ok(report) if report.transition != DiskTransition::Unchanged => eprintln!(
                            "disk check info: {:?}: free {} vs minFreeBytes {} at {}",
                            report.transition,
                            report.free_bytes,
                            report.min_free_bytes,
                            report.path.display()
                        ),
                        Ok(_) => {}
                        Err(error) => eprintln!("disk check completed with an error: {error}"),
                    }
                }
                Err(error) => eprintln!("disk check completed with an error: {error}"),
            }
            // Keep this defensive clamp aligned with RetentionLadder::validate's 5..=1_440 range;
            // the persisted document may have been edited outside the validated settings API.
            let stored_interval_minutes = settings.retention_ladder.disk_check.interval_minutes;
            let interval_minutes = stored_interval_minutes.clamp(5, 1_440);
            if interval_minutes != stored_interval_minutes {
                eprintln!(
                    "disk check interval out of range: stored {stored_interval_minutes} minutes; using {interval_minutes} minutes (validated range 5..=1440)"
                );
            }
            tokio::time::sleep(Duration::from_secs(interval_minutes as u64 * 60)).await;
        }
    })
}

fn spawn_cold_export_loop(state: AppState) -> JoinHandle<()> {
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(60)).await;
        let mut interval = tokio::time::interval(Duration::from_secs(3_600));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            interval.tick().await;
            let settings = match state.store.get_settings().await {
                Ok(settings) => settings,
                Err(error) => {
                    eprintln!("cold export completed with an error: {error}");
                    continue;
                }
            };
            if !settings.retention_ladder.archive.cold {
                continue;
            }
            let now = match now_ms() {
                Ok(now) => now,
                Err(error) => {
                    eprintln!("cold export completed with an error: {error}");
                    continue;
                }
            };
            let paths = tinytop_store::archive::archive_paths(
                state.store.database_path(),
                &settings.retention_ladder.archive,
            );
            match tinytop_store::archive::export_cold_months(
                &state.store,
                &paths,
                &settings.retention_ladder,
                now,
            )
            .await
            {
                Ok(written) if !written.is_empty() => {
                    let files = written
                        .iter()
                        .map(|row| row.file.as_str())
                        .collect::<Vec<_>>()
                        .join(", ");
                    eprintln!("cold export info: wrote {} file(s): {files}", written.len());
                }
                Ok(_) => {}
                Err(error) => eprintln!("cold export completed with an error: {error}"),
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
    if let Some(limit) = params.limit
        && limit < 1
    {
        return Err(invalid_query(format!(
            "limit must be between 1 and 10000; observed {limit}"
        )));
    }
    if let (Some(since_ms), Some(until_ms)) = (params.since_ms, params.until_ms)
        && since_ms > until_ms
    {
        return Err(invalid_query(format!(
            "sinceMs must be less than or equal to untilMs; observed sinceMs={since_ms}, untilMs={until_ms}"
        )));
    }
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

fn detail_history_query(
    since_ms: Option<i64>,
    until_ms: Option<i64>,
    limit: Option<i64>,
) -> HistoryQuery {
    HistoryQuery {
        since_ms,
        until_ms,
        limit: Some(limit.unwrap_or(DEFAULT_HISTORY_LIMIT).clamp(1, 10_000)),
    }
}

fn parse_history_points_params(raw_query: Option<&str>) -> Result<HistoryPointsParams, ServeError> {
    let pairs = parse_query_pairs(raw_query)?;
    Ok(HistoryPointsParams {
        limit: query_i64(&pairs, "limit", &["limit"])?,
        window_seconds: query_i64(
            &pairs,
            "windowSeconds",
            &["windowSeconds", "window_seconds"],
        )?,
        since_ms: query_i64(&pairs, "sinceMs", &["sinceMs", "since_ms"])?,
        until_ms: query_i64(&pairs, "untilMs", &["untilMs", "until_ms"])?,
        source: query_string(&pairs, "source", &["source"])?,
    })
}

fn parse_history_filesystems_params(
    raw_query: Option<&str>,
) -> Result<HistoryFilesystemsParams, ServeError> {
    let pairs = parse_query_pairs(raw_query)?;
    Ok(HistoryFilesystemsParams {
        since_ms: query_i64(&pairs, "sinceMs", &["sinceMs", "since_ms"])?,
        until_ms: query_i64(&pairs, "untilMs", &["untilMs", "until_ms"])?,
        mount: query_string(&pairs, "mount", &["mount"])?,
        limit: query_i64(&pairs, "limit", &["limit"])?,
    })
}

fn parse_history_processes_params(
    raw_query: Option<&str>,
) -> Result<HistoryProcessesParams, ServeError> {
    let pairs = parse_query_pairs(raw_query)?;
    Ok(HistoryProcessesParams {
        since_ms: query_i64(&pairs, "sinceMs", &["sinceMs", "since_ms"])?,
        until_ms: query_i64(&pairs, "untilMs", &["untilMs", "until_ms"])?,
        limit: query_i64(&pairs, "limit", &["limit"])?,
    })
}

fn parse_query_pairs(raw_query: Option<&str>) -> Result<Vec<(String, String)>, ServeError> {
    raw_query
        .unwrap_or_default()
        .split('&')
        .filter(|pair| !pair.is_empty())
        .map(|pair| {
            let (raw_name, raw_value) = pair.split_once('=').unwrap_or((pair, ""));
            let name = decode_query_component("name", raw_name)?;
            let value = decode_query_component(&name, raw_value)?;
            Ok((name, value))
        })
        .collect()
}

fn query_i64(
    pairs: &[(String, String)],
    field: &str,
    aliases: &[&str],
) -> Result<Option<i64>, ServeError> {
    let Some(value) = query_value(pairs, field, aliases)? else {
        return Ok(None);
    };
    value.parse::<i64>().map(Some).map_err(|_| {
        invalid_query(format!(
            "query parameter {field}: invalid value {value:?}; expected a signed 64-bit integer; provide {field} as an integer"
        ))
    })
}

fn query_string(
    pairs: &[(String, String)],
    field: &str,
    aliases: &[&str],
) -> Result<Option<String>, ServeError> {
    Ok(query_value(pairs, field, aliases)?.map(str::to_string))
}

fn query_value<'a>(
    pairs: &'a [(String, String)],
    field: &str,
    aliases: &[&str],
) -> Result<Option<&'a str>, ServeError> {
    let mut matches = pairs
        .iter()
        .filter(|(name, _)| aliases.contains(&name.as_str()));
    let first = matches.next();
    if let Some((_, observed)) = matches.next() {
        return Err(invalid_query(format!(
            "query parameter {field}: invalid value {observed:?}; the parameter may appear at most once; remove the duplicate {field} value"
        )));
    }
    Ok(first.map(|(_, value)| value.as_str()))
}

fn decode_query_component(field: &str, observed: &str) -> Result<String, ServeError> {
    let input = observed.as_bytes();
    let mut decoded = Vec::with_capacity(input.len());
    let mut index = 0;
    while index < input.len() {
        match input[index] {
            b'+' => {
                decoded.push(b' ');
                index += 1;
            }
            b'%' if index + 2 < input.len() => {
                let Some(high) = hex_value(input[index + 1]) else {
                    return Err(invalid_query(format!(
                        "query parameter {field}: invalid value {observed:?}; percent escapes require two hexadecimal digits; percent-encode {field} correctly"
                    )));
                };
                let Some(low) = hex_value(input[index + 2]) else {
                    return Err(invalid_query(format!(
                        "query parameter {field}: invalid value {observed:?}; percent escapes require two hexadecimal digits; percent-encode {field} correctly"
                    )));
                };
                decoded.push(high * 16 + low);
                index += 3;
            }
            b'%' => {
                return Err(invalid_query(format!(
                    "query parameter {field}: invalid value {observed:?}; percent escapes require two hexadecimal digits; percent-encode {field} correctly"
                )));
            }
            byte => {
                decoded.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8(decoded).map_err(|_| {
        invalid_query(format!(
            "query parameter {field}: invalid value {observed:?}; decoded query values must be UTF-8; percent-encode {field} as UTF-8"
        ))
    })
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn invalid_query(message: String) -> ServeError {
    ServeError::Store(tinytop_store::StoreError::Validation(message))
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
    if previous.retention_ladder != saved.retention_ladder {
        changed.push("retentionLadder");
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
        "/ladder-rules.js" => Some(Path::new("ladder-rules.js")),
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
mod tests {
    use super::*;
    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use http_body_util::BodyExt;
    use std::{collections::VecDeque, io, sync::Mutex as StdMutex};
    use tinytop_store::{DiskTransition, FreeBytesProvider, check_disk};
    use tower::ServiceExt;

    struct ScriptedFreeBytes(StdMutex<VecDeque<Result<u64, io::ErrorKind>>>);

    impl ScriptedFreeBytes {
        fn new(readings: impl IntoIterator<Item = Result<u64, io::ErrorKind>>) -> Self {
            Self(StdMutex::new(readings.into_iter().collect()))
        }
    }

    impl FreeBytesProvider for ScriptedFreeBytes {
        fn free_bytes(&self, _path: &Path) -> io::Result<u64> {
            self.0
                .lock()
                .expect("scripted provider mutex should not be poisoned")
                .pop_front()
                .expect("scripted provider should have a reading")
                .map_err(|kind| io::Error::new(kind, "scripted free-bytes failure"))
        }
    }

    struct TempDatabase {
        directory: PathBuf,
        url: String,
    }

    impl TempDatabase {
        fn new(prefix: &str) -> Self {
            let stamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("test clock should be after the epoch")
                .as_nanos();
            let directory = std::env::temp_dir().join(format!(
                "tinytop-agent-{prefix}-{}-{stamp}",
                std::process::id()
            ));
            assert!(directory.starts_with(std::env::temp_dir()));
            std::fs::create_dir_all(&directory).expect("temp directory should be created");
            let database_path = directory.join("history.sqlite");
            Self {
                directory,
                url: format!("sqlite://{}", database_path.display()),
            }
        }
    }

    impl Drop for TempDatabase {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.directory).ok();
        }
    }

    async fn test_state(prefix: &str) -> (TempDatabase, AppState) {
        let fixture = TempDatabase::new(prefix);
        let store = SqliteHistoryStore::connect(&fixture.url)
            .await
            .expect("fixture store should connect");
        let state = AppState {
            collector: Arc::new(Mutex::new(NativeCollector::default())),
            store,
            dashboard_assets: DashboardAssets::Disabled,
            daemon: DaemonMetadata {
                os: "test".to_string(),
                arch: "test".to_string(),
                install: InstallMetadata {
                    executable: "test".to_string(),
                    working_directory: fixture.directory.display().to_string(),
                },
                bind: BindMetadata {
                    host: "127.0.0.1".to_string(),
                    port: 0,
                },
                storage: StorageMetadata {
                    sqlite_url: fixture.url.clone(),
                    sqlite_path: fixture
                        .directory
                        .join("history.sqlite")
                        .display()
                        .to_string(),
                },
            },
            embed_frame_ancestors: "'self'".to_string(),
        };
        (fixture, state)
    }

    async fn request_json(app: Router, uri: &str) -> (StatusCode, JsonValue) {
        let response = app
            .oneshot(
                Request::builder()
                    .uri(uri)
                    .body(Body::empty())
                    .expect("request should build"),
            )
            .await
            .expect("router request should complete");
        let status = response.status();
        let bytes = response
            .into_body()
            .collect()
            .await
            .expect("response body should collect")
            .to_bytes();
        let body = serde_json::from_slice(&bytes).expect("response should be JSON");
        (status, body)
    }

    async fn insert_fixture_snapshot(store: &SqliteHistoryStore, captured_at_ms: i64) {
        let snapshot = serde_json::from_value(json!({
            "timestamp": format!("fixture-{captured_at_ms}"),
            "identity": {
                "hostname": "test-host",
                "platform": "linux",
                "arch": "x86_64",
                "distro": "test",
                "kernel": "test",
                "runtime": { "kind": "Linux", "confidence": "high", "reason": "fixture" },
                "uptimeSeconds": 60
            },
            "cpu": {
                "usagePercent": 10.0,
                "cores": 4,
                "times": {
                    "user": 0, "nice": 0, "system": 0, "idle": 0, "iowait": 0,
                    "irq": 0, "softirq": 0, "steal": 0, "guest": 0, "guestNice": 0,
                    "total": 0, "idleTotal": 0
                }
            },
            "memory": {
                "totalBytes": 100, "availableBytes": 40, "usedBytes": 60,
                "usedPercent": 60.0
            },
            "swap": {
                "totalBytes": 10, "freeBytes": 5, "usedBytes": 5, "usedPercent": 50.0
            },
            "load": {
                "one": 1.0, "five": 2.0, "fifteen": 3.0, "runnable": 1,
                "totalThreads": 2, "lastPid": 3
            },
            "pressure": { "cpu": {}, "memory": {}, "io": {} },
            "filesystems": [
                {
                    "filesystem": "/dev/root", "type": "ext4", "sizeBytes": 100,
                    "usedBytes": 50, "availableBytes": 50, "usedPercent": 50.0,
                    "mount": "/", "inodeUsedPercent": 10.0, "inodeUsed": 1,
                    "inodeTotal": 10
                },
                {
                    "filesystem": "/dev/data", "type": "xfs", "sizeBytes": 200,
                    "usedBytes": 100, "availableBytes": 100, "usedPercent": 50.0,
                    "mount": "/data", "inodeUsedPercent": null, "inodeUsed": null,
                    "inodeTotal": null
                }
            ],
            "processes": [
                {
                    "pid": 42, "command": "tinytop", "cpuPercent": 1.0,
                    "memoryPercent": 2.0, "rssBytes": 3, "parentPid": null,
                    "startedAt": null
                },
                {
                    "pid": 43, "command": "worker", "cpuPercent": 4.0,
                    "memoryPercent": 5.0, "rssBytes": 6, "parentPid": 42,
                    "startedAt": "2026-08-28T00:00:00Z"
                }
            ]
        }))
        .expect("fixture snapshot JSON should match SystemSnapshot");
        store
            .insert_snapshot(captured_at_ms, &snapshot)
            .await
            .expect("fixture snapshot should insert");
    }

    #[test]
    fn detail_history_query_clamps_limit() {
        assert_eq!(detail_history_query(None, None, Some(0)).limit, Some(1));
        assert_eq!(
            detail_history_query(None, None, Some(99_999)).limit,
            Some(10_000)
        );
        assert_eq!(
            detail_history_query(None, None, None).limit,
            Some(DEFAULT_HISTORY_LIMIT)
        );
    }

    async fn assert_bad_query_names_parameter(
        prefix: &str,
        path: &str,
        parameter: &str,
        observed: &str,
    ) {
        let (_fixture, state) = test_state(prefix).await;
        let (status, body) = request_json(router(state), path).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        let error = body["error"].as_str().unwrap_or_default();
        assert!(error.contains(parameter), "missing {parameter} in {error}");
        assert!(error.contains(observed), "missing {observed} in {error}");
    }

    #[tokio::test]
    async fn history_points_query_rejections_name_parameter_and_observed_value() {
        assert_bad_query_names_parameter(
            "points-bad-limit",
            "/api/history/points?limit=abc",
            "limit",
            "abc",
        )
        .await;
        assert_bad_query_names_parameter(
            "points-bad-since",
            "/api/history/points?sinceMs=x",
            "sinceMs",
            "x",
        )
        .await;
    }

    #[tokio::test]
    async fn history_points_rejects_zero_limit_and_inverted_range() {
        // Break caught: boundary-invalid requests are silently clamped or queried as empty ranges.
        let (_fixture, state) = test_state("points-invalid-boundaries").await;
        let (status, body) =
            request_json(router(state.clone()), "/api/history/points?limit=0").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(
            body,
            json!({ "error": "limit must be between 1 and 10000; observed 0" })
        );

        let (status, body) =
            request_json(router(state), "/api/history/points?sinceMs=200&untilMs=100").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(
            body,
            json!({ "error": "sinceMs must be less than or equal to untilMs; observed sinceMs=200, untilMs=100" })
        );
    }

    #[tokio::test]
    async fn history_filesystems_query_rejections_name_parameter_and_observed_value() {
        assert_bad_query_names_parameter(
            "filesystems-bad-limit",
            "/api/history/filesystems?limit=abc",
            "limit",
            "abc",
        )
        .await;
        assert_bad_query_names_parameter(
            "filesystems-bad-since",
            "/api/history/filesystems?sinceMs=x",
            "sinceMs",
            "x",
        )
        .await;
    }

    #[tokio::test]
    async fn history_processes_query_rejections_name_parameter_and_observed_value() {
        assert_bad_query_names_parameter(
            "processes-bad-limit",
            "/api/history/processes?limit=abc",
            "limit",
            "abc",
        )
        .await;
        assert_bad_query_names_parameter(
            "processes-bad-since",
            "/api/history/processes?sinceMs=x",
            "sinceMs",
            "x",
        )
        .await;
    }

    #[tokio::test]
    async fn auto_picks_finest_tier_that_still_holds_the_range_start() {
        struct Case {
            name: &'static str,
            age_ms: i64,
            limit: i64,
            include_until: bool,
            l3_enabled: bool,
            l4_keep_days: i64,
            archive_queryable: bool,
            expected_source: &'static str,
            expected_resolution_ms: i64,
        }

        let hour = 3_600_000;
        let day = 86_400_000;
        let cases = [
            Case {
                name: "one hour",
                age_ms: hour,
                limit: 10_000,
                include_until: true,
                l3_enabled: true,
                l4_keep_days: 730,
                archive_queryable: false,
                expected_source: "raw",
                expected_resolution_ms: 1_500,
            },
            Case {
                name: "two days",
                age_ms: 2 * day,
                limit: 10_000,
                include_until: true,
                l3_enabled: true,
                l4_keep_days: 730,
                archive_queryable: false,
                expected_source: "rollup",
                expected_resolution_ms: 60_000,
            },
            Case {
                name: "six days",
                age_ms: 6 * day,
                limit: 10_000,
                include_until: true,
                l3_enabled: true,
                l4_keep_days: 730,
                archive_queryable: false,
                expected_source: "rollup",
                expected_resolution_ms: 60_000,
            },
            Case {
                name: "thirty days",
                age_ms: 30 * day,
                limit: 10_000,
                include_until: true,
                l3_enabled: true,
                l4_keep_days: 730,
                archive_queryable: false,
                expected_source: "5m",
                expected_resolution_ms: 300_000,
            },
            Case {
                name: "sixty days",
                age_ms: 60 * day,
                limit: 10_000,
                include_until: true,
                l3_enabled: true,
                l4_keep_days: 730,
                archive_queryable: false,
                expected_source: "1h",
                expected_resolution_ms: 3_600_000,
            },
            Case {
                name: "three hundred days",
                age_ms: 300 * day,
                limit: 10_000,
                include_until: true,
                l3_enabled: true,
                l4_keep_days: 730,
                archive_queryable: false,
                expected_source: "1h",
                expected_resolution_ms: 3_600_000,
            },
            Case {
                name: "thirty days without L3",
                age_ms: 30 * day,
                limit: 10_000,
                include_until: true,
                l3_enabled: false,
                l4_keep_days: 730,
                archive_queryable: false,
                expected_source: "1h",
                expected_resolution_ms: 3_600_000,
            },
            Case {
                name: "thirty days at limit one hundred",
                age_ms: 30 * day,
                limit: 100,
                include_until: true,
                l3_enabled: true,
                l4_keep_days: 730,
                archive_queryable: false,
                expected_source: "1h",
                expected_resolution_ms: 3_600_000,
            },
            Case {
                name: "eight hundred days",
                age_ms: 800 * day,
                limit: 10_000,
                include_until: true,
                l3_enabled: true,
                l4_keep_days: 730,
                archive_queryable: false,
                expected_source: "1h",
                expected_resolution_ms: 3_600_000,
            },
            Case {
                name: "eight hundred days with archive",
                age_ms: 800 * day,
                limit: 10_000,
                include_until: true,
                l3_enabled: true,
                l4_keep_days: 730,
                archive_queryable: true,
                expected_source: "archive",
                expected_resolution_ms: 3_600_000,
            },
            Case {
                name: "one hour without until",
                age_ms: hour,
                limit: 10_000,
                include_until: false,
                l3_enabled: true,
                l4_keep_days: 730,
                archive_queryable: false,
                expected_source: "raw",
                expected_resolution_ms: 1_500,
            },
            Case {
                name: "three thousand days with L4 forever",
                age_ms: 3_000 * day,
                limit: 10_000,
                include_until: true,
                l3_enabled: true,
                l4_keep_days: 0,
                archive_queryable: false,
                expected_source: "1h",
                expected_resolution_ms: 3_600_000,
            },
        ];

        for (index, case) in cases.into_iter().enumerate() {
            let (_fixture, state) = test_state(&format!("auto-{index}")).await;
            let mut settings = state.store.get_settings().await.expect("default settings");
            settings.retention_ladder.l3.enabled = case.l3_enabled;
            settings.retention_ladder.l4.keep_days = case.l4_keep_days;
            settings.retention_ladder.archive.queryable = case.archive_queryable;
            state
                .store
                .put_settings(&settings)
                .await
                .expect("case settings");

            let now = now_ms().expect("test time");
            let mut uri = format!(
                "/api/history/points?source=auto&sinceMs={}&limit={}",
                now.saturating_sub(case.age_ms),
                case.limit
            );
            if case.include_until {
                uri.push_str(&format!("&untilMs={now}"));
            }
            let (status, body) = request_json(router(state), &uri).await;
            assert_eq!(status, StatusCode::OK, "{}: {body}", case.name);
            assert_eq!(body["source"], case.expected_source, "{}", case.name);
            assert_eq!(
                body["resolutionMs"], case.expected_resolution_ms,
                "{}",
                case.name
            );
            assert_eq!(
                body["available"],
                case.expected_source != "archive" || case.archive_queryable,
                "{}",
                case.name
            );
        }
    }

    #[tokio::test]
    async fn queryable_auto_archive_response_is_available_and_contains_points() {
        // Break caught: archive-backed auto pages are returned as unavailable or empty.
        let (_fixture, state) = test_state("queryable-auto-archive").await;
        let mut settings = state.store.get_settings().await.expect("default settings");
        settings.retention_ladder.archive.queryable = true;
        state
            .store
            .put_settings(&settings)
            .await
            .expect("archive settings should persist");
        let stat = tinytop_store::ladder::Stat {
            avg: 10.0,
            min: 10.0,
            max: 10.0,
        };
        state
            .store
            .upsert_tier_bucket(
                tinytop_store::ladder::Tier::L4,
                &tinytop_store::ladder::TierBucket {
                    bucket_start_ms: 0,
                    first_captured_at_ms: 0,
                    newest_captured_at_ms: 3_599_999,
                    sample_count: 60,
                    cpu: stat,
                    memory: stat,
                    swap: stat,
                    load: stat,
                    root_used: Some(stat),
                },
            )
            .await
            .expect("L4 fixture bucket should insert");
        let paths = tinytop_store::archive::archive_paths(
            state.store.database_path(),
            &settings.retention_ladder.archive,
        );
        assert_eq!(
            tinytop_store::archive::move_expired_l4(&state.store, &paths, 7_200_000, 10)
                .await
                .expect("fixture row should archive"),
            1
        );

        let now = now_ms().expect("test time");
        let uri = format!("/api/history/points?source=auto&sinceMs=0&untilMs={now}&limit=10000");
        let (status, body) = request_json(router(state), &uri).await;

        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["source"], "archive");
        assert_eq!(body["resolutionMs"], 3_600_000);
        assert_eq!(body["available"], true);
        assert_eq!(body["points"].as_array().expect("points array").len(), 1);
        assert_eq!(body["points"][0]["source"], "archive");
    }

    #[tokio::test]
    async fn auto_counts_both_inclusive_endpoints_against_the_limit() {
        let (_fixture, state) = test_state("auto-inclusive-endpoints").await;
        let now = now_ms().expect("test time");
        let poll_interval_ms = 1_500;
        let since_ms = now.saturating_sub(poll_interval_ms);
        insert_fixture_snapshot(&state.store, since_ms).await;
        insert_fixture_snapshot(&state.store, now).await;

        let uri =
            format!("/api/history/points?source=auto&sinceMs={since_ms}&untilMs={now}&limit=1");
        let (status, body) = request_json(router(state), &uri).await;

        assert_eq!(status, StatusCode::OK, "{body}");
        assert_ne!(body["source"], "raw");
        assert_eq!(body["source"], "rollup");
    }

    #[tokio::test]
    async fn coverage_reports_every_tier_and_json_horizon() {
        fn sorted_object_keys(value: &JsonValue) -> Vec<&str> {
            let mut keys = value
                .as_object()
                .expect("coverage value should be an object")
                .keys()
                .map(String::as_str)
                .collect::<Vec<_>>();
            keys.sort_unstable();
            keys
        }

        let (_fixture, state) = test_state("coverage").await;
        let (status, body) = request_json(router(state), "/api/history/coverage").await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let tiers = body["tiers"].as_array().expect("tiers should be an array");
        assert_eq!(
            tiers
                .iter()
                .map(|tier| tier["tier"].as_str().expect("tier name"))
                .collect::<Vec<_>>(),
            ["l1", "l2", "l3", "l4"]
        );
        for tier in tiers {
            assert_eq!(
                sorted_object_keys(tier),
                [
                    "bucketCount",
                    "enabled",
                    "keepDays",
                    "newestMs",
                    "oldestMs",
                    "resolutionMs",
                    "tier",
                ]
            );
        }
        for key in [
            "snapshotJsonOldestMs",
            "detailIntervalSec",
            "disk",
            "archive",
            "migration",
        ] {
            assert!(
                body.get(key).is_some(),
                "coverage must contain {key}: {body}"
            );
        }
        assert_eq!(
            sorted_object_keys(&body["disk"]),
            [
                "freeBytes",
                "lastCheckMs",
                "minFreeBytes",
                "pressure",
                "pressureSinceMs",
            ]
        );
        assert_eq!(
            sorted_object_keys(&body["archive"]["queryable"]),
            ["bucketCount", "enabled", "newestMs", "oldestMs", "path"]
        );
        assert_eq!(
            sorted_object_keys(&body["archive"]["cold"]),
            [
                "bytes",
                "directory",
                "enabled",
                "exportedUntilMonth",
                "fileCount",
            ]
        );
        assert!(
            body["migration"].is_null(),
            "fresh database migration must be null: {body}"
        );
    }

    #[tokio::test]
    async fn coverage_reports_pressure_since_after_a_breach() {
        // Break caught: the HTTP coverage shape drops the persisted breach start/check time.
        let (_fixture, state) = test_state("coverage-pressure-since").await;
        let provider = ScriptedFreeBytes::new([Ok(100)]);
        let mut ladder = tinytop_store::retention_ladder::RetentionLadder::default();
        ladder.disk_check.min_free_bytes = 200;
        let now = 1_234_567;
        let report = check_disk(&state.store, &provider, &ladder, now)
            .await
            .expect("scripted breach should succeed");
        assert_eq!(report.transition, DiskTransition::Breached);

        let (status, body) = request_json(router(state), "/api/history/coverage").await;

        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["disk"]["pressure"], true);
        assert!(body["disk"]["pressureSinceMs"].is_number(), "{body}");
        assert_eq!(body["disk"]["pressureSinceMs"], now);
        assert!(body["disk"]["lastCheckMs"].is_number(), "{body}");
        assert_eq!(body["disk"]["lastCheckMs"], now);
    }

    #[tokio::test]
    async fn filesystems_endpoint_filters_by_mount_and_clamps_limit() {
        let (_fixture, state) = test_state("filesystems").await;
        let now = now_ms().expect("test time");
        insert_fixture_snapshot(&state.store, now - 120_000).await;
        insert_fixture_snapshot(&state.store, now - 60_000).await;

        let uri = format!(
            "/api/history/filesystems?sinceMs={}&untilMs={now}&mount=%2Fdata&limit=0",
            now - 180_000
        );
        let (status, body) = request_json(router(state.clone()), &uri).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let filesystems = body["filesystems"].as_array().expect("filesystem rows");
        assert_eq!(filesystems.len(), 1, "limit=0 must clamp to one row");
        assert_eq!(filesystems[0]["mount"], "/data");
        assert_eq!(filesystems[0]["capturedAtMs"], now - 60_000);

        let uri = format!(
            "/api/history/filesystems?sinceMs={}&untilMs={now}&mount=%2Fdata&limit=99999",
            now - 180_000
        );
        let (status, body) = request_json(router(state), &uri).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let filesystems = body["filesystems"].as_array().expect("filesystem rows");
        assert_eq!(filesystems.len(), 2, "large limit must return every row");
        assert!(filesystems.iter().all(|row| row["mount"] == "/data"));
    }

    #[tokio::test]
    async fn processes_endpoint_groups_by_capture_time() {
        let (_fixture, state) = test_state("processes").await;
        let now = now_ms().expect("test time");
        insert_fixture_snapshot(&state.store, now - 120_000).await;
        insert_fixture_snapshot(&state.store, now - 60_000).await;

        let uri = format!(
            "/api/history/processes?sinceMs={}&untilMs={now}&limit=10",
            now - 180_000
        );
        let (status, body) = request_json(router(state.clone()), &uri).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let captures = body["captures"].as_array().expect("process captures");
        assert_eq!(captures.len(), 2);
        assert_eq!(captures[0]["capturedAtMs"], now - 120_000);
        assert_eq!(captures[1]["capturedAtMs"], now - 60_000);
        assert_eq!(captures[0]["processes"].as_array().unwrap().len(), 2);
        assert_eq!(captures[0]["processes"][0]["rank"], 0);
        assert_eq!(captures[0]["processes"][1]["rank"], 1);

        let uri = format!(
            "/api/history/processes?sinceMs={}&untilMs={now}&limit=99999",
            now - 180_000
        );
        let (status, body) = request_json(router(state), &uri).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let captures = body["captures"].as_array().expect("process captures");
        assert_eq!(captures.len(), 2, "large limit must return every capture");
    }

    #[tokio::test]
    async fn raw_history_omits_rows_without_json() {
        let (_fixture, state) = test_state("raw-json").await;
        let now = now_ms().expect("test time");
        let stripped_at = now - 61 * 60_000;
        let recent_at = now - 30 * 60_000;
        insert_fixture_snapshot(&state.store, stripped_at).await;
        insert_fixture_snapshot(&state.store, recent_at).await;
        tinytop_store::maintenance::maintain(&state.store, &DashboardSettings::default(), now)
            .await
            .expect("maintenance should strip old JSON");

        let uri = format!(
            "/api/history?sinceMs={}&untilMs={now}&limit=10",
            now - 2 * 60 * 60_000
        );
        let (status, body) = request_json(router(state), &uri).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let samples = body["samples"].as_array().expect("history samples");
        assert_eq!(samples.len(), 1);
        assert_eq!(samples[0]["capturedAtMs"], recent_at);
    }

    #[test]
    fn static_relative_path_serves_ladder_rules() {
        assert_eq!(
            static_relative_path("/ladder-rules.js"),
            Some(Path::new("ladder-rules.js"))
        );
    }

    fn csp(response: &Response) -> &str {
        response
            .headers()
            .get(header::CONTENT_SECURITY_POLICY)
            .expect("CSP header should be present")
            .to_str()
            .expect("CSP header should be valid ASCII")
    }

    #[test]
    fn embed_frame_ancestors_preserves_a_valid_configuration() {
        let mut response = Response::new(Body::empty());
        insert_embed_frame_ancestors(&mut response, "'self' https://example.com");
        assert_eq!(csp(&response), "frame-ancestors 'self' https://example.com");
    }

    #[test]
    fn embed_frame_ancestors_falls_back_to_self_on_invalid_header_bytes() {
        // A bell control char passes the newline/CR check but is rejected by
        // HeaderValue::from_str — the header must still be set, to `'self'` (D2).
        let mut response = Response::new(Body::empty());
        insert_embed_frame_ancestors(&mut response, "'self'\u{7}evil.example.com");
        assert_eq!(csp(&response), "frame-ancestors 'self'");
    }

    #[test]
    fn embed_frame_ancestors_falls_back_to_self_when_empty() {
        let mut response = Response::new(Body::empty());
        insert_embed_frame_ancestors(&mut response, "   ");
        assert_eq!(csp(&response), "frame-ancestors 'self'");
    }

    #[test]
    fn changed_setting_keys_reports_the_ladder_once_instead_of_derived_aliases() {
        let previous = DashboardSettings::default();
        let mut saved = previous.clone();
        saved.retention_ladder.l1.keep_days = 4;
        saved.retention_hours = 96;

        assert_eq!(
            changed_setting_keys(&previous, &saved),
            vec!["retentionLadder"]
        );
    }
}
