use std::{
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::Serialize;
use tinytop_collectors::linux::LinuxCollector;
use tinytop_store::SqliteHistoryStore;

mod writer;

const DEFAULT_HOST: &str = "127.0.0.1";
const DEFAULT_DASHBOARD_PORT: u16 = 4274;
const DEFAULT_WRITER_PORT: u16 = 4276;
const DEFAULT_POLL_MS: u64 = 1500;

#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("{error}");
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
    #[serde(flatten)]
    value: T,
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
            default_port: DEFAULT_DASHBOARD_PORT,
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

    let mut collector = LinuxCollector::default();
    let snapshot = collector.collect()?;

    if let Some(database_url) = sqlite_url {
        let store = SqliteHistoryStore::connect(&database_url).await?;
        store.insert_snapshot(now_ms()?, &snapshot).await?;
    }

    println!("{}", serde_json::to_string_pretty(&snapshot)?);
    Ok(())
}

async fn db(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let Some(subcommand) = args.first().map(String::as_str) else {
        return Err("db requires a subcommand: stats, check, or vacuum".into());
    };

    let mut sqlite_url = default_sqlite_url()?;
    let mut index = 1;
    while index < args.len() {
        match args[index].as_str() {
            "--sqlite" => {
                sqlite_url = normalize_sqlite_url(&require_value(args, index, "--sqlite")?)?;
                index += 2;
            }
            other => return Err(format!("unknown db option: {other}").into()),
        }
    }

    let store = SqliteHistoryStore::connect(&sqlite_url).await?;
    match subcommand {
        "stats" => println!(
            "{}",
            serde_json::to_string_pretty(&DbStatus {
                status: "ok",
                value: store.stats().await?,
            })?
        ),
        "check" => println!(
            "{}",
            serde_json::to_string_pretty(&DbStatus {
                status: "ok",
                value: IntegrityResult {
                    result: store.integrity_check().await?,
                },
            })?
        ),
        "vacuum" => {
            store.vacuum().await?;
            println!(
                "{}",
                serde_json::to_string_pretty(&DbStatus {
                    status: "ok",
                    value: VacuumResult { action: "vacuum" },
                })?
            );
        }
        unknown => return Err(format!("unknown db command: {unknown}").into()),
    }

    Ok(())
}

async fn serve(args: &[String], defaults: ServeDefaults) -> Result<(), Box<dyn std::error::Error>> {
    let options = parse_serve_options(args, defaults)?;
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
    let mut sqlite_url = default_sqlite_url()?;
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
                sqlite_url = normalize_sqlite_url(&require_value(args, index, "--sqlite")?)?;
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

    Ok(writer::ServeOptions {
        host,
        port,
        sqlite_url,
        poll_ms,
        dashboard_assets,
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
    if let Ok(value) = std::env::var("TINYTOP_HISTORY_DB") {
        return normalize_sqlite_url(&value);
    }

    let home = std::env::var("HOME")?;
    normalize_sqlite_url(&format!("{home}/.local/share/tinytop/history.sqlite"))
}

fn normalize_sqlite_url(value: &str) -> Result<String, Box<dyn std::error::Error>> {
    if value.starts_with("sqlite:") {
        return Ok(value.to_string());
    }

    let expanded = expand_home(value)?;
    if let Some(parent) = expanded.parent() {
        std::fs::create_dir_all(parent)?;
    }
    Ok(format!("sqlite://{}", expanded.display()))
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

fn now_ms() -> Result<i64, Box<dyn std::error::Error>> {
    let duration = SystemTime::now().duration_since(UNIX_EPOCH)?;
    Ok(i64::try_from(duration.as_millis())?)
}

fn print_help() {
    println!(
        r#"TinyTop Rust collector

Usage:
  tinytop-agent collect [--json] [--sqlite <database-url>]
  tinytop-agent db stats|check|vacuum [--sqlite <database-url>]
  tinytop-agent serve [--host <host>] [--port <port>] [--sqlite <database-url>] [--poll-ms <ms>] [--public-dir <path>]
  tinytop-agent serve-writer [--host <host>] [--port <port>] [--sqlite <database-url>] [--poll-ms <ms>]
  tinytop-agent help

Examples:
  tinytop-agent collect --json
  tinytop-agent collect --sqlite sqlite::memory:
  tinytop-agent db stats
  tinytop-agent serve --host 127.0.0.1 --port 4274
  tinytop-agent serve-writer --host 127.0.0.1 --port 4276
"#
    );
}
