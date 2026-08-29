use std::{
    io::Write,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::Serialize;
use tinytop_collectors::NativeCollector;
use tinytop_store::{
    HistoryArchiveCoverage, HistoryColdArchiveCoverage, HistoryDiskCoverage,
    HistoryQueryableArchiveCoverage, HistoryTierCoverage, SqliteHistoryStore, StoreStats,
    archive::{
        ArchiveManifestRow, ArchiveSchemaState, MAX_COLD_MONTHS_PER_PASS, archive_months_present,
        archive_paths, archive_schema_state, export_cold_months, exportable_months,
        months_ready_to_export, read_archive_manifest,
    },
    database_path_from_url, inspect_database_path, pre_image_path,
    settings_transfer::{apply_import, export_document, import_marker, plan_import},
};

mod otel;
mod writer;

const DEFAULT_HOST: &str = "127.0.0.1";
const DEFAULT_DASHBOARD_PORT: u16 = 4274;
const DEFAULT_WINDOWS_DASHBOARD_PORT: u16 = 4275;
const DEFAULT_WRITER_PORT: u16 = 4276;
const DEFAULT_POLL_MS: u64 = 1500;

#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        if let Some(refusal) = error.downcast_ref::<Refused>() {
            let mut response = serde_json::json!({
                "status": "refused",
                "reason": refusal.reason,
            });
            if let Some(details) = &refusal.details {
                response["details"] = details.clone();
            }
            println!("{response}");
        } else if error.downcast_ref::<OutputAlreadyReported>().is_some() {
            // The command already emitted its structured failure response.
        } else {
            eprintln!("{error}");
        }
        std::process::exit(1);
    }
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let command = args.first().map(String::as_str).unwrap_or("collect");

    match command {
        "collect" => collect(args.get(1..).unwrap_or(&[])).await,
        "db" => db(args.get(1..).unwrap_or(&[])).await,
        "config" => config(args.get(1..).unwrap_or(&[])).await,
        "serve" => serve(args.get(1..).unwrap_or(&[]), ServeDefaults::dashboard()).await,
        "serve-writer" => serve(args.get(1..).unwrap_or(&[]), ServeDefaults::writer()).await,
        "help" | "--help" | "-h" => {
            print_help();
            Ok(())
        }
        unknown => Err(format!("unknown command: {unknown}").into()),
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DbStatus<T: Serialize> {
    status: &'static str,
    value: T,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DbStatsOutput {
    #[serde(flatten)]
    stats: StoreStats,
    user_version: i64,
    tiers: Vec<HistoryTierCoverage>,
    snapshot_json_sample_count: i64,
    archive: HistoryArchiveCoverage,
    disk: HistoryDiskCoverage,
    otel: OtelStatsOutput,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct OtelStatsOutput {
    enabled: bool,
    endpoint: String,
    interval_sec: i64,
    headers_env_var: String,
    headers_env_var_set: bool,
    service_name: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct IntegrityResult {
    result: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct VacuumResult {
    action: &'static str,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PreImageStatus {
    path: String,
    exists: bool,
    bytes: Option<u64>,
    database_exists: bool,
    user_version: Option<i64>,
    integrity_check: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PreImageRemoveResult {
    action: &'static str,
    path: String,
    bytes: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ArchiveStatusOutput {
    queryable: HistoryQueryableArchiveCoverage,
    cold: ArchiveColdStatus,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ArchiveColdStatus {
    #[serde(flatten)]
    coverage: HistoryColdArchiveCoverage,
    manifest: Vec<ArchiveManifestRow>,
    #[serde(skip_serializing_if = "Option::is_none")]
    next_exportable_months: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ArchiveExportOutput {
    action: &'static str,
    written: Vec<ArchiveManifestRow>,
}

#[derive(Debug, Serialize)]
struct Refused {
    reason: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    details: Option<serde_json::Value>,
}

impl std::fmt::Display for Refused {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.reason)
    }
}

impl std::error::Error for Refused {}

#[derive(Debug)]
struct OutputAlreadyReported;

impl std::fmt::Display for OutputAlreadyReported {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("structured command failure already reported")
    }
}

impl std::error::Error for OutputAlreadyReported {}

#[derive(Debug, Clone)]
struct ServeDefaults {
    host_env: &'static str,
    port_env: &'static str,
    default_port: u16,
    include_dashboard: bool,
}

impl ServeDefaults {
    fn dashboard() -> Self {
        Self {
            host_env: "HOST",
            port_env: "PORT",
            default_port: default_dashboard_port(),
            include_dashboard: true,
        }
    }

    fn writer() -> Self {
        Self {
            host_env: "HISTORY_WRITER_HOST",
            port_env: "HISTORY_WRITER_PORT",
            default_port: DEFAULT_WRITER_PORT,
            include_dashboard: false,
        }
    }
}

async fn collect(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let mut sqlite_url = None;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--sqlite" => {
                let Some(value) = args.get(index + 1) else {
                    return Err("--sqlite requires a database URL".into());
                };
                sqlite_url = Some(value.clone());
                index += 2;
            }
            "--json" => {
                index += 1;
            }
            other => return Err(format!("unknown collect option: {other}").into()),
        }
    }

    let mut collector = NativeCollector::default();
    let snapshot = collector.collect()?;

    if let Some(database_url) = sqlite_url {
        create_sqlite_parent(&database_url)?;
        let captured_at_ms = now_ms()?;
        let store = SqliteHistoryStore::connect(&database_url).await?;
        let insert_result = store.insert_snapshot(captured_at_ms, &snapshot).await;
        let close_result = store.close().await;
        insert_result?;
        close_result?;
    }

    println!("{}", serde_json::to_string_pretty(&snapshot)?);
    Ok(())
}

async fn db(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let Some(subcommand) = args.first().map(String::as_str) else {
        return Err("db requires a subcommand: stats, check, vacuum, pre-image, or archive".into());
    };

    let (pre_image_action, archive_action, mut index) = match subcommand {
        "pre-image" => {
            let action = args
                .get(1)
                .map(String::as_str)
                .ok_or("db pre-image requires an action: status or remove")?;
            (Some(action), None, 2)
        }
        "archive" => {
            let action = args
                .get(1)
                .map(String::as_str)
                .ok_or("db archive requires an action: status or export-now")?;
            (None, Some(action), 2)
        }
        _ => (None, None, 1),
    };

    let mut sqlite_url: Option<String> = None;
    let mut yes = false;
    while index < args.len() {
        match args[index].as_str() {
            "--sqlite" => {
                sqlite_url = Some(normalize_sqlite_url(&require_value(
                    args, index, "--sqlite",
                )?)?);
                index += 2;
            }
            "--json" if subcommand == "stats" => {
                index += 1;
            }
            "--yes" if subcommand == "pre-image" && pre_image_action == Some("remove") => {
                yes = true;
                index += 1;
            }
            other => return Err(format!("unknown db option: {other}").into()),
        }
    }
    let sqlite_url = match sqlite_url {
        Some(url) => url,
        None => default_sqlite_url()?,
    };

    match subcommand {
        "stats" => {
            let store = connect_for_db_diagnostic(&sqlite_url).await?;
            let operation: Result<String, Box<dyn std::error::Error>> = async {
                let user_version = store.user_version().await?;
                if user_version < 1 {
                    return Err(Refused {
                        reason: format!(
                            "user_version check observed {user_version}; the schema v1 migration has not run — stop every TinyTop writer and start the daemon once (it takes the pre-image first; see INSTALL.md)"
                        ),
                        details: None,
                    }
                    .into());
                }
                let settings = store.get_settings().await?;
                let otel = OtelStatsOutput {
                    enabled: settings.otel.enabled,
                    endpoint: settings.otel.endpoint.clone(),
                    interval_sec: settings.otel.interval_sec,
                    headers_env_var: settings.otel.headers_env_var.clone(),
                    headers_env_var_set: std::env::var_os(&settings.otel.headers_env_var).is_some(),
                    service_name: settings.otel.service_name.clone(),
                };
                let coverage = store.history_coverage(&settings).await?;
                let stats = StoreStats {
                    sample_count: coverage.sample_count,
                    oldest_captured_at_ms: coverage.oldest_captured_at_ms,
                    newest_captured_at_ms: coverage.newest_captured_at_ms,
                };
                Ok(serde_json::to_string_pretty(&DbStatus {
                    status: "ok",
                    value: DbStatsOutput {
                        stats,
                        user_version,
                        tiers: coverage.tiers,
                        snapshot_json_sample_count: coverage.snapshot_json_sample_count,
                        archive: coverage.archive,
                        disk: coverage.disk,
                        otel,
                    },
                })?)
            }
            .await;
            let close_result = store.close().await;
            let output = operation?;
            close_result?;
            println!("{output}");
        }
        "check" => {
            let store = connect_for_db_diagnostic(&sqlite_url).await?;
            let operation: Result<String, Box<dyn std::error::Error>> = async {
                Ok(serde_json::to_string_pretty(&DbStatus {
                    status: "ok",
                    value: IntegrityResult {
                        result: store.integrity_check().await?,
                    },
                })?)
            }
            .await;
            let close_result = store.close().await;
            let output = operation?;
            close_result?;
            println!("{output}");
        }
        "vacuum" => {
            let store = connect_for_db_diagnostic(&sqlite_url).await?;
            let operation: Result<String, Box<dyn std::error::Error>> = async {
                store.vacuum().await?;
                Ok(serde_json::to_string_pretty(&DbStatus {
                    status: "ok",
                    value: VacuumResult { action: "vacuum" },
                })?)
            }
            .await;
            let close_result = store.close().await;
            let output = operation?;
            close_result?;
            println!("{output}");
        }
        "pre-image" => {
            let Some(action) = pre_image_action else {
                return Err("db pre-image requires an action: status or remove".into());
            };
            db_pre_image(action, &sqlite_url, yes).await?;
        }
        "archive" => {
            let Some(action) = archive_action else {
                return Err("db archive requires an action: status or export-now".into());
            };
            db_archive(action, &sqlite_url).await?;
        }
        unknown => return Err(format!("unknown db command: {unknown}").into()),
    }

    Ok(())
}

async fn config(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let Some(subcommand) = args.first().map(String::as_str) else {
        return Err("config requires a subcommand: export or import".into());
    };

    let mut sqlite_url: Option<String> = None;
    let mut output_path: Option<PathBuf> = None;
    let mut dry_run = false;
    let (input_path, mut index) = match subcommand {
        "export" => (None, 1),
        "import" => {
            let path = args
                .get(1)
                .filter(|value| !value.starts_with('-'))
                .ok_or("config import requires FILE as its first argument")?;
            (Some(PathBuf::from(path)), 2)
        }
        unknown => return Err(format!("unknown config command: {unknown}").into()),
    };

    while index < args.len() {
        match args[index].as_str() {
            "--sqlite" => {
                sqlite_url = Some(normalize_sqlite_url(&require_value(
                    args, index, "--sqlite",
                )?)?);
                index += 2;
            }
            "--out" if subcommand == "export" => {
                output_path = Some(PathBuf::from(require_value(args, index, "--out")?));
                index += 2;
            }
            "--dry-run" if subcommand == "import" => {
                dry_run = true;
                index += 1;
            }
            other => return Err(format!("unknown config option: {other}").into()),
        }
    }

    let sqlite_url = match sqlite_url {
        Some(url) => url,
        None => default_sqlite_url()?,
    };
    let store = connect_for_db_diagnostic(&sqlite_url).await?;
    let operation: Result<ConfigCommandOutput, Box<dyn std::error::Error>> = async {
        let user_version = store.user_version().await?;
        if user_version < 1 {
            return Err(Refused {
                reason: format!(
                    "user_version check observed {user_version}; the schema v1 migration has not run — stop every TinyTop writer and start the daemon once (it takes the pre-image first; see INSTALL.md)"
                ),
                details: None,
            }
            .into());
        }

        match subcommand {
            "export" => {
                let settings = store.get_settings().await?;
                let document =
                    export_document(&settings, now_ms()?, env!("CARGO_PKG_VERSION"));
                let mut contents = serde_json::to_string_pretty(&document)?;
                contents.push('\n');

                if let Some(path) = output_path {
                    refuse_existing_export_path(&path)?;
                    let temporary = suffixed_path(&path, ".tmp");
                    write_settings_export_temp(&temporary, &contents)?;
                    publish_settings_export_no_clobber(&temporary, &path)?;
                    let absolute_path = path.canonicalize().map_err(|error| {
                        format!(
                            "could not resolve exported settings path {}: {error}",
                            path.display()
                        )
                    })?;
                    Ok(ConfigCommandOutput::Success(serde_json::to_string_pretty(
                        &serde_json::json!({
                            "status": "ok",
                            "action": "export",
                            "path": absolute_path.display().to_string(),
                            "bytes": contents.len(),
                        }),
                    )?))
                } else {
                    Ok(ConfigCommandOutput::Success(contents))
                }
            }
            "import" => {
                let input_path = input_path
                    .as_ref()
                    .ok_or("config import requires FILE as its first argument")?;
                let contents = std::fs::read_to_string(input_path).map_err(|error| {
                    format!(
                        "could not read settings document {}: {error}",
                        input_path.display()
                    )
                })?;
                let document: serde_json::Value =
                    serde_json::from_str(&contents).map_err(|error| {
                        format!(
                            "could not parse settings document {} as JSON: {error}",
                            input_path.display()
                        )
                    })?;
                let plan = plan_import(&store, &document, now_ms()?).await?;
                if dry_run {
                    let output = serde_json::to_string_pretty(&plan)?;
                    return Ok(if plan.valid {
                        ConfigCommandOutput::Success(output)
                    } else {
                        ConfigCommandOutput::Failure(output)
                    });
                }
                if !plan.valid {
                    return Err(Refused {
                        reason: format!(
                            "settings document invalid: {}",
                            plan.errors.join("; ")
                        ),
                        details: Some(serde_json::to_value(&plan)?),
                    }
                    .into());
                }

                let outcome = apply_import(&store, &document, now_ms()?).await?;
                let (label, details) = import_marker(&outcome.changed_keys);
                store
                    .record_event(
                        now_ms()?,
                        tinytop_store::HistoryMarkerType::SettingsChange,
                        label,
                        details,
                    )
                    .await?;
                Ok(ConfigCommandOutput::Success(serde_json::to_string_pretty(
                    &serde_json::json!({
                        "status": "ok",
                        "action": "import",
                        "changedKeys": outcome.changed_keys,
                        "wouldDelete": outcome.would_delete,
                        "maintenance": "deferred to the daemon's next tick",
                    }),
                )?))
            }
            _ => unreachable!("config subcommand was validated before opening the store"),
        }
    }
    .await;
    let close_result = store.close().await;
    let output = operation?;
    close_result?;
    match output {
        ConfigCommandOutput::Success(output) => {
            print_config_output(&output);
            Ok(())
        }
        ConfigCommandOutput::Failure(output) => {
            print_config_output(&output);
            Err(OutputAlreadyReported.into())
        }
    }
}

enum ConfigCommandOutput {
    Success(String),
    Failure(String),
}

fn print_config_output(output: &str) {
    print!("{output}");
    if !output.ends_with('\n') {
        println!();
    }
}

fn refuse_existing_export_path(path: &std::path::Path) -> Result<(), Refused> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => Err(existing_export_refusal(path)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(Refused {
            reason: format!(
                "could not check whether settings export path {} exists: {error}",
                path.display()
            ),
            details: None,
        }),
    }
}

fn existing_export_refusal(path: &std::path::Path) -> Refused {
    Refused {
        reason: format!(
            "{} exists; remove it or choose another name — config export never overwrites",
            path.display()
        ),
        details: None,
    }
}

fn write_settings_export_temp(
    temporary: &std::path::Path,
    contents: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(temporary)
        .map_err(|error| {
            format!(
                "could not create temporary settings export {}: {error}",
                temporary.display()
            )
        })?;
    if let Err(error) = file.write_all(contents.as_bytes()) {
        let error = format!(
            "could not write temporary settings export {}: {error}",
            temporary.display()
        )
        .into();
        drop(file);
        return Err(remove_temp_after_failure(temporary, error));
    }
    if let Err(error) = file.sync_all() {
        let error = format!(
            "could not sync temporary settings export {}: {error}",
            temporary.display()
        )
        .into();
        drop(file);
        return Err(remove_temp_after_failure(temporary, error));
    }
    Ok(())
}

fn remove_temp_after_failure(
    temporary: &std::path::Path,
    error: Box<dyn std::error::Error>,
) -> Box<dyn std::error::Error> {
    match std::fs::remove_file(temporary) {
        Ok(()) => error,
        Err(cleanup_error) => format!(
            "{error}; temporary file {} could not be removed: {cleanup_error}",
            temporary.display()
        )
        .into(),
    }
}

fn publish_settings_export_no_clobber(
    temporary: &std::path::Path,
    target: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error>> {
    // `rename` is atomic but may replace a target created after a preflight check.
    // A same-directory hard link atomically publishes the synced inode only when
    // the final name is still absent; unlinking the temp then leaves one name.
    match std::fs::hard_link(temporary, target) {
        Ok(()) => {
            std::fs::remove_file(temporary).map_err(|error| {
                format!(
                    "settings export was published at {}, but temporary link {} could not be removed: {error}",
                    target.display(),
                    temporary.display()
                )
            })?;
            sync_settings_export_directory(target)
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            if let Err(cleanup_error) = std::fs::remove_file(temporary) {
                eprintln!(
                    "settings export refusal cleanup could not remove temporary link {}: {cleanup_error}",
                    temporary.display()
                );
            }
            Err(existing_export_refusal(target).into())
        }
        Err(error) if hard_link_error_supports_rename_fallback(&error) => {
            publish_by_rename_no_clobber(temporary, target)?;
            sync_settings_export_directory(target)
        }
        Err(error) => {
            if let Err(cleanup_error) = std::fs::remove_file(temporary) {
                eprintln!(
                    "settings export publication cleanup could not remove temporary link {}: {cleanup_error}",
                    temporary.display()
                );
            }
            Err(format!(
                "could not publish temporary settings export {} at {} without overwriting: {error}",
                temporary.display(),
                target.display()
            )
            .into())
        }
    }
}

fn hard_link_error_supports_rename_fallback(error: &std::io::Error) -> bool {
    if error.kind() == std::io::ErrorKind::Unsupported {
        return true;
    }
    if !matches!(
        error.kind(),
        std::io::ErrorKind::PermissionDenied | std::io::ErrorKind::Other
    ) {
        return false;
    }
    hard_link_raw_os_error_is_unsupported(error.raw_os_error())
}

#[cfg(unix)]
fn hard_link_raw_os_error_is_unsupported(raw_os_error: Option<i32>) -> bool {
    // EPERM is 1 on Unix. ENOTSUP/EOPNOTSUPP are 95 on Linux, 45 or 102
    // across the BSD/Darwin family; no dependency is needed just for errno names.
    matches!(raw_os_error, Some(1 | 45 | 95 | 102))
}

#[cfg(not(unix))]
fn hard_link_raw_os_error_is_unsupported(_raw_os_error: Option<i32>) -> bool {
    false
}

fn publish_by_rename_no_clobber(
    temporary: &std::path::Path,
    target: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error>> {
    match std::fs::symlink_metadata(target) {
        Ok(_) => {
            if let Err(cleanup_error) = std::fs::remove_file(temporary) {
                eprintln!(
                    "settings export refusal cleanup could not remove temporary file {}: {cleanup_error}",
                    temporary.display()
                );
            }
            return Err(existing_export_refusal(target).into());
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            if let Err(cleanup_error) = std::fs::remove_file(temporary) {
                eprintln!(
                    "settings export fallback cleanup could not remove temporary file {}: {cleanup_error}",
                    temporary.display()
                );
            }
            return Err(format!(
                "could not re-check settings export path {} before rename fallback: {error}",
                target.display()
            )
            .into());
        }
    }

    if let Err(error) = std::fs::rename(temporary, target) {
        if let Err(cleanup_error) = std::fs::remove_file(temporary) {
            eprintln!(
                "settings export fallback cleanup could not remove temporary file {}: {cleanup_error}",
                temporary.display()
            );
        }
        return Err(format!(
            "could not publish temporary settings export {} at {} by rename fallback: {error}",
            temporary.display(),
            target.display()
        )
        .into());
    }
    Ok(())
}

#[cfg(unix)]
fn sync_settings_export_directory(
    target: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let directory = target
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| std::path::Path::new("."));
    std::fs::File::open(directory)
        .and_then(|file| file.sync_all())
        .map_err(|error| {
            format!(
                "settings export was published at {} but its directory could not be synced: {}: {error}",
                target.display(),
                directory.display()
            )
            .into()
        })
}

#[cfg(not(unix))]
fn sync_settings_export_directory(
    _target: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error>> {
    Ok(())
}

fn suffixed_path(path: &std::path::Path, suffix: &str) -> PathBuf {
    let mut value = path.as_os_str().to_os_string();
    value.push(suffix);
    PathBuf::from(value)
}

async fn db_archive(action: &str, sqlite_url: &str) -> Result<(), Box<dyn std::error::Error>> {
    let store = connect_for_db_diagnostic(sqlite_url).await?;
    let operation: Result<String, Box<dyn std::error::Error>> = async {
        let user_version = store.user_version().await?;
        if user_version < 1 {
            return Err(Refused {
                reason: format!(
                    "user_version check observed {user_version}; the schema v1 migration has not run — stop every TinyTop writer and start the daemon once (it takes the pre-image first; see INSTALL.md)"
                ),
                details: None,
            }
            .into());
        }
        let settings = store.get_settings().await?;
        let paths = archive_paths(store.database_path(), &settings.retention_ladder.archive);
        match action {
            "status" => {
                let schema_state = archive_schema_state(&paths).await?;
                let coverage = store.history_coverage(&settings).await?;
                let manifest = read_archive_manifest(&paths).await?;
                let (next_exportable_months, reason) = if let ArchiveSchemaState::Incomplete {
                    user_version,
                    required_objects,
                } = schema_state
                {
                    (
                        None,
                        Some(format!(
                            "history-archive.sqlite at {} has user_version {user_version} but {required_objects} of 3 required objects; the next L4 move completes the schema; cold export refuses until then",
                            paths.db.display()
                        )),
                    )
                } else if !settings.retention_ladder.l4.enabled {
                    (
                        None,
                        Some(
                            "retentionLadder.l4.enabled is false; no L4 hours expire into the queryable archive — enable L4 with a finite retentionLadder.l4.keepDays"
                                .to_string(),
                        ),
                    )
                } else if settings.retention_ladder.l4.keep_days == 0 {
                    (
                        None,
                        Some(
                            "retentionLadder.l4.keepDays is 0 (forever); no L4 hours expire into the queryable archive — set a finite retentionLadder.l4.keepDays"
                                .to_string(),
                        ),
                    )
                } else {
                    let months_present = archive_months_present(&paths).await?;
                    let candidates = exportable_months(
                        &months_present,
                        &settings.retention_ladder,
                        coverage.archive.cold.exported_until_month.as_deref(),
                        now_ms()?,
                    );
                    let candidate_count = candidates.len().min(MAX_COLD_MONTHS_PER_PASS);
                    (
                        Some(
                            months_ready_to_export(&store, &candidates[..candidate_count]).await?,
                        ),
                        None,
                    )
                };
                Ok(serde_json::to_string_pretty(&DbStatus {
                    status: "ok",
                    value: ArchiveStatusOutput {
                        queryable: coverage.archive.queryable,
                        cold: ArchiveColdStatus {
                            coverage: coverage.archive.cold,
                            manifest,
                            next_exportable_months,
                            reason,
                        },
                    },
                })?)
            }
            "export-now" => {
                if !settings.retention_ladder.archive.cold {
                    return Err(Refused {
                        reason: "retentionLadder.archive.cold must be true for db archive export-now; observed false — enable retentionLadder.archive.cold and retry"
                            .to_string(),
                        details: None,
                    }
                    .into());
                }
                if !settings.retention_ladder.archive.queryable {
                    return Err(Refused {
                        reason: "retentionLadder.archive.queryable must be true for db archive export-now; observed false — enable retentionLadder.archive.queryable and retry"
                            .to_string(),
                        details: None,
                    }
                    .into());
                }
                let written = export_cold_months(
                    &store,
                    &paths,
                    &settings.retention_ladder,
                    now_ms()?,
                )
                .await?;
                Ok(serde_json::to_string_pretty(&DbStatus {
                    status: "ok",
                    value: ArchiveExportOutput {
                        action: "export-now",
                        written,
                    },
                })?)
            }
            unknown => Err(format!("unknown db archive action: {unknown}").into()),
        }
    }
    .await;
    let close_result = store.close().await;
    let output = operation?;
    close_result?;
    println!("{output}");
    Ok(())
}

async fn db_pre_image(
    action: &str,
    sqlite_url: &str,
    yes: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let inspected_database_path = inspect_database_path(sqlite_url)?;
    let raw_database_path = database_path_from_url(sqlite_url)?;
    let database_exists = inspected_database_path.is_some();
    let database_path = inspected_database_path.unwrap_or(raw_database_path);
    let path = pre_image_path(&database_path);
    let metadata = match std::fs::symlink_metadata(&path) {
        Ok(metadata) => Some(metadata),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error.into()),
    };
    let database_checks = if database_exists {
        let store = SqliteHistoryStore::connect_for_inspection(sqlite_url).await?;
        let operation = async {
            Ok::<_, tinytop_store::StoreError>((
                store.user_version().await?,
                store.integrity_check().await?,
            ))
        }
        .await;
        let close_result = store.close().await;
        let checks = operation?;
        close_result?;
        Some(checks)
    } else {
        None
    };

    match action {
        "status" => println!(
            "{}",
            serde_json::to_string_pretty(&DbStatus {
                status: "ok",
                value: PreImageStatus {
                    path: path.display().to_string(),
                    exists: metadata.is_some(),
                    bytes: metadata.as_ref().map(std::fs::Metadata::len),
                    database_exists,
                    user_version: database_checks.as_ref().map(|(version, _)| *version),
                    integrity_check: database_checks
                        .as_ref()
                        .map(|(_, integrity)| integrity.clone()),
                },
            })?
        ),
        "remove" => {
            let observed_database_checks = database_checks
                .as_ref()
                .map(|(version, integrity)| (*version, integrity.as_str()));
            if let Err(reason) = pre_image_remove_allowed(
                observed_database_checks,
                metadata.is_some(),
                yes,
                &database_path,
            ) {
                return Err(Refused {
                    reason: format!("{reason}; path is {}", path.display()),
                    details: None,
                }
                .into());
            }
            let Some(metadata) = metadata else {
                return Err("pre-image metadata disappeared after removal checks".into());
            };
            let bytes = metadata.len();
            std::fs::remove_file(&path)?;
            println!(
                "{}",
                serde_json::to_string_pretty(&DbStatus {
                    status: "ok",
                    value: PreImageRemoveResult {
                        action: "remove",
                        path: path.display().to_string(),
                        bytes,
                    },
                })?
            );
        }
        unknown => return Err(format!("unknown db pre-image action: {unknown}").into()),
    }

    Ok(())
}

fn pre_image_remove_allowed(
    database_checks: Option<(i64, &str)>,
    pre_image_exists: bool,
    yes: bool,
    database_path: &std::path::Path,
) -> Result<(), String> {
    if !yes {
        return Err(
            "pre-image remove --yes confirmation check observed false; pass --yes to confirm removal"
                .to_string(),
        );
    }
    if !pre_image_exists {
        return Err("pre-image exists check observed false; nothing can be removed".to_string());
    }
    let Some((user_version, integrity)) = database_checks else {
        return Err(format!(
            "database exists check observed false: database {} does not exist; the pre-image may be the only copy — restore or recreate the database before removing it",
            database_path.display()
        ));
    };
    if user_version < 1 {
        return Err(format!(
            "user_version check observed {user_version}; expected at least 1 because the schema v1 migration must run before removal"
        ));
    }
    if integrity != "ok" {
        return Err(format!(
            "integrity_check is {integrity:?}; expected \"ok\" before removing the pre-image"
        ));
    }
    Ok(())
}

async fn serve(args: &[String], defaults: ServeDefaults) -> Result<(), Box<dyn std::error::Error>> {
    let options = parse_serve_options(args, defaults)?;
    create_sqlite_parent(&options.sqlite_url)?;
    writer::serve(options).await?;
    Ok(())
}

fn parse_serve_options(
    args: &[String],
    defaults: ServeDefaults,
) -> Result<writer::ServeOptions, Box<dyn std::error::Error>> {
    let mut host = std::env::var(defaults.host_env).unwrap_or_else(|_| DEFAULT_HOST.to_string());
    let mut port = std::env::var(defaults.port_env)
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(defaults.default_port);
    let mut sqlite_url: Option<String> = None;
    let mut poll_ms = std::env::var("HISTORY_POLL_MS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(DEFAULT_POLL_MS);
    let mut dashboard_assets = if defaults.include_dashboard {
        dashboard_assets_from_env()
    } else {
        writer::DashboardAssets::Disabled
    };

    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--host" => {
                host = require_value(args, index, "--host")?;
                index += 2;
            }
            "--port" => {
                port = require_value(args, index, "--port")?.parse()?;
                index += 2;
            }
            "--sqlite" => {
                sqlite_url = Some(normalize_sqlite_url(&require_value(
                    args, index, "--sqlite",
                )?)?);
                index += 2;
            }
            "--poll-ms" => {
                poll_ms = require_value(args, index, "--poll-ms")?.parse()?;
                index += 2;
            }
            "--public-dir" => {
                dashboard_assets = writer::DashboardAssets::Directory(PathBuf::from(
                    require_value(args, index, "--public-dir")?,
                ));
                index += 2;
            }
            "--no-dashboard" => {
                dashboard_assets = writer::DashboardAssets::Disabled;
                index += 1;
            }
            other => return Err(format!("unknown serve option: {other}").into()),
        }
    }
    let sqlite_url = match sqlite_url {
        Some(url) => url,
        None => default_sqlite_url()?,
    };

    Ok(writer::ServeOptions {
        host,
        port,
        sqlite_url,
        poll_ms,
        dashboard_assets,
        embed_frame_ancestors: embed_frame_ancestors_from_env(),
    })
}

fn require_value(
    args: &[String],
    index: usize,
    option: &'static str,
) -> Result<String, Box<dyn std::error::Error>> {
    args.get(index + 1)
        .cloned()
        .ok_or_else(|| format!("{option} requires a value").into())
}

fn default_sqlite_url() -> Result<String, Box<dyn std::error::Error>> {
    default_sqlite_url_from_env(|key| std::env::var(key).ok(), std::env::consts::OS)
}

fn default_dashboard_port() -> u16 {
    default_dashboard_port_for_os(std::env::consts::OS)
}

fn default_dashboard_port_for_os(os: &str) -> u16 {
    if os == "windows" {
        DEFAULT_WINDOWS_DASHBOARD_PORT
    } else {
        DEFAULT_DASHBOARD_PORT
    }
}

fn default_sqlite_url_from_env<F>(lookup: F, os: &str) -> Result<String, Box<dyn std::error::Error>>
where
    F: Fn(&str) -> Option<String>,
{
    if let Some(value) = lookup("TINYTOP_HISTORY_DB") {
        return normalize_sqlite_url(&value);
    }

    let path = if os == "windows" {
        if let Some(local_app_data) = lookup("LOCALAPPDATA") {
            PathBuf::from(local_app_data)
                .join("TinyTop")
                .join("state")
                .join("history.sqlite")
        } else if let Some(user_profile) = lookup("USERPROFILE") {
            PathBuf::from(user_profile)
                .join("AppData")
                .join("Local")
                .join("TinyTop")
                .join("state")
                .join("history.sqlite")
        } else {
            return Err(
                "TINYTOP_HISTORY_DB is unset and neither LOCALAPPDATA nor USERPROFILE is available"
                    .into(),
            );
        }
    } else {
        let home = lookup("HOME").ok_or("TINYTOP_HISTORY_DB is unset and HOME is unavailable")?;
        PathBuf::from(home)
            .join(".local")
            .join("share")
            .join("tinytop")
            .join("history.sqlite")
    };

    normalize_sqlite_url(&path.to_string_lossy())
}

fn normalize_sqlite_url(value: &str) -> Result<String, Box<dyn std::error::Error>> {
    if value.starts_with("sqlite:") {
        return Ok(value.to_string());
    }

    let expanded = expand_home(value)?;
    Ok(format!("sqlite://{}", expanded.display()))
}

async fn connect_for_db_diagnostic(
    sqlite_url: &str,
) -> Result<SqliteHistoryStore, Box<dyn std::error::Error>> {
    if inspect_database_path(sqlite_url)?.is_some() {
        return Ok(SqliteHistoryStore::connect_for_inspection(sqlite_url).await?);
    }
    let database_path = database_path_from_url(sqlite_url)?;
    Err(Refused {
        reason: format!(
            "database {} does not exist; nothing was created — check the path or start the daemon once",
            database_path.display()
        ),
        details: None,
    }
    .into())
}

fn create_sqlite_parent(sqlite_url: &str) -> Result<(), Box<dyn std::error::Error>> {
    let database_path = database_path_from_url(sqlite_url)?;
    if let Some(parent) = database_path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)?;
    }
    Ok(())
}

fn expand_home(value: &str) -> Result<PathBuf, Box<dyn std::error::Error>> {
    if value == "~" {
        return Ok(PathBuf::from(std::env::var("HOME")?));
    }

    if let Some(rest) = value.strip_prefix("~/") {
        return Ok(PathBuf::from(std::env::var("HOME")?).join(rest));
    }

    Ok(PathBuf::from(value))
}

fn dashboard_assets_from_env() -> writer::DashboardAssets {
    std::env::var("TINYTOP_PUBLIC_DIR")
        .map(|path| writer::DashboardAssets::Directory(PathBuf::from(path)))
        .unwrap_or(writer::DashboardAssets::Embedded)
}

fn embed_frame_ancestors_from_env() -> String {
    std::env::var("TINYTOP_EMBED_FRAME_ANCESTORS").unwrap_or_else(|_| "'self'".to_string())
}

fn now_ms() -> Result<i64, Box<dyn std::error::Error>> {
    let duration = SystemTime::now().duration_since(UNIX_EPOCH)?;
    Ok(i64::try_from(duration.as_millis())?)
}

fn print_help() {
    println!(
        r#"TinyTop Rust collector

Usage:
  tinytop-agent collect [--json] [--sqlite <database-url>]
  tinytop-agent db stats [--json] [--sqlite <database-url>]
  tinytop-agent db check|vacuum [--sqlite <database-url>]
  tinytop-agent db pre-image status [--sqlite <database-url>]
  tinytop-agent db pre-image remove [--yes] [--sqlite <database-url>]
  tinytop-agent db archive status [--sqlite <database-url>]
  tinytop-agent db archive export-now [--sqlite <database-url>]
  tinytop-agent config export [--out <file>] [--sqlite <database-url>]
  tinytop-agent config import <file> [--dry-run] [--sqlite <database-url>]
  tinytop-agent serve [--host <host>] [--port <port>] [--sqlite <database-url>] [--poll-ms <ms>] [--public-dir <path>]
  tinytop-agent serve-writer [--host <host>] [--port <port>] [--sqlite <database-url>] [--poll-ms <ms>]
  tinytop-agent help

Examples:
  tinytop-agent collect --json
  tinytop-agent collect --sqlite sqlite::memory:
  tinytop-agent db stats
  tinytop-agent db pre-image remove --yes
  tinytop-agent db archive status
  tinytop-agent serve --host 127.0.0.1 --port 4274
  tinytop-agent serve-writer --host 127.0.0.1 --port 4276
"#
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn env_lookup(values: HashMap<&'static str, &'static str>) -> impl Fn(&str) -> Option<String> {
        move |key| values.get(key).map(|value| value.to_string())
    }

    #[test]
    fn windows_dashboard_default_port_avoids_wsl_dashboard_default() {
        assert_eq!(default_dashboard_port_for_os("windows"), 4275);
        assert_eq!(default_dashboard_port_for_os("linux"), 4274);
        assert_eq!(default_dashboard_port_for_os("macos"), 4274);
    }

    #[test]
    fn windows_default_sqlite_url_uses_localappdata_state_path() {
        let mut values = HashMap::new();
        values.insert("LOCALAPPDATA", r"C:\Users\michel\AppData\Local");

        let url = default_sqlite_url_from_env(env_lookup(values), "windows").unwrap();

        assert!(url.contains(r"C:\Users\michel\AppData\Local"));
        assert!(url.contains(r"TinyTop"));
        assert!(url.contains(r"state"));
        assert!(url.contains(r"history.sqlite"));
    }

    #[test]
    fn windows_default_sqlite_url_falls_back_to_userprofile() {
        let mut values = HashMap::new();
        values.insert("USERPROFILE", r"C:\Users\michel");

        let url = default_sqlite_url_from_env(env_lookup(values), "windows").unwrap();

        assert!(url.contains(r"C:\Users\michel"));
        assert!(url.contains(r"AppData"));
        assert!(url.contains(r"Local"));
        assert!(url.contains(r"TinyTop"));
        assert!(url.contains(r"history.sqlite"));
    }

    #[test]
    fn settings_export_directory_sync_succeeds_for_a_temp_directory() {
        // Break caught: a successful settings export is reported before its directory entry is durable.
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should be after the Unix epoch")
            .as_nanos();
        let directory = std::env::temp_dir().join(format!(
            "tinytop-config-directory-sync-{}-{stamp}",
            std::process::id()
        ));
        std::fs::create_dir_all(&directory).expect("fixture directory should be created");
        let target = directory.join("settings.json");

        sync_settings_export_directory(&target)
            .expect("the settings export directory should be syncable");

        std::fs::remove_dir_all(&directory).expect("fixture directory should be removable");
    }

    #[test]
    fn settings_export_failure_cleanup_removes_the_temporary_file() {
        // Break caught: a failed temporary-file write strands the create-new path for the next export.
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should be after the Unix epoch")
            .as_nanos();
        let directory = std::env::temp_dir().join(format!(
            "tinytop-config-temp-cleanup-{}-{stamp}",
            std::process::id()
        ));
        std::fs::create_dir_all(&directory).expect("fixture directory should be created");
        let temporary = directory.join("settings.json.tmp");
        std::fs::write(&temporary, b"partial").expect("temporary export should be written");

        let error = remove_temp_after_failure(&temporary, "original write failure".into());

        assert_eq!(error.to_string(), "original write failure");
        assert!(!temporary.exists());
        std::fs::remove_dir_all(&directory).expect("fixture directory should be removable");
    }

    #[test]
    fn settings_export_failure_cleanup_reports_a_stranded_path() {
        // Break caught: cleanup failure hides either the original error or the stranded path.
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should be after the Unix epoch")
            .as_nanos();
        let directory = std::env::temp_dir().join(format!(
            "tinytop-config-temp-stranded-{}-{stamp}",
            std::process::id()
        ));
        let temporary = directory.join("settings.json.tmp");
        std::fs::create_dir_all(&temporary).expect("temporary directory fixture should be created");
        std::fs::write(temporary.join("child"), b"not empty")
            .expect("temporary directory should be non-empty");

        let error = remove_temp_after_failure(&temporary, "original sync failure".into());
        let message = error.to_string();

        assert!(message.contains("original sync failure"));
        assert!(message.contains(&format!(
            "temporary file {} could not be removed",
            temporary.display()
        )));
        assert!(temporary.exists());
        std::fs::remove_dir_all(&directory).expect("fixture directory should be removable");
    }

    #[test]
    fn settings_export_publication_refuses_a_destination_created_after_temp_write() {
        // Break caught: a destination created after the preflight check is overwritten by rename.
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should be after the Unix epoch")
            .as_nanos();
        let directory = std::env::temp_dir().join(format!(
            "tinytop-config-publish-race-{}-{stamp}",
            std::process::id()
        ));
        std::fs::create_dir_all(&directory).expect("fixture directory should be created");
        let target = directory.join("settings.json");
        let temporary = suffixed_path(&target, ".tmp");
        std::fs::write(&temporary, b"candidate").expect("temporary export should be written");
        std::fs::write(&target, b"competitor").expect("competing destination should be written");

        let error = publish_settings_export_no_clobber(&temporary, &target)
            .expect_err("publication must refuse a destination that appeared after temp creation");
        let refusal = error
            .downcast_ref::<Refused>()
            .expect("publication race should retain the structured refusal");

        assert_eq!(
            refusal.reason,
            format!(
                "{} exists; remove it or choose another name — config export never overwrites",
                target.display()
            )
        );
        assert_eq!(
            std::fs::read(&target).expect("competing destination should remain readable"),
            b"competitor"
        );
        assert!(!temporary.exists());
        std::fs::remove_dir_all(&directory).expect("fixture directory should be removable");
    }

    #[test]
    fn settings_export_rename_fallback_publishes_an_absent_destination() {
        // Break caught: filesystems without hard-link support cannot publish an export at all.
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should be after the Unix epoch")
            .as_nanos();
        let directory = std::env::temp_dir().join(format!(
            "tinytop-config-rename-publish-{}-{stamp}",
            std::process::id()
        ));
        std::fs::create_dir_all(&directory).expect("fixture directory should be created");
        let target = directory.join("settings.json");
        let temporary = suffixed_path(&target, ".tmp");
        std::fs::write(&temporary, b"candidate").expect("temporary export should be written");

        publish_by_rename_no_clobber(&temporary, &target)
            .expect("rename fallback should publish an absent destination");

        assert_eq!(
            std::fs::read(&target).expect("published destination should be readable"),
            b"candidate"
        );
        assert!(!temporary.exists());
        std::fs::remove_dir_all(&directory).expect("fixture directory should be removable");
    }

    #[test]
    fn settings_export_rename_fallback_refuses_an_existing_destination() {
        // Break caught: the fallback replaces a destination found by its final pre-rename check.
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should be after the Unix epoch")
            .as_nanos();
        let directory = std::env::temp_dir().join(format!(
            "tinytop-config-rename-refuse-{}-{stamp}",
            std::process::id()
        ));
        std::fs::create_dir_all(&directory).expect("fixture directory should be created");
        let target = directory.join("settings.json");
        let temporary = suffixed_path(&target, ".tmp");
        std::fs::write(&temporary, b"candidate").expect("temporary export should be written");
        std::fs::write(&target, b"competitor").expect("competing destination should be written");

        let error = publish_by_rename_no_clobber(&temporary, &target)
            .expect_err("rename fallback must refuse an existing destination");
        let refusal = error
            .downcast_ref::<Refused>()
            .expect("rename fallback should retain the structured refusal");

        assert!(refusal.reason.contains("never overwrites"));
        assert_eq!(
            std::fs::read(&target).expect("competing destination should remain readable"),
            b"competitor"
        );
        assert!(!temporary.exists());
        std::fs::remove_dir_all(&directory).expect("fixture directory should be removable");
    }

    #[test]
    fn pre_image_remove_predicate_refuses_without_yes() {
        let reason = pre_image_remove_allowed(
            Some((1, "ok")),
            true,
            false,
            std::path::Path::new("h.sqlite"),
        )
        .expect_err("removal without --yes must be refused");

        assert!(reason.contains("--yes"));
        assert!(reason.contains("false"));
    }

    #[test]
    fn pre_image_remove_predicate_refuses_when_absent() {
        let reason = pre_image_remove_allowed(
            Some((1, "ok")),
            false,
            true,
            std::path::Path::new("h.sqlite"),
        )
        .expect_err("an absent pre-image must be refused");

        assert!(reason.contains("exists"));
        assert!(reason.contains("false"));
    }

    #[test]
    fn pre_image_remove_predicate_refuses_when_database_is_missing() {
        let reason = pre_image_remove_allowed(None, true, true, std::path::Path::new("h.sqlite"))
            .expect_err("a missing database must be refused");

        assert!(reason.contains("database exists"));
        assert!(reason.contains("false"));
        assert!(reason.contains("h.sqlite"));
    }

    #[test]
    fn pre_image_remove_predicate_refuses_before_schema_v1() {
        let reason = pre_image_remove_allowed(
            Some((0, "ok")),
            true,
            true,
            std::path::Path::new("h.sqlite"),
        )
        .expect_err("removal before schema v1 must be refused");

        assert!(reason.contains("user_version"));
        assert!(reason.contains('0'));
    }

    #[test]
    fn pre_image_remove_predicate_refuses_failed_integrity_check() {
        let reason = pre_image_remove_allowed(
            Some((1, "database disk image is malformed")),
            true,
            true,
            std::path::Path::new("h.sqlite"),
        )
        .expect_err("removal after a failed integrity check must be refused");

        assert!(reason.contains("integrity_check"));
        assert!(reason.contains("database disk image is malformed"));
    }

    #[test]
    fn pre_image_remove_predicate_allows_every_check_to_pass() {
        pre_image_remove_allowed(
            Some((1, "ok")),
            true,
            true,
            std::path::Path::new("h.sqlite"),
        )
        .expect("removal should be allowed after every check passes");
    }
}
