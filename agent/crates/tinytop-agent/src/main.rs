use std::time::{SystemTime, UNIX_EPOCH};

use tinytop_collectors::linux::LinuxCollector;
use tinytop_store::SqliteHistoryStore;

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
        "help" | "--help" | "-h" => {
            print_help();
            Ok(())
        }
        unknown => Err(format!("unknown command: {unknown}").into()),
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

fn now_ms() -> Result<i64, Box<dyn std::error::Error>> {
    let duration = SystemTime::now().duration_since(UNIX_EPOCH)?;
    Ok(i64::try_from(duration.as_millis())?)
}

fn print_help() {
    println!(
        r#"TinyTop Rust agent

Usage:
  tinytop-agent collect [--json] [--sqlite <database-url>]
  tinytop-agent help

Examples:
  tinytop-agent collect --json
  tinytop-agent collect --sqlite sqlite::memory:
"#
    );
}
