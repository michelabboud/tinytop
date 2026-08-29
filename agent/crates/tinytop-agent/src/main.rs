use std::{
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::Serialize;
use tinytop_collectors::NativeCollector;
use tinytop_store::{
    HistoryArchiveCoverage, HistoryDiskCoverage, HistoryTierCoverage, SqliteHistoryStore,
    StoreStats, database_path_from_url, inspect_database_path, pre_image_path,
};

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
            println!(
                "{}",
                serde_json::json!({ "status": "refused", "reason": refusal.reason })
            );
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
    tiers: Vec<HistoryTierCoverage>,
    snapshot_json_sample_count: i64,
    archive: HistoryArchiveCoverage,
    disk: HistoryDiskCoverage,
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

#[derive(Debug)]
struct Refused {
    reason: String,
}

impl std::fmt::Display for Refused {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.reason)
    }
}

impl std::error::Error for Refused {}

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
        let store = SqliteHistoryStore::connect(&database_url).await?;
        store.insert_snapshot(now_ms()?, &snapshot).await?;
    }

    println!("{}", serde_json::to_string_pretty(&snapshot)?);
    Ok(())
}

async fn db(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let Some(subcommand) = args.first().map(String::as_str) else {
        return Err("db requires a subcommand: stats, check, vacuum, or pre-image".into());
    };

    let (pre_image_action, mut index) = if subcommand == "pre-image" {
        let action = args
            .get(1)
            .map(String::as_str)
            .ok_or("db pre-image requires an action: status or remove")?;
        (Some(action), 2)
    } else {
        (None, 1)
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
            let (store, database_existed) = connect_for_db_diagnostic(&sqlite_url).await?;
            if database_existed {
                let user_version = store.user_version().await?;
                if user_version < 1 {
                    return Err(Refused {
                        reason: format!(
                            "user_version check observed {user_version}; the schema v1 migration has not run — stop every TinyTop writer and start the daemon once (it takes the pre-image first; see INSTALL.md)"
                        ),
                    }
                    .into());
                }
            }
            let settings = store.get_settings().await?;
            let coverage = store.history_coverage(&settings).await?;
            let stats = StoreStats {
                sample_count: coverage.sample_count,
                oldest_captured_at_ms: coverage.oldest_captured_at_ms,
                newest_captured_at_ms: coverage.newest_captured_at_ms,
            };
            println!(
                "{}",
                serde_json::to_string_pretty(&DbStatus {
                    status: "ok",
                    value: DbStatsOutput {
                        stats,
                        tiers: coverage.tiers,
                        snapshot_json_sample_count: coverage.snapshot_json_sample_count,
                        archive: coverage.archive,
                        disk: coverage.disk,
                    },
                })?
            );
        }
        "check" => {
            let (store, _) = connect_for_db_diagnostic(&sqlite_url).await?;
            println!(
                "{}",
                serde_json::to_string_pretty(&DbStatus {
                    status: "ok",
                    value: IntegrityResult {
                        result: store.integrity_check().await?,
                    },
                })?
            );
        }
        "vacuum" => {
            let (store, _) = connect_for_db_diagnostic(&sqlite_url).await?;
            store.vacuum().await?;
            println!(
                "{}",
                serde_json::to_string_pretty(&DbStatus {
                    status: "ok",
                    value: VacuumResult { action: "vacuum" },
                })?
            );
        }
        "pre-image" => {
            let Some(action) = pre_image_action else {
                return Err("db pre-image requires an action: status or remove".into());
            };
            db_pre_image(action, &sqlite_url, yes).await?;
        }
        unknown => return Err(format!("unknown db command: {unknown}").into()),
    }

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
        Some((store.user_version().await?, store.integrity_check().await?))
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
) -> Result<(SqliteHistoryStore, bool), Box<dyn std::error::Error>> {
    if inspect_database_path(sqlite_url)?.is_some() {
        return Ok((
            SqliteHistoryStore::connect_for_inspection(sqlite_url).await?,
            true,
        ));
    }
    create_sqlite_parent(sqlite_url)?;
    Ok((SqliteHistoryStore::connect(sqlite_url).await?, false))
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
  tinytop-agent serve [--host <host>] [--port <port>] [--sqlite <database-url>] [--poll-ms <ms>] [--public-dir <path>]
  tinytop-agent serve-writer [--host <host>] [--port <port>] [--sqlite <database-url>] [--poll-ms <ms>]
  tinytop-agent help

Examples:
  tinytop-agent collect --json
  tinytop-agent collect --sqlite sqlite::memory:
  tinytop-agent db stats
  tinytop-agent db pre-image remove --yes
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
