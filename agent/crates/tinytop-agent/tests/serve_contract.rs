use std::{
    fs,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    path::PathBuf,
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

#[test]
fn serve_exposes_dashboard_and_history_api() {
    let port = reserve_port();
    let public_dir = temp_public_dir();
    let mut child = Command::new(env!("CARGO_BIN_EXE_tinytop-agent"))
        .args([
            "serve",
            "--host",
            "127.0.0.1",
            "--port",
            &port.to_string(),
            "--sqlite",
            "sqlite::memory:",
            "--poll-ms",
            "100000",
            "--public-dir",
            public_dir
                .to_str()
                .expect("public path should be valid UTF-8"),
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("tinytop-agent serve should start");

    let result = wait_for_server(port)
        .and_then(|_| http_get(port, "/health"))
        .map(|response| {
            assert!(response.contains(r#""status":"ok""#));
            assert!(response.contains(r#""sqlitePath":"memory""#));
            assert!(response.contains(r#""os":"#));
        })
        .and_then(|_| http_get(port, "/"))
        .map(|response| assert!(response.contains("TinyTop test dashboard")))
        .and_then(|_| http_get(port, "/api/history?limit=1"))
        .map(|response| assert!(response.contains("\"samples\"")));

    stop_child(&mut child);
    fs::remove_dir_all(public_dir).ok();

    result.expect("server should expose dashboard and history API");
}

#[test]
fn serve_exposes_embedded_dashboard_without_public_dir() {
    let port = reserve_port();
    let cwd = temp_empty_dir("tinytop-agent-empty-cwd");
    let mut child = Command::new(env!("CARGO_BIN_EXE_tinytop-agent"))
        .args([
            "serve",
            "--host",
            "127.0.0.1",
            "--port",
            &port.to_string(),
            "--sqlite",
            "sqlite::memory:",
            "--poll-ms",
            "100000",
        ])
        .current_dir(&cwd)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("tinytop-agent serve should start from a cwd without dashboard files");

    let result = wait_for_server(port)
        .and_then(|_| http_get(port, "/"))
        .map(|response| assert!(response.contains("<title>TinyTop</title>")))
        .and_then(|_| http_get(port, "/embed?theme=dark"))
        .map(|response| assert!(response.contains("<title>TinyTop</title>")))
        .and_then(|_| http_get(port, "/styles.css"))
        .map(|response| assert!(response.contains("status-message")))
        .and_then(|_| http_get(port, "/app.js"))
        .map(|response| assert!(response.contains("requestConfirmation")))
        .and_then(|_| http_get(port, "/favicon.svg"))
        .map(|response| {
            assert!(response.contains("<svg"));
            assert!(response.contains("TinyTop"));
        })
        .and_then(|_| http_get(port, "/vendor/echarts.min.js"))
        .map(|response| assert!(response.contains("echarts")));

    stop_child(&mut child);
    fs::remove_dir_all(cwd).ok();

    result.expect("server should expose embedded dashboard assets without --public-dir");
}

#[test]
fn serve_exposes_embed_dashboard_with_configurable_frame_ancestors() {
    let port = reserve_port();
    let mut child = Command::new(env!("CARGO_BIN_EXE_tinytop-agent"))
        .env(
            "TINYTOP_EMBED_FRAME_ANCESTORS",
            "'self' http://127.0.0.1:9323",
        )
        .args([
            "serve",
            "--host",
            "127.0.0.1",
            "--port",
            &port.to_string(),
            "--sqlite",
            "sqlite::memory:",
            "--poll-ms",
            "100000",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("tinytop-agent serve should start");

    let result =
        wait_for_server(port)
            .and_then(|_| http_get_raw(port, "/embed?theme=dark"))
            .map(|response| {
                assert!(response.contains("http/1.1 200 ok"));
                assert!(response.contains(
                    "content-security-policy: frame-ancestors 'self' http://127.0.0.1:9323"
                ));
                assert!(response.contains("<title>tinytop</title>"));
                assert!(response.contains(r#"id="main""#));
            })
            .and_then(|_| http_get(port, "/api/version"))
            .map(|response| {
                assert!(response.contains(r#""dashboard":"embedded""#));
                assert!(response.contains(r#""capabilities":["snapshot","history","embed"]"#));
            });

    stop_child(&mut child);

    result.expect("server should expose framed embed dashboard");
}

#[test]
fn serve_sets_frame_ancestors_on_dashboard_html_routes() {
    let port = reserve_port();
    let mut child = Command::new(env!("CARGO_BIN_EXE_tinytop-agent"))
        .args([
            "serve",
            "--host",
            "127.0.0.1",
            "--port",
            &port.to_string(),
            "--sqlite",
            "sqlite::memory:",
            "--poll-ms",
            "100000",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("tinytop-agent serve should start");

    let result = wait_for_server(port)
        .and_then(|_| http_get_raw(port, "/"))
        .map(|response| {
            assert!(response.contains("http/1.1 200 ok"));
            assert!(
                response.contains("content-security-policy: frame-ancestors 'self'"),
                "root dashboard HTML must carry a frame-ancestors CSP, got {response}"
            );
        })
        .and_then(|_| http_get_raw(port, "/index.html"))
        .map(|response| {
            assert!(
                response.contains("content-security-policy: frame-ancestors 'self'"),
                "/index.html must carry a frame-ancestors CSP, got {response}"
            );
        })
        .and_then(|_| http_get_raw(port, "/styles.css"))
        .map(|response| {
            assert!(
                !response.contains("content-security-policy"),
                "non-HTML assets must not carry a frame-ancestors CSP, got {response}"
            );
        });

    stop_child(&mut child);

    result.expect("server should set frame-ancestors on dashboard HTML routes");
}

#[test]
fn serve_respects_empty_explicit_history_bounds() {
    let port = reserve_port();
    let mut child = Command::new(env!("CARGO_BIN_EXE_tinytop-agent"))
        .args([
            "serve",
            "--host",
            "127.0.0.1",
            "--port",
            &port.to_string(),
            "--sqlite",
            "sqlite::memory:",
            "--poll-ms",
            "100000",
            "--no-dashboard",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("tinytop-agent serve should start");

    let result = wait_for_server(port)
        .and_then(|_| http_get(port, "/api/history?limit=5&since_ms=0&until_ms=1"))
        .map(|response| {
            assert!(
                response.contains(r#""samples":[]"#),
                "explicitly bounded empty history window should stay empty, got {response}"
            );
        });

    stop_child(&mut child);

    result.expect("server should preserve explicit empty history bounds");
}

#[test]
fn serve_exposes_version_identity() {
    let port = reserve_port();
    let mut child = Command::new(env!("CARGO_BIN_EXE_tinytop-agent"))
        .args([
            "serve",
            "--host",
            "127.0.0.1",
            "--port",
            &port.to_string(),
            "--sqlite",
            "sqlite::memory:",
            "--poll-ms",
            "100000",
            "--no-dashboard",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("tinytop-agent serve should start");

    let result = wait_for_server(port)
        .and_then(|_| http_get(port, "/api/version"))
        .map(|response| {
            let version = product_version();
            assert!(
                response.contains(&format!(r#""version":"{version}""#)),
                "version endpoint should report product version {version}, got {response}"
            );
            assert!(response.contains(r#""runtime":"rust""#));
            assert!(response.contains(r#""component":"collector-dashboard-daemon""#));
            assert!(response.contains(r#""dashboard":"disabled""#));
            assert!(response.contains(r#""daemon":{"#));
            assert!(response.contains(r#""install":{"#));
            assert!(response.contains(r#""storage":{"#));
            assert!(response.contains(r#""sqliteUrl":"sqlite::memory:""#));
            assert!(response.contains(r#""sqlitePath":"memory""#));
            assert!(response.contains(r#""bind":{"#));
            assert!(response.contains(&format!(r#""port":{port}"#)));
        })
        .and_then(|_| http_get(port, "/version"))
        .map(|response| {
            assert!(response.contains(r#""runtime":"rust""#));
            assert!(response.contains(r#""component":"collector-dashboard-daemon""#));
        });

    stop_child(&mut child);

    result.expect("server should expose version identity");
}

#[test]
fn serve_persists_dashboard_settings_api() {
    let port = reserve_port();
    let mut child = Command::new(env!("CARGO_BIN_EXE_tinytop-agent"))
        .args([
            "serve",
            "--host",
            "127.0.0.1",
            "--port",
            &port.to_string(),
            "--sqlite",
            "sqlite::memory:",
            "--poll-ms",
            "100000",
            "--no-dashboard",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("tinytop-agent serve should start");

    let settings = r#"{"defaultTheme":"aurora","defaultGraphMode":"heatmap","pollIntervalMs":3000,"defaultHistoryWindow":"1h","retentionHours":96,"rollupRetentionDays":30,"topProcessCount":12,"redactionDefault":true,"thresholds":{"cpuWarn":75,"memoryWarn":82,"diskWarn":88},"enabledSections":{"overview":true,"history":true,"filesystem":true,"pressure":true,"processes":true}}"#;

    let result = wait_for_server(port)
        .and_then(|_| http_get(port, "/api/settings"))
        .map(|response| {
            assert!(response.contains(r#""defaultTheme":"midnight""#));
            assert!(response.contains(r#""pollIntervalMs":1500"#));
        })
        .and_then(|_| http_request(port, "PUT", "/api/settings", Some(settings)))
        .map(|response| {
            assert!(response.contains(r#""defaultTheme":"aurora""#));
            assert!(response.contains(r#""defaultGraphMode":"heatmap""#));
            assert!(response.contains(r#""pollIntervalMs":3000"#));
            assert!(response.contains(r#""redactionDefault":true"#));
        })
        .and_then(|_| http_get(port, "/api/settings"))
        .map(|response| {
            assert!(response.contains(r#""defaultTheme":"aurora""#));
            assert!(response.contains(r#""retentionHours":96"#));
            assert!(response.contains(r#""topProcessCount":12"#));
        });

    stop_child(&mut child);

    result.expect("server should persist dashboard settings through the API");
}

#[test]
fn serve_rejects_invalid_dashboard_settings_api() {
    let port = reserve_port();
    let mut child = Command::new(env!("CARGO_BIN_EXE_tinytop-agent"))
        .args([
            "serve",
            "--host",
            "127.0.0.1",
            "--port",
            &port.to_string(),
            "--sqlite",
            "sqlite::memory:",
            "--poll-ms",
            "100000",
            "--no-dashboard",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("tinytop-agent serve should start");

    let settings = r#"{"defaultTheme":"midnight","defaultGraphMode":"line","pollIntervalMs":100,"defaultHistoryWindow":"live","retentionHours":72,"rollupRetentionDays":30,"topProcessCount":8,"redactionDefault":false,"thresholds":{"cpuWarn":80,"memoryWarn":85,"diskWarn":85},"enabledSections":{"overview":true,"history":true,"filesystem":true,"pressure":true,"processes":true}}"#;

    let result = wait_for_server(port)
        .and_then(|_| http_request(port, "PUT", "/api/settings", Some(settings)))
        .map(|response| {
            assert!(
                response.starts_with("HTTP/1.1 400"),
                "invalid settings should return HTTP 400, got {response}"
            );
            assert!(response.contains("pollIntervalMs"));
        });

    stop_child(&mut child);

    result.expect("server should reject invalid dashboard settings");
}

#[test]
fn serve_exposes_history_coverage_api() {
    let port = reserve_port();
    let mut child = Command::new(env!("CARGO_BIN_EXE_tinytop-agent"))
        .args([
            "serve",
            "--host",
            "127.0.0.1",
            "--port",
            &port.to_string(),
            "--sqlite",
            "sqlite::memory:",
            "--poll-ms",
            "100000",
            "--no-dashboard",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("tinytop-agent serve should start");

    let result = wait_for_server(port)
        .and_then(|_| http_get(port, "/api/history/coverage"))
        .map(|response| {
            assert!(
                response.starts_with("HTTP/1.1 200"),
                "coverage endpoint should return HTTP 200, got {response}"
            );
            assert!(response.contains(r#""sampleCount":"#));
            assert!(response.contains(r#""retentionHours":72"#));
            assert!(response.contains(r#""rollupRetentionDays":30"#));
            assert!(response.contains(r#""rollupBucketCount":"#));
            assert!(response.contains(r#""databaseBytes":"#));
            assert!(response.contains(r#""targetDatabaseBytes":134217728"#));
            assert!(response.contains(r#""databaseBudgetPercent":"#));
        });

    stop_child(&mut child);

    result.expect("server should expose history coverage");
}

#[test]
fn serve_exposes_rollup_history_points_and_markers_api() {
    let port = reserve_port();
    let mut child = Command::new(env!("CARGO_BIN_EXE_tinytop-agent"))
        .args([
            "serve",
            "--host",
            "127.0.0.1",
            "--port",
            &port.to_string(),
            "--sqlite",
            "sqlite::memory:",
            "--poll-ms",
            "100000",
            "--no-dashboard",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("tinytop-agent serve should start");

    let settings = r#"{"defaultTheme":"aurora","defaultGraphMode":"line","pollIntervalMs":3000,"defaultHistoryWindow":"7d","retentionHours":96,"rollupRetentionDays":45,"targetDatabaseBytes":268435456,"topProcessCount":12,"redactionDefault":true,"thresholds":{"cpuWarn":75,"memoryWarn":82,"diskWarn":88},"enabledSections":{"overview":true,"history":true,"filesystem":true,"pressure":true,"processes":true}}"#;

    let result = wait_for_server(port)
        .and_then(|_| http_get(port, "/api/history/points?limit=5&source=rollup"))
        .map(|response| {
            assert!(
                response.starts_with("HTTP/1.1 200"),
                "points endpoint should return HTTP 200, got {response}"
            );
            assert!(response.contains(r#""points":["#));
            assert!(response.contains(r#""source":"rollup""#));
        })
        .and_then(|_| http_get(port, "/api/history/markers?limit=10&expected_gap_ms=60000"))
        .map(|response| {
            assert!(response.contains(r#""markers":["#));
            assert!(
                response.contains(r#""markerType":"daemonStart""#),
                "startup should write a daemonStart marker, got {response}"
            );
        })
        .and_then(|_| http_request(port, "PUT", "/api/settings", Some(settings)))
        .map(|response| {
            assert!(response.contains(r#""defaultHistoryWindow":"7d""#));
            assert!(response.contains(r#""targetDatabaseBytes":268435456"#));
        })
        .and_then(|_| http_get(port, "/api/history/markers?limit=10&expected_gap_ms=60000"))
        .map(|response| {
            assert!(
                response.contains(r#""markerType":"settingsChange""#),
                "settings PUT should write a settingsChange marker, got {response}"
            );
            assert!(response.contains("targetDatabaseBytes"));
        });

    stop_child(&mut child);

    result.expect("server should expose history points and markers");
}

fn reserve_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("should reserve a local port");
    listener
        .local_addr()
        .expect("reserved listener should have an address")
        .port()
}

fn temp_empty_dir(prefix: &str) -> std::path::PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be after epoch")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("{prefix}-{stamp}"));
    fs::create_dir_all(&dir).expect("temp directory should be created");
    dir
}

fn temp_public_dir() -> std::path::PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be after epoch")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("tinytop-agent-public-{stamp}"));
    fs::create_dir_all(dir.join("vendor")).expect("public vendor directory should be created");
    fs::write(dir.join("index.html"), "TinyTop test dashboard").expect("index should be written");
    fs::write(dir.join("styles.css"), "body{}").expect("styles should be written");
    fs::write(dir.join("app.js"), "console.log('test')").expect("app should be written");
    fs::write(dir.join("vendor/echarts.min.js"), "window.echarts = {};")
        .expect("vendor script should be written");
    dir
}

fn product_version() -> String {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    fs::read_to_string(manifest_dir.join("../../../VERSION"))
        .expect("workspace VERSION should be readable")
        .trim()
        .to_string()
}

fn wait_for_server(port: u16) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        if let Ok(response) = http_get(port, "/health")
            && response.contains(r#""status":"ok""#)
        {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(100));
    }
    Err("server did not become healthy".to_string())
}

fn http_get(port: u16, path: &str) -> Result<String, String> {
    http_request(port, "GET", path, None)
}

fn http_get_raw(port: u16, path: &str) -> Result<String, String> {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).map_err(|error| error.to_string())?;
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n"
    )
    .map_err(|error| error.to_string())?;
    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .map_err(|error| error.to_string())?;
    Ok(response.to_ascii_lowercase())
}

fn http_request(port: u16, method: &str, path: &str, body: Option<&str>) -> Result<String, String> {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).map_err(|error| error.to_string())?;
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(|error| error.to_string())?;
    if let Some(body) = body {
        write!(
            stream,
            "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
        .map_err(|error| error.to_string())?;
    } else {
        write!(
            stream,
            "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n"
        )
        .map_err(|error| error.to_string())?;
    }
    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .map_err(|error| error.to_string())?;
    Ok(response)
}

fn stop_child(child: &mut Child) {
    child.kill().ok();
    child.wait().ok();
}
